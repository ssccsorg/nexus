// Integration tests for the gateway API.
//
// Starts the server on a random port, exercises every endpoint,
// and verifies the full FIH lifecycle works over HTTP.

use nexus_gateway_api::build_router;
use nexus_gateway_api::state::AppState;

fn test_state() -> AppState {
    AppState::in_memory()
}

/// Start the server on a random port and return the base URL.
async fn start_server(state: AppState) -> (String, tokio::task::JoinHandle<()>) {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (url, handle)
}

fn api(base: &str) -> String {
    format!("{base}/api/v1/fih")
}

#[tokio::test]
async fn test_submit_and_read_fact() {
    let (url, _handle) = start_server(test_state()).await;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/facts", api(&url)))
        .json(&serde_json::json!({
            "id": "f_test_001",
            "origin": "integration-test",
            "content": "Gateway API test fact",
            "creator": "test-agent"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "submit fact should succeed");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "f_test_001");

    let resp = client
        .get(format!("{}/state", api(&url)))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let state: serde_json::Value = resp.json().await.unwrap();
    let facts = state["facts"].as_array().unwrap();
    assert_eq!(facts.len(), 1, "should have 1 fact");
    assert_eq!(facts[0]["id"], "f_test_001");
    assert_eq!(facts[0]["content"]["mime_type"], "text/plain");
    let expected: Vec<u8> = b"Gateway API test fact".to_vec();
    assert_eq!(
        facts[0]["content"]["data"].as_array().unwrap(),
        &expected
            .into_iter()
            .map(serde_json::Value::from)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_intent_lifecycle_over_http() {
    let (url, _handle) = start_server(test_state()).await;
    let client = reqwest::Client::new();
    let base = api(&url);

    client
        .post(format!("{base}/facts"))
        .json(&serde_json::json!({
            "id": "f_lifecycle",
            "origin": "test",
            "content": "Ground truth",
            "creator": "agent-a"
        }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{base}/intents"))
        .json(&serde_json::json!({
            "id": "i_lifecycle",
            "from_facts": ["f_lifecycle"],
            "description": "Test lifecycle",
            "creator": "agent-b"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = client
        .post(format!("{base}/intents/i_lifecycle/claim"))
        .json(&serde_json::json!({ "agent": "worker-1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = client
        .post(format!("{base}/intents/i_lifecycle/heartbeat"))
        .json(&serde_json::json!({ "agent": "worker-1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = client
        .post(format!("{base}/intents/i_lifecycle/claim"))
        .json(&serde_json::json!({ "agent": "worker-2" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "double claim should conflict");

    let resp = client
        .post(format!("{base}/intents/i_lifecycle/conclude"))
        .json(&serde_json::json!({ "result": "Lifecycle verified over HTTP" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let conclude: serde_json::Value = resp.json().await.unwrap();
    let concluded_content = conclude["fact"]["content"]["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect::<Vec<_>>();
    assert!(String::from_utf8_lossy(&concluded_content).contains("Lifecycle verified"));

    let resp = client.get(format!("{base}/state")).send().await.unwrap();
    let state: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        state["facts"].as_array().unwrap().len(),
        2,
        "1 original + 1 concluded"
    );
    assert_eq!(
        state["intents"].as_array().unwrap().len(),
        1,
        "1 original intent"
    );
}

#[tokio::test]
async fn test_submit_hint() {
    let (url, _handle) = start_server(test_state()).await;
    let client = reqwest::Client::new();
    let base = api(&url);

    let resp = client
        .post(format!("{base}/hints"))
        .json(&serde_json::json!({
            "id": "h_test_001",
            "content": "Important observation",
            "creator": "observer"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = client.get(format!("{base}/state")).send().await.unwrap();
    let state: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(state["hints"].as_array().unwrap().len(), 1);
    assert_eq!(state["hints"][0]["content"], "Important observation");
}

#[tokio::test]
async fn test_submit_intent_without_facts_fails() {
    let (url, _handle) = start_server(test_state()).await;
    let client = reqwest::Client::new();
    let base = api(&url);

    let resp = client
        .post(format!("{base}/intents"))
        .json(&serde_json::json!({
            "id": "i_orphan",
            "from_facts": [],
            "description": "No grounding",
            "creator": "agent-x"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "ungrounded intent should be forbidden");
}

#[tokio::test]
async fn test_release_intent() {
    let (url, _handle) = start_server(test_state()).await;
    let client = reqwest::Client::new();
    let base = api(&url);

    client
        .post(format!("{base}/facts"))
        .json(&serde_json::json!({
            "id": "f_release",
            "origin": "test",
            "content": "Release test",
            "creator": "agent-a"
        }))
        .send()
        .await
        .unwrap();

    client
        .post(format!("{base}/intents"))
        .json(&serde_json::json!({
            "id": "i_release",
            "from_facts": ["f_release"],
            "description": "Release lifecycle",
            "creator": "agent-b"
        }))
        .send()
        .await
        .unwrap();

    client
        .post(format!("{base}/intents/i_release/claim"))
        .json(&serde_json::json!({ "agent": "worker-1" }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(format!("{base}/intents/i_release/release"))
        .json(&serde_json::json!({ "agent": "worker-1" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = client
        .post(format!("{base}/intents/i_release/claim"))
        .json(&serde_json::json!({ "agent": "worker-2" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "should be claimable after release");
}
