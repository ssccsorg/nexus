// ── Server — Unix domain socket IPC server ───────────────────────────
//
// Listens on a Unix domain socket, accepts connections, reads
// line-delimited JSON-RPC requests, dispatches to handler, and writes
// JSON-RPC responses. All FIH operations go through NexClient (IPC to
// nex-server).

use std::sync::{Arc, Mutex};

/// Maximum concurrent client connections.
const MAX_CONNECTIONS: usize = 128;

use crate::daemon::ShutdownHandle;
use anyhow;
use nex_client::{NexClient, RpcRequest, RpcResponse};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{error, info, warn};

use crate::config::NexdConfig;
use crate::manager::ProcessManager;

/// Run the IPC server as a proc-daemon task.
pub async fn run(
    mut shutdown: ShutdownHandle,
    config: NexdConfig,
    process_manager: Arc<Mutex<ProcessManager>>,
) -> crate::daemon::Result<()> {
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
                        let pm = process_manager.clone();
                        let sem = connection_semaphore.clone();
                        let nex_socket = config.nex_server_socket.clone();
                        tokio::spawn(async move {
                            let _permit = sem.acquire().await;
                            if let Err(e) = handle_connection(stream, pm, &nex_socket).await {
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

/// Handle a single client connection.
async fn handle_connection(
    stream: tokio::net::UnixStream,
    process_manager: Arc<Mutex<ProcessManager>>,
    nex_socket: &str,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    // Connect NexClient per-connection (or share via Arc)
    let mut client = NexClient::connect(nex_socket)
        .await
        .map_err(|e| anyhow::anyhow!("nex-client connect: {e}"))?;

    loop {
        line.clear();
        let n = buf_reader.read_line(&mut line).await?;
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
                let json = serde_json::to_string(&resp)?;
                writer.write_all(json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
                continue;
            }
        };

        let resp = dispatch(req, &mut client, &process_manager).await;
        let json = serde_json::to_string(&resp)?;

        writer.write_all(json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}

/// Dispatch a JSON-RPC request. FIH methods go through NexClient;
/// agent management methods use ProcessManager directly.
async fn dispatch(
    req: RpcRequest,
    client: &mut NexClient,
    pm: &Arc<Mutex<ProcessManager>>,
) -> RpcResponse {
    let id = req.id;

    match req.method.as_str() {
        // ── FIH methods → NexClient IPC ─────────────────────────────
        "write_fact" | "read_state" | "read_fact" | "read_intent" | "read_hint"
        | "write_intent" | "claim_intent" | "heartbeat_intent" | "release_intent"
        | "conclude_intent" | "write_hint" => {
            let resp = client.call_raw(&req.method, req.params).await;
            RpcResponse {
                id,
                result: resp.result,
                error: resp.error,
            }
        }

        // ── Agent management methods → ProcessManager ──────────────
        "spawn_agent" => handle_spawn_agent(id, &req.params, pm),
        "list_agents" => handle_list_agents(id, pm),
        "kill_agent" => handle_kill_agent(id, &req.params, pm),
        _ => RpcResponse::method_not_found(id, &req.method),
    }
}

// ── Agent management handlers ────────────────────────────────────────────

use serde::Deserialize;
use serde_json::Value;

fn handle_spawn_agent(id: Value, params: &Value, pm: &Arc<Mutex<ProcessManager>>) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        command: String,
        args: Option<Vec<String>>,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };

    let mut guard = match pm.lock() {
        Ok(g) => g,
        Err(e) => return RpcResponse::error(id, -32000, format!("lock poisoned: {e}")),
    };
    match guard.spawn(&p.command, &p.args.unwrap_or_default()) {
        Ok(handle) => RpcResponse::success(id, serde_json::json!({"pid": handle.pid})),
        Err(e) => RpcResponse::error(id, -32000, e),
    }
}

fn handle_list_agents(id: Value, pm: &Arc<Mutex<ProcessManager>>) -> RpcResponse {
    let guard = match pm.lock() {
        Ok(g) => g,
        Err(e) => return RpcResponse::error(id, -32000, format!("lock poisoned: {e}")),
    };
    let agents = guard.list_agents();
    RpcResponse::success(id, serde_json::json!({ "agents": agents }))
}

fn handle_kill_agent(id: Value, params: &Value, pm: &Arc<Mutex<ProcessManager>>) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        pid: u32,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };

    let mut guard = match pm.lock() {
        Ok(g) => g,
        Err(e) => return RpcResponse::error(id, -32000, format!("lock poisoned: {e}")),
    };
    match guard.kill(p.pid) {
        Ok(()) => RpcResponse::success(id, serde_json::json!("ok")),
        Err(e) => RpcResponse::error(id, -32000, e),
    }
}
