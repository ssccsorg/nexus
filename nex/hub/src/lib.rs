// ── nex-hub: the connection layer ──────────────────────────────────────
//
// A client sends a request; the hub finds the node that answers the method and forwards it.
// The hub keeps no record, opens no volume, and decides nothing about where anything is
// stored. What it holds is the routing table and one queue per node.
//
// A node connects and sends `node.register` first, naming itself and the methods it answers.
// After that the connection carries forwarded requests and their responses. A node is
// answered one request at a time, so a slow node delays only its own callers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nex_protocol::{NodeRegistration, RpcRequest, RpcResponse, METHOD_REGISTER, NO_NODE};
use nex_router::Router;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, RwLock};

/// How long a client waits for the node to answer.
const NODE_TIMEOUT: Duration = Duration::from_secs(30);

type Job = (RpcRequest, oneshot::Sender<RpcResponse>);

/// The hub's state: who answers what, and the queue to each node.
#[derive(Default)]
pub struct Hub {
    router: RwLock<Router>,
    nodes: RwLock<HashMap<String, mpsc::Sender<Job>>>,
}

/// Serve clients and nodes on one Unix socket.
///
/// The first frame on a connection decides what it is: `node.register` makes it a node, and
/// anything else makes it a client whose request is already in hand.
pub async fn serve(socket: &Path, hub: Arc<Hub>) -> std::io::Result<()> {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    tracing::info!(?socket, "nex-hub listening");
    loop {
        let (stream, _) = listener.accept().await?;
        let hub = Arc::clone(&hub);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, hub).await {
                tracing::warn!(%error, "connection ended");
            }
        });
    }
}

/// A handle to one hub, for a host that serves it in its own runtime.
pub fn state() -> Arc<Hub> {
    Arc::new(Hub::default())
}

impl Hub {
    /// What the table holds, for a status call.
    pub async fn routes(&self) -> Vec<(String, String)> {
        let router = self.router.read().await;
        router
            .nodes()
            .into_iter()
            .flat_map(|node| {
                router
                    .methods_of(&node)
                    .into_iter()
                    .map(move |method| (method, node.clone()))
            })
            .collect()
    }
}

async fn handle_connection(stream: UnixStream, hub: Arc<Hub>) -> std::io::Result<()> {
    let (read, write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let Some(line) = read_frame(&mut reader).await? else {
        return Ok(());
    };
    let first: RpcRequest = match serde_json::from_str(&line) {
        Ok(request) => request,
        Err(error) => {
            let mut write = write;
            let response = RpcResponse::invalid_request(serde_json::Value::Null);
            let bytes = serde_json::to_vec(&response)?;
            write.write_all(&bytes).await?;
            write.write_all(b"\n").await?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                error.to_string(),
            ));
        }
    };

    if first.method == METHOD_REGISTER {
        serve_node(reader, write, first, hub).await
    } else {
        serve_client(reader, write, first, hub).await
    }
}

