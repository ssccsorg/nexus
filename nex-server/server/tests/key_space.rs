// The node answers the key space the volume answers: which keys a prefix holds, the bytes
// at one key, and nothing at a key the volume does not hold. A reader that reaches the node
// through the hub therefore reads what a reader that opens the volume reads.

use std::path::PathBuf;
use std::sync::Arc;

use nex::io::fs_io::FsIo;
use nex_client::RpcRequest;
use nex_io::FileIo;
use nex_server::handler::{dispatch, METHODS};
use nexus_storage_ref::RefFihStorage;
use serde_json::{json, Value};

fn volume() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../storage/ref/demo/volume")
}

fn node() -> Arc<RefFihStorage<FsIo>> {
    let io = FsIo::new(volume()).expect("the demo volume opens");
    Arc::new(RefFihStorage::new(io, "nex-server-test"))
}

async fn ask(node: &Arc<RefFihStorage<FsIo>>, method: &str, params: Value) -> Value {
    let response = dispatch(
        RpcRequest {
            id: json!(1),
            method: method.into(),
            params,
        },
        node,
    )
    .await;
    response
        .result
        .unwrap_or_else(|| panic!("{method} answered with an error"))
}

fn decode(frame: &str) -> Vec<u8> {
    (0..frame.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&frame[at..at + 2], 16).expect("hex"))
        .collect()
}

#[tokio::test]
async fn the_key_space_is_what_the_volume_holds() {
    let node = node();
    let io = FsIo::new(volume()).expect("the demo volume opens");

    let mut direct = io.list("facts/").await.expect("the volume lists");
    direct.sort();
    assert!(!direct.is_empty(), "the fixture holds a record");

    let answered = ask(&node, "storage.keys", json!({ "prefix": "facts/" })).await;
    let mut listed: Vec<String> =
        serde_json::from_value(answered["keys"].clone()).expect("a list of keys");
    listed.sort();
    assert_eq!(listed, direct);
}

#[tokio::test]
async fn one_key_reads_the_bytes_the_volume_holds_at_it() {
    let node = node();
    let io = FsIo::new(volume()).expect("the demo volume opens");

    let keys = io.list("facts/").await.expect("the volume lists");
    let key = keys.into_iter().next().expect("the fixture holds a record");
    let direct = io
        .read(&key)
        .await
        .expect("the volume reads")
        .expect("the key is present");

    let answered = ask(&node, "storage.read", json!({ "key": key })).await;
    let frame = answered["data"].as_str().expect("a hex frame");
    assert_eq!(
        decode(frame),
        direct,
        "the node answered the volume's bytes"
    );
}

#[tokio::test]
async fn a_key_the_volume_does_not_hold_reads_as_nothing() {
    let node = node();
    let answered = ask(
        &node,
        "storage.read",
        json!({ "key": "facts/f_000000000000000000000000000000000000000000000000000000000000.fact" }),
    )
    .await;
    assert!(
        answered["data"].is_null(),
        "an absent key is a null answer"
    );
}

#[tokio::test]
async fn a_key_space_request_without_its_parameter_is_refused() {
    let node = node();
    let response = dispatch(
        RpcRequest {
            id: json!(1),
            method: "storage.read".into(),
            params: json!({ "prefix": "facts/" }),
        },
        &node,
    )
    .await;
    assert!(
        response.result.is_none(),
        "a request without a key is refused"
    );
}

#[test]
fn the_node_registers_the_key_space_methods() {
    assert!(
        METHODS.contains(&"storage.keys"),
        "the node owns the listing"
    );
    assert!(
        METHODS.contains(&"storage.read"),
        "the node owns the read"
    );
}
