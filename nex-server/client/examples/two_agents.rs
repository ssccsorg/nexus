//! Two agents sharing a blackboard through the nex-client SDK.
//!
//! Run against a running nex-server (spawned by nexd):
//!
//!   cargo run -p nex-client --example two_agents
//!
//! The socket is read from `NEX_SOCKET_PATH` and defaults to
//! `/tmp/nex-server.sock`.

use nex_client::NexClient;
use serde_json::Value;

fn socket_path() -> String {
    std::env::var("NEX_SOCKET_PATH").unwrap_or_else(|_| "/tmp/nex-server.sock".into())
}

/// Origins of every fact currently on the board.
fn origins(state: &Value) -> Vec<String> {
    state["facts"]
        .as_array()
        .map(|facts| {
            facts
                .iter()
                .filter_map(|f| f["origin"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket = socket_path();

    // Agent A writes a fact through its own connection.
    let mut alice = NexClient::connect(&socket).await?;
    let alice_fact = alice
        .write_fact("alice", "observation by alice", "alice")
        .await?;
    println!("alice wrote {alice_fact}");

    // Agent B connects independently and reads the shared state.
    let mut bob = NexClient::connect(&socket).await?;
    let bob_fact = bob.write_fact("bob", "observation by bob", "bob").await?;
    println!("bob wrote {bob_fact}");

    let state = bob.read_state().await?;
    let seen = origins(&state);
    assert!(
        seen.iter().any(|o| o == "alice"),
        "bob must see alice's fact on the shared blackboard"
    );
    assert!(seen.iter().any(|o| o == "bob"), "bob must see his own fact");
    println!("shared blackboard origins: {seen:?}");

    Ok(())
}
