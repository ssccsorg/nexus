// ── nex-server: standalone FIH blackboard server ─────────────────────────
//
// Serves the FIH blackboard over a Unix domain socket using JSON-RPC 2.0.
// No daemon, no agent management — pure blackboard.
//
// Usage:
//   nex-server [/path/to/socket] [/path/to/data]

use std::path::PathBuf;
use std::sync::Arc;

use nex::io::fs_io::FsIo;
use nex::storage::core::store::FihStorage;
use nex_client::{RpcRequest, RpcResponse};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tracing::{error, info};

mod handler;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nex-server=info")),
        )
        .init();

    let socket_path = std::env::var("NEX_SOCKET_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/nex-server.sock"));

    let data_dir = std::env::var("NEX_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data/fih"));

    // Remove stale socket file
    let _ = tokio::fs::remove_file(&socket_path).await;

    // Initialize FihStorage with FsIo
    let io = FsIo::new(&data_dir).unwrap_or_else(|e| {
        panic!("failed to create FsIo at {:?}: {e}", data_dir);
    });

    let storage = FihStorage::with_auto_flush(io, "nex-server");
    storage.rebuild_cache().await.unwrap_or_else(|e| {
        panic!("failed to rebuild cache: {e}");
    });

    let storage = Arc::new(storage);

    // Bind Unix socket
    let listener = UnixListener::bind(&socket_path).unwrap_or_else(|e| {
        panic!("failed to bind socket at {:?}: {e}", socket_path);
    });

    info!(?socket_path, "nex-server started");

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let storage = Arc::clone(&storage);
                tokio::spawn(async move {
                    handle_client(stream, storage).await;
                });
            }
            Err(e) => {
                error!(error = %e, "accept failed");
            }
        }
    }
}

async fn handle_client(stream: tokio::net::UnixStream, storage: Arc<FihStorage<FsIo>>) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => {
                let err = RpcResponse::invalid_request(serde_json::Value::Null);
                let _ = writer
                    .write_all(format!("{}\n", serde_json::to_string(&err).unwrap()).as_bytes())
                    .await;
                continue;
            }
        };

        let resp = handler::dispatch(req, &storage).await;
        let json = serde_json::to_string(&resp).unwrap_or_default();
        let _ = writer.write_all(format!("{json}\n").as_bytes()).await;
    }
}
