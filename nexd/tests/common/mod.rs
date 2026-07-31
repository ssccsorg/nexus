// ── Test helpers for nexd integration tests ──────────────────────────────
//
// Provides DaemonHandle: starts nexd as child process, manages lifecycle.
// Reads responses line-by-line (nexd uses line-delimited JSON).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Path to nexd binary. Set by cargo at compile time for integration tests.
const NEXD_BIN: &str = env!("CARGO_BIN_EXE_nexd");

/// Path to nex-server binary, derived from nexd binary path.
/// Builds nex-server automatically if not found.
pub fn nex_server_bin() -> std::path::PathBuf {
    let nexd_path = std::path::Path::new(NEXD_BIN);
    // nexd and nex-server are in the same target/{profile}/ directory
    if let Some(parent) = nexd_path.parent() {
        let candidate = parent.join("nex-server");
        if candidate.exists() {
            return candidate;
        }
    }
    // Auto-build nex-server if missing
    eprintln!("nex-server binary not found, building with 'cargo build -p nex-server'...");
    let status = std::process::Command::new("cargo")
        .args(["build", "-p", "nex-server", "-q"])
        .status()
        .expect("failed to run cargo build");
    if !status.success() {
        panic!("cargo build -p nex-server failed (exit: {})", status);
    }
    // Retry lookup after build
    if let Some(parent) = nexd_path.parent() {
        let candidate = parent.join("nex-server");
        if candidate.exists() {
            return candidate;
        }
    }
    // Final fallback: workspace root
    let candidate = std::path::PathBuf::from("./target/debug/nex-server");
    if candidate.exists() {
        return candidate;
    }
    panic!(
        "nex-server still not found after build. \
         Tried parent path: {} and ./target/debug/nex-server",
        nexd_path
            .parent()
            .map(|p| p.join("nex-server").display().to_string())
            .unwrap_or_default()
    );
}

/// Manages a nexd daemon instance for testing.
pub struct DaemonHandle {
    child: Option<Child>,
    nex_child: Option<Child>,
    pub socket_path: PathBuf,
    #[allow(dead_code)]
    pub temp_dir: tempfile::TempDir,
}

impl DaemonHandle {
    /// Start nexd with a unique socket in a temp dir.
    /// Also starts nex-server as a child process.
    pub fn start() -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let socket_path = temp_dir.path().join("nexd.sock");
        let nex_server_socket = temp_dir.path().join("nex-server.sock");

        // Start nex-server first with unique data dir
        let nex_data_dir = temp_dir.path().join("fih-data");
        std::fs::create_dir_all(&nex_data_dir).expect("create nex-data dir");
        let nex_bin = nex_server_bin();
        let nex_stderr_path = temp_dir.path().join("nex-server.stderr");
        let nex_stderr_file =
            std::fs::File::create(&nex_stderr_path).expect("create nex-server.stderr");
        let nex_child = Command::new(&nex_bin)
            .env("NEX_SOCKET_PATH", nex_server_socket.to_str().unwrap())
            .env("NEX_DATA_DIR", nex_data_dir.to_str().unwrap())
            .env("RUST_LOG", "nex-server=error")
            .stdout(Stdio::null())
            .stderr(nex_stderr_file)
            .spawn()
            .unwrap_or_else(|e| panic!("spawn nex-server failed: {e}"));

        // Wait for nex-server socket
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if nex_server_socket.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !nex_server_socket.exists() {
            let stderr_log = std::fs::read_to_string(&nex_stderr_path).unwrap_or_default();
            panic!("nex-server not ready in 5s. stderr:\n{}", stderr_log);
        }

        let nexd_stderr_path = temp_dir.path().join("nexd.stderr");
        let nexd_stderr_file =
            std::fs::File::create(&nexd_stderr_path).expect("create nexd.stderr");
        let child = Command::new(NEXD_BIN)
            .env("NEXD_SOCKET_PATH", socket_path.to_str().unwrap())
            .env("NEXD_NEX_SERVER_PATH", nex_server_bin())
            .env("NEX_SOCKET_PATH", nex_server_socket.to_str().unwrap())
            .env("NEX_DATA_DIR", nex_data_dir.to_str().unwrap())
            .env("RUST_LOG", "nexd=error")
            .stdout(Stdio::null())
            .stderr(nexd_stderr_file)
            .spawn()
            .unwrap_or_else(|e| panic!("spawn nexd ({NEXD_BIN}) failed: {e}"));

        let handle = Self {
            child: Some(child),
            nex_child: Some(nex_child),
            socket_path: socket_path.clone(),
            temp_dir,
        };
        if let Err(msg) = handle.wait_ready(5) {
            let nexd_stderr = std::fs::read_to_string(&nexd_stderr_path).unwrap_or_default();
            let nex_stderr = std::fs::read_to_string(&nex_stderr_path).unwrap_or_default();
            panic!(
                "{}.\nnex-server stderr:\n{}\nnexd stderr:\n{}",
                msg, nex_stderr, nexd_stderr
            );
        }
        handle
    }

    fn wait_ready(&self, timeout_secs: u64) -> Result<(), String> {
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);
        while start.elapsed() < timeout {
            if self.socket_path.exists() && UnixStream::connect(&self.socket_path).is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!(
            "nexd not ready in {timeout_secs}s at {}",
            self.socket_path.display()
        ))
    }

    /// Connect, send one request, read one line response.
    pub fn rpc(&self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let mut stream = UnixStream::connect(&self.socket_path).expect("connect");
        let req = serde_json::json!({"id":1,"method":method,"params":params});
        let mut buf = serde_json::to_string(&req).unwrap();
        buf.push('\n');
        stream.write_all(buf.as_bytes()).unwrap();
        stream.flush().unwrap();

        // Read exactly one line (nexd sends line-delimited JSON)
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        let n = reader.read_line(&mut line).unwrap_or_else(|e| {
            panic!("read_line failed for {method}: {e}");
        });
        if n == 0 {
            panic!(
                "nexd closed connection for {method} (EOF). Check nexd.stderr and nex-server.stderr in temp dir."
            );
        }
        serde_json::from_str(line.trim()).unwrap_or_else(|e| {
            panic!(
                "JSON-RPC response parse failed for {method}: {e}. raw line: {:?}",
                line.trim()
            );
        })
    }

    /// Assert RPC succeeded, return result.
    pub fn ok(&self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let resp = self.rpc(method, params);
        assert!(
            resp["error"].is_null(),
            "RPC {method} error: {:?}",
            resp["error"]
        );
        resp["result"].clone()
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(mut child) = self.nex_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
