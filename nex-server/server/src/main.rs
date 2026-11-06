// ── nex-server: standalone FIH blackboard server ─────────────────────────
//
// Serves the FIH blackboard over a Unix domain socket using JSON-RPC 2.0.
// No daemon, no agent management — pure blackboard.
//
// Usage:
//   NEX_SOCKET_PATH=/path/to/socket NEX_DATA_DIR=/path/to/data nex-server

use std::path::PathBuf;
use std::sync::Arc;

use nex::io::fs_io::FsIo;
use nexus_storage_ref::RefFihStorage;
use nex_client::{RpcRequest, RpcResponse};
use nex_protocol::{METHOD_REGISTER, NodeRegistration};
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

    // Initialize RefFihStorage with FsIo
    let io = FsIo::new(&data_dir).unwrap_or_else(|e| {
        panic!("failed to create FsIo at {:?}: {e}", data_dir);
    });

    let storage = RefFihStorage::with_auto_flush(io, "nex-server");
    storage.rebuild_cache().await.unwrap_or_else(|e| {
        panic!("failed to rebuild cache: {e}");
    });

    let storage = Arc::new(storage);

    // A hub in front of this node means the node connects out and answers forwarded
    // requests. Without one, it serves its own socket as before.
    if let Ok(hub) = std::env::var("NEX_HUB_SOCKET") {
        let name = std::env::var("NEX_NODE_NAME").unwrap_or_else(|_| "nex-server".into());
        serve_via_hub(&hub, &name, storage)
            .await
            .unwrap_or_else(|e| panic!("failed to serve through the hub at {hub}: {e}"));
        return;
    }

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

/// Register with the hub and answer what it forwards.
async fn serve_via_hub(
    hub: &str,
    name: &str,
    storage: Arc<RefFihStorage<FsIo>>,
) -> std::io::Result<()> {
    let stream = tokio::net::UnixStream::connect(hub).await?;
    let (read, write) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(read);
    let mut writer = write;

    let registration = RpcRequest {
        id: serde_json::json!(0),
        method: METHOD_REGISTER.into(),
        params: serde_json::to_value(NodeRegistration {
            node: name.into(),
            methods: handler::METHODS.iter().map(|m| m.to_string()).collect(),
        })
        .expect("the registration serializes"),
    };
    write_frame(&mut writer, &serde_json::to_vec(&registration)?).await?;

    let mut ack = String::new();
    if tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut ack).await? == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "the hub closed before answering the registration",
        ));
    }
    info!(hub, node = name, "registered with the hub");

    loop {
        let mut line = String::new();
        if tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line).await? == 0 {
            return Ok(());
        }
        let Ok(request) = serde_json::from_str::<RpcRequest>(line.trim_end()) else {
            continue;
        };
        let response = handler::dispatch(request, &storage).await;
        write_frame(&mut writer, &serde_json::to_vec(&response)?).await?;
    }
}

async fn write_frame(writer: &mut tokio::io::WriteHalf<tokio::net::UnixStream>, bytes: &[u8]) -> std::io::Result<()> {
    tokio::io::AsyncWriteExt::write_all(writer, bytes).await?;
    tokio::io::AsyncWriteExt::write_all(writer, b"\n").await
}

async fn handle_client(stream: tokio::net::UnixStream, storage: Arc<RefFihStorage<FsIo>>) {
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
