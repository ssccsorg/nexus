// A reader reaches the node through the hub: the same address answered over a socket and
// over a volume. The connection therefore adds nothing to the answer, which is the whole
// claim of the hub. The node here is a stand-in that answers the key space, because what
// this guards is the reader's path through the connection.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

fn sample_volume() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo/volume")
}

fn socket_path() -> PathBuf {
    // The answer socket has to fit `sun_path`, so the name is short on purpose.
    PathBuf::from(format!("/tmp/nex-hub-{}.sock", std::process::id()))
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nexus-storage-ref"))
        .args(args)
        .output()
        .expect("the binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// The keys the fixture holds, and the bytes at each.
fn records() -> BTreeMap<String, Vec<u8>> {
    let mut records = BTreeMap::new();
    for entry in fs::read_dir(sample_volume().join("facts")).expect("the facts directory") {
        let entry = entry.expect("an entry");
        let key = format!(
            "facts/{}",
            entry.file_name().into_string().expect("a utf-8 name")
        );
        records.insert(key, fs::read(entry.path()).expect("the record's bytes"));
    }
    records
}

fn record_address() -> String {
    let records = records();
    let key = records.keys().next().expect("the fixture holds a record");
    key.strip_prefix("facts/f_")
        .and_then(|rest| rest.strip_suffix(".fact"))
        .expect("a record key")
        .to_string()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

async fn write_line<W: AsyncWriteExt + Unpin>(write: &mut W, value: &Value) {
    let mut bytes = serde_json::to_vec(value).expect("serializes");
    bytes.push(b'\n');
    write.write_all(&bytes).await.expect("writes");
}

/// A node that answers the key space, which is the two questions the reader asks.
async fn serve_node(socket: PathBuf, records: BTreeMap<String, Vec<u8>>) {
    let stream = UnixStream::connect(&socket).await.expect("the node connects");
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);

    write_line(
        &mut write,
        &json!({
            "id": 0,
            "method": nex_protocol::METHOD_REGISTER,
            "params": { "node": "stand-in", "methods": ["storage.keys", "storage.read"] }
        }),
    )
    .await;
    let mut ack = String::new();
    reader.read_line(&mut ack).await.expect("the registration is answered");

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }
        let request: Value = serde_json::from_str(line.trim_end()).expect("a request");
        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or_default();
        let answer = match method {
            "storage.keys" => {
                let prefix = request["params"]["prefix"].as_str().unwrap_or_default();
                let keys: Vec<&String> = records
                    .keys()
                    .filter(|key| key.starts_with(prefix))
                    .collect();
                json!({ "id": id, "result": { "keys": keys } })
            }
            "storage.read" => {
                let key = request["params"]["key"].as_str().unwrap_or_default();
                let data = match records.get(key) {
                    Some(bytes) => Value::String(hex(bytes)),
                    None => Value::Null,
                };
                json!({ "id": id, "result": { "data": data } })
            }
            _ => json!({ "id": id, "error": { "code": -32601, "message": method } }),
        };
        write_line(&mut write, &answer).await;
    }
}

#[test]
fn the_reader_reaches_the_node_through_the_hub() {
    let socket = socket_path();
    let _ = fs::remove_file(&socket);
    let volume = sample_volume();
    let volume_arg = volume.to_str().expect("a utf-8 path").to_string();
    let socket_arg = socket.to_str().expect("a utf-8 path").to_string();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");

    runtime.block_on(async {
        let hub_socket = socket.clone();
        tokio::spawn(async move {
            nex_hub::serve(&hub_socket, nex_hub::state()).await.ok();
        });
        for _ in 0..200 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        tokio::spawn(serve_node(socket.clone(), records()));
        // The node registers on its own connection; a request sent before it does would be
        // refused for having no owner rather than answered.
        for _ in 0..200 {
            let probe = run(&["--hub", &socket_arg, "keys", "--prefix", "facts/"]);
            if probe.status.success() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let over_hub = run(&["--hub", &socket_arg, "keys", "--prefix", "facts/"]);
        let over_volume = run(&["keys", "--volume", &volume_arg, "--prefix", "facts/"]);
        assert!(over_hub.status.success(), "{}", stderr(&over_hub));
        assert!(over_volume.status.success(), "{}", stderr(&over_volume));
        assert_eq!(
            stdout(&over_hub),
            stdout(&over_volume),
            "the connection changed the listing"
        );

        let address = record_address();
        let read_over_hub = run(&["--hub", &socket_arg, "read", "--address", &address]);
        let read_over_volume = run(&[
            "read",
            "--volume",
            &volume_arg,
            "--address",
            &address,
        ]);
        assert!(read_over_hub.status.success(), "{}", stderr(&read_over_hub));
        assert_eq!(
            stdout(&read_over_hub),
            stdout(&read_over_volume),
            "the connection changed the read"
        );

        let absent = run(&[
            "--hub",
            &socket_arg,
            "read",
            "--address",
            &"0".repeat(60),
        ]);
        assert_eq!(
            absent.status.code(),
            Some(2),
            "an absent address is missing over the hub as well"
        );
        assert!(stderr(&absent).contains("absent:"));
    });

    let _ = fs::remove_file(&socket);
}
