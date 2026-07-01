// ── Server — Unix domain socket IPC server ───────────────────────────
//
// Listens on a Unix domain socket, accepts connections, reads
// line-delimited JSON-RPC requests, dispatches to handler, and writes
// JSON-RPC responses.

use std::sync::{Arc, Mutex};

/// Maximum concurrent client connections.
const MAX_CONNECTIONS: usize = 128;

use nexus_storage_composite::HybridBlackboard;
use proc_daemon::ShutdownHandle;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{error, info, warn};

use crate::config::NexdConfig;
use crate::handler::{RpcRequest, RpcResponse, dispatch};
use crate::manager::ProcessManager;

/// Run the IPC server as a proc-daemon task.
///
/// Binds to the configured Unix socket path, accepts connections,
/// and processes JSON-RPC requests until shutdown is signalled.
pub async fn run(
    mut shutdown: ShutdownHandle,
    config: NexdConfig,
    blackboard: Arc<Mutex<HybridBlackboard>>,
    process_manager: Arc<Mutex<ProcessManager>>,
) -> proc_daemon::Result<()> {
    // Limit concurrent connections to prevent resource exhaustion.
    let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let socket_path = config.socket_path.as_path();

    // Remove stale socket file before binding
    if socket_path.exists() {
        tokio::fs::remove_file(socket_path)
            .await
            .map_err(|e| {
                warn!(path = %socket_path.display(), "could not remove stale socket: {e}");
                e
            })
            .ok();
    }

    let listener = UnixListener::bind(socket_path).map_err(|e| {
        error!(path = %socket_path.display(), "failed to bind socket: {e}");
        e
    })?;

    info!(
        path = %socket_path.display(),
        "IPC server listening"
    );

    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                info!("IPC server shutting down");
                // Clean up socket file
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
                            // Acquire semaphore permit — dropped when task completes
                            let _permit = sem.acquire().await;
                            if let Err(e) = handle_connection(stream, bb, pm).await {
                                warn!("connection handler error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        warn!("accept error: {e}");
                    }
                }
            }
        }
    }

    Ok(())
}

/// Handle a single client connection. Reads lines, dispatches, writes responses.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    blackboard: Arc<Mutex<HybridBlackboard>>,
    process_manager: Arc<Mutex<ProcessManager>>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let n = buf_reader.read_line(&mut line).await?;
        if n == 0 {
            // EOF — client disconnected
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Parse JSON-RPC request
        let req: RpcRequest = match serde_json::from_str(line) {
            Ok(req) => req,
            Err(e) => {
                warn!("invalid JSON-RPC request: {e}");
                let resp = RpcResponse::invalid_request(serde_json::Value::Null);
                let json = serde_json::to_string(&resp)?;
                writer.write_all(json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                continue;
            }
        };

        // Dispatch and respond
        let resp = dispatch(req, &blackboard, &process_manager);
        let json = serde_json::to_string(&resp)?;

        writer.write_all(json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}