/// A node: register what it answers, then answer one forwarded request at a time.
async fn serve_node(
    mut reader: BufReader<ReadHalf<UnixStream>>,
    mut write: WriteHalf<UnixStream>,
    first: RpcRequest,
    hub: Arc<Hub>,
) -> std::io::Result<()> {
    let registration: NodeRegistration = match serde_json::from_value(first.params.clone()) {
        Ok(registration) => registration,
        Err(error) => {
            reply(&mut write, RpcResponse::invalid_params(first.id, error.to_string())).await?;
            return Ok(());
        }
    };

    let (job_tx, mut job_rx) = mpsc::channel::<Job>(16);
    {
        let mut router = hub.router.write().await;
        if let Err(error) = router.register(&registration.node, &registration.methods) {
            reply(&mut write, RpcResponse::error(first.id, NO_NODE, error)).await?;
            return Ok(());
        }
        hub.nodes
            .write()
            .await
            .insert(registration.node.clone(), job_tx);
    }
    reply(&mut write, RpcResponse::success(first.id, serde_json::json!({ "ok": true }))).await?;
    tracing::info!(node = %registration.node, methods = ?registration.methods, "node registered");

    // The reader feeds responses to the job loop. A node speaks only when spoken to, so the
    // order is the order of the jobs sent.
    let (response_tx, mut response_rx) = mpsc::channel::<RpcResponse>(16);
    let mut response_reader = tokio::spawn(async move {
        loop {
            match read_frame(&mut reader).await {
                Ok(Some(line)) => match serde_json::from_str::<RpcResponse>(&line) {
                    Ok(response) => {
                        if response_tx.send(response).await.is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "node sent a frame that is not a response");
                        return;
                    }
                },
                Ok(None) => return,
                Err(error) => {
                    tracing::warn!(%error, "node read failed");
                    return;
                }
            }
        }
    });

    loop {
        tokio::select! {
            job = job_rx.recv() => {
                let Some((request, answer)) = job else { break };
                let bytes = serde_json::to_vec(&request)?;
                write.write_all(&bytes).await?;
                write.write_all(b"\n").await?;
                match response_rx.recv().await {
                    Some(response) => {
                        let _ = answer.send(response);
                    }
                    None => {
                        let _ = answer.send(RpcResponse::error(
                            request.id,
                            NO_NODE,
                            format!("node {} left before answering", registration.node),
                        ));
                        break;
                    }
                }
            }
            _ = &mut response_reader => break,
        }
    }

    response_reader.abort();
    hub.nodes.write().await.remove(&registration.node);
    hub.router.write().await.deregister(&registration.node);
    tracing::info!(node = %registration.node, "node left");
    Ok(())
}

/// A client: answer each request from the node its method names.
async fn serve_client(
    mut reader: BufReader<ReadHalf<UnixStream>>,
    mut write: WriteHalf<UnixStream>,
    first: RpcRequest,
    hub: Arc<Hub>,
) -> std::io::Result<()> {
    if let Some(response) = route(&hub, first).await {
        reply(&mut write, response).await?;
    }
    while let Some(line) = read_frame(&mut reader).await? {
        let request: RpcRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(error) => {
                tracing::warn!(%error, "client sent a frame that is not a request");
                continue;
            }
        };
        if let Some(response) = route(&hub, request).await {
            reply(&mut write, response).await?;
        }
    }
    Ok(())
}

async fn route(hub: &Hub, request: RpcRequest) -> Option<RpcResponse> {
    let owner = {
        let router = hub.router.read().await;
        router.route(&request.method).map(str::to_string)
    };
    let Some(owner) = owner else {
        return Some(RpcResponse::error(
            request.id,
            NO_NODE,
            format!("no node answers {}", request.method),
        ));
    };
    let sender = {
        let nodes = hub.nodes.read().await;
        nodes.get(&owner).cloned()
    };
    let Some(sender) = sender else {
        return Some(RpcResponse::error(
            request.id,
            NO_NODE,
            format!("{owner} is not connected"),
        ));
    };
    let (answer_tx, answer_rx) = oneshot::channel();
    if sender.send((request.clone(), answer_tx)).await.is_err() {
        return Some(RpcResponse::error(
            request.id,
            NO_NODE,
            format!("{owner} is not taking requests"),
        ));
    }
    match tokio::time::timeout(NODE_TIMEOUT, answer_rx).await {
        Ok(Ok(response)) => Some(response),
        Ok(Err(_)) => Some(RpcResponse::error(
            request.id,
            NO_NODE,
            format!("{owner} dropped the request"),
        )),
        Err(_) => Some(RpcResponse::error(
            request.id,
            NO_NODE,
            format!("{owner} did not answer in time"),
        )),
    }
}

/// One newline-terminated frame, without its terminator.
async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut line = String::new();
    let read = reader.read_line(&mut line).await?;
    if read == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim_end().to_string()))
}

async fn reply(write: &mut WriteHalf<UnixStream>, response: RpcResponse) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(&response)?;
    write.write_all(&bytes).await?;
    write.write_all(b"\n").await
}
