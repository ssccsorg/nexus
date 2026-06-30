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

/// Manages a nexd daemon instance for testing.
pub struct DaemonHandle {
    child: Option<Child>,
    pub socket_path: PathBuf,
    #[allow(dead_code)]
    pub temp_dir: tempfile::TempDir,
}

impl DaemonHandle {
    /// Start nexd with a unique socket in a temp dir.
    pub fn start() -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let socket_path = temp_dir.path().join("nexd.sock");

        let child = Command::new(NEXD_BIN)
            .env("NEXD_SOCKET_PATH", socket_path.to_str().unwrap())
            .env("RUST_LOG", "nexd=error")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn nexd ({NEXD_BIN}) failed: {e}"));

        let handle = Self {
            child: Some(child),
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
    }
}
