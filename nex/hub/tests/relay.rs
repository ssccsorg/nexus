// A client reaches the node that names its method, and a method no node owns is refused by
// the hub rather than forwarded.

use std::time::Duration;

use nex_protocol::NO_NODE;
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

fn socket_path() -> std::path::PathBuf {
    std::path::PathBuf::from(format!("/tmp/nex-hub-{}.sock", std::process::id()))
}

async fn write_line<W: AsyncWrite + Unpin>(write: &mut W, value: &Value) {
    let mut bytes = serde_json::to_vec(value).expect("serializes");
    bytes.push(b'\n');
    write.write_all(&bytes).await.expect("writes");
}

async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("reads");
    serde_json::from_str(line.trim_end()).expect("a frame")
}

#[tokio::test]
async fn a_client_reaches_the_node_that_answers_its_method() {
    let socket = socket_path();
    let hub = tokio::spawn({
        let socket = socket.clone();
        async move { nex_hub::serve(&socket, nex_hub::state()).await }
    });
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // The node registers what it answers, then answers.
    let node = tokio::spawn({
        let socket = socket.clone();
        async move {
            let stream = UnixStream::connect(&socket).await.expect("node connects");
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            write_line(
                &mut write,
                &json!({
                    "id": 0,
                    "method": nex_protocol::METHOD_REGISTER,
                    "params": { "node": "fake", "methods": ["ping"] }
                }),
            )
            .await;
            let _ack = read_line(&mut reader).await;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    break;
                }
                let request: Value = serde_json::from_str(line.trim_end()).expect("a request");
                let id = request["id"].clone();
                write_line(&mut write, &json!({ "id": id, "result": "pong" })).await;
            }
        }
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&socket).await.expect("client connects");
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    write_line(&mut write, &json!({ "id": 1, "method": "ping" })).await;
    let answered = read_line(&mut reader).await;
    assert_eq!(answered["result"], json!("pong"));
    assert_eq!(answered["id"], 1);

    write_line(&mut write, &json!({ "id": 2, "method": "nobody_answers" })).await;
    let refused = read_line(&mut reader).await;
    assert_eq!(refused["error"]["code"], json!(NO_NODE));

    node.abort();
    hub.abort();
    let _ = std::fs::remove_file(&socket);
}
