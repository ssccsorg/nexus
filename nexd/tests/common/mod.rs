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
fn nex_server_bin() -> std::path::PathBuf {
    let nexd_path = std::path::Path::new(NEXD_BIN);
    // nexd and nex-server are in the same target/{profile}/ directory
    if let Some(parent) = nexd_path.parent() {
        let candidate = parent.join("nex-server");
        if candidate.exists() {
            return candidate;
        }
    }
    // Fallback: workspace root
    let candidate = std::path::PathBuf::from("./target/debug/nex-server");
    if candidate.exists() {
        return candidate;
    }
    panic!(
        "nex-server binary not found. Run 'cargo build -p nex-server' first. \
         Tried: {} and ./target/debug/nex-server",
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

        // Start nex-server first
        let nex_bin = nex_server_bin();
        let nex_child = Command::new(&nex_bin)
            .env("NEX_SOCKET_PATH", nex_server_socket.to_str().unwrap())
            .env("RUST_LOG", "nex-server=error")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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
            panic!("nex-server not ready in 5s");
        }

        let child = Command::new(NEXD_BIN)
            .env("NEXD_SOCKET_PATH", socket_path.to_str().unwrap())
            .env("NEXD_NEX_SERVER_PATH", nex_server_bin())
            .env("NEX_SOCKET_PATH", nex_server_socket.to_str().unwrap())
            .env("RUST_LOG", "nexd=error")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn nexd ({NEXD_BIN}) failed: {e}"));

        let handle = Self {
            child: Some(child),
            nex_child: Some(nex_child),
            socket_path,
            temp_dir,
        };
        handle.wait_ready(5);
        handle
    }

    fn wait_ready(&self, timeout_secs: u64) {
        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);
        while start.elapsed() < timeout {
            if self.socket_path.exists() && UnixStream::connect(&self.socket_path).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "nexd not ready in {timeout_secs}s at {}",
            self.socket_path.display()
        );
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
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).expect("JSON-RPC response")
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
