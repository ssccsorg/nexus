// ── Server — Unix domain socket IPC server ───────────────────────────
//
// Listens on a Unix domain socket, accepts connections, reads
// line-delimited JSON-RPC requests, dispatches to handler, and writes
// JSON-RPC responses.

use std::sync::{Arc, Mutex};

use nexus_storage_composite::HybridBlackboard;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{error, info, warn};

use crate::config::NexdConfig;
use crate::handler::{RpcRequest, RpcResponse, dispatch};
use crate::manager::ProcessManager;
use crate::rt::ShutdownHandle;

/// Maximum concurrent client connections.
const MAX_CONNECTIONS: usize = 128;

/// Run the IPC server as a daemon task.
pub async fn run(
    mut shutdown: ShutdownHandle,
    config: NexdConfig,
    blackboard: Arc<Mutex<HybridBlackboard>>,
    process_manager: Arc<Mutex<ProcessManager>>,
) {
    let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let socket_path = config.socket_path.as_path();

    // Remove stale socket file before binding
    if socket_path.exists() {
        if let Err(e) = tokio::fs::remove_file(socket_path).await {
            warn!(path = %socket_path.display(), "could not remove stale socket: {e}");
        }
    }

    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => l,
        Err(e) => {
            error!(path = %socket_path.display(), "failed to bind socket: {e}");
            return;
        }
    };

    info!(path = %socket_path.display(), "IPC server listening");

    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                info!("IPC server shutting down");
                let _ = tokio::fs::remove_file(socket_path).await;
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let bb = blackboard.clone();
                        let pm = process_manager.clone();
                        let sem = connection_semaphore.clone();
                        tokio::spawn(async move {
                            let _permit = sem.acquire().await;
                            handle_connection(stream, bb, pm).await;
                        });
                    }
                    Err(e) => {
                        warn!("accept error: {e}");
                    }
                }
            }
        }
    }
}

/// Handle a single client connection.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    blackboard: Arc<Mutex<HybridBlackboard>>,
    process_manager: Arc<Mutex<ProcessManager>>,
) {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let n = match buf_reader.read_line(&mut line).await {
            Ok(n) => n,
            Err(e) => {
                warn!("read error: {e}");
                break;
            }
        };
        if n == 0 {
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: RpcRequest = match serde_json::from_str(line) {
            Ok(req) => req,
            Err(e) => {
                warn!("invalid JSON-RPC request: {e}");
                let resp = RpcResponse::invalid_request(serde_json::Value::Null);
                if let Ok(json) = serde_json::to_string(&resp) {
                    let _ = writer.write_all(json.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                    let _ = writer.flush().await;
                }
                continue;
            }
        };

        let resp = dispatch(req, &blackboard, &process_manager);
        if let Ok(json) = serde_json::to_string(&resp) {
            let _ = writer.write_all(json.as_bytes()).await;
            let _ = writer.write_all(b"\n").await;
            let _ = writer.flush().await;
        }
    }
}
