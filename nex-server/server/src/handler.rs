// ── Handler — JSON-RPC 2.0 method dispatch for FihStorage ────────────────

use std::sync::Arc;

use nex::io::fs_io::FsIo;
use nex::storage::core::store::FihStorage;
use nex_client::{RpcRequest, RpcResponse};
use nexus_model::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead, BlackboardError,
    Content, Fact, FihHash, Hint, Intent,
};
use serde::Deserialize;
use serde_json::{json, Value};

fn map_error(e: BlackboardError) -> (i64, String) {
    match e {
        BlackboardError::NotFound(m) => (-32001, format!("not found: {m}")),
        BlackboardError::Conflict(m) => (-32002, format!("conflict: {m}")),
        BlackboardError::Forbidden(m) => (-32003, format!("forbidden: {m}")),
        BlackboardError::Internal(m) => (-32000, format!("internal: {m}")),
    }
}

pub async fn dispatch(
    req: RpcRequest,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    let id = req.id;

    match req.method.as_str() {
        "write_fact" => handle_write_fact(id, &req.params, storage).await,
        "read_state" => handle_read_state(id, storage).await,
        "read_fact" => handle_read_fact(id, &req.params, storage).await,
        "read_intent" => handle_read_intent(id, &req.params, storage).await,
        "read_hint" => handle_read_hint(id, &req.params, storage).await,
        "write_intent" => handle_write_intent(id, &req.params, storage).await,
        "claim_intent" => handle_claim_intent(id, &req.params, storage).await,
        "heartbeat_intent" => handle_heartbeat_intent(id, &req.params, storage).await,
        "release_intent" => handle_release_intent(id, &req.params, storage).await,
        "conclude_intent" => handle_conclude_intent(id, &req.params, storage).await,
        "write_hint" => handle_write_hint(id, &req.params, storage).await,
        _ => RpcResponse::method_not_found(id, &req.method),
    }
}

// ── Fact handlers ────────────────────────────────────────────────────────

async fn handle_write_fact(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        origin: String,
        content: Value,
        creator: String,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    let fact = Fact {
        id: FihHash::from_hex(&format!("fact_{}", uuid::Uuid::new_v4())),
        origin: p.origin,
        content: match &p.content {
            Value::String(s) => Content {
                mime_type: "text/plain".into(),
                data: s.clone().into_bytes(),
            },
            other => Content {
                mime_type: "application/json".into(),
                data: serde_json::to_string(other).unwrap_or_default().into_bytes(),
            },
        },
        creator: p.creator,
    };

    match storage.submit_fact(&fact).await {
        Ok(hash) => RpcResponse::success(id, json!({"id": hash.to_string()})),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

async fn handle_read_state(
    id: Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    let state = storage.read_state().await;
    RpcResponse::success(id, serde_json::to_value(state).unwrap_or_default())
}

async fn handle_read_fact(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params { id: String }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    let state = storage.read_state().await;
    let target = FihHash::from_hex(&p.id).to_string();
    for fact in &state.facts {
        if fact.id.to_string() == target {
            return RpcResponse::success(id, serde_json::to_value(fact).unwrap_or_default());
        }
    }
    RpcResponse::error(id, -32001, format!("fact not found: {}", p.id))
}

async fn handle_read_intent(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params { id: String }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    let state = storage.read_state().await;
    let target = FihHash::from_hex(&p.id).to_string();
    for intent in &state.intents {
        if intent.id.to_string() == target {
            return RpcResponse::success(id, serde_json::to_value(intent).unwrap_or_default());
        }
    }
    RpcResponse::error(id, -32001, format!("intent not found: {}", p.id))
}

async fn handle_read_hint(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params { id: String }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    let state = storage.read_state().await;
    let target = FihHash::from_hex(&p.id).to_string();
    for hint in &state.hints {
        if hint.id.to_string() == target {
            return RpcResponse::success(id, serde_json::to_value(hint).unwrap_or_default());
        }
    }
    RpcResponse::error(id, -32001, format!("hint not found: {}", p.id))
}

// ── Intent handlers ──────────────────────────────────────────────────────

async fn handle_write_intent(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        from_facts: Vec<String>,
        description: String,
        creator: String,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    if p.from_facts.is_empty() {
        return RpcResponse::error(id, -32003, "intent must reference at least one fact");
    }

    let intent = Intent {
        id: FihHash::from_hex(&format!("intent_{}", uuid::Uuid::new_v4())),
        from_facts: p.from_facts.iter().map(|s| FihHash::from_hex(s)).collect(),
        description: p.description,
        creator: p.creator,
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };

    match storage.submit_intent(&intent).await {
        Ok(hash) => RpcResponse::success(id, json!({"id": hash.to_string()})),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

async fn handle_claim_intent(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        agent: String,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    match storage.claim_intent(&p.id, &p.agent).await {
        Ok(()) => RpcResponse::success(id, json!("ok")),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

async fn handle_heartbeat_intent(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        agent: String,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    match storage.heartbeat(&p.id, &p.agent).await {
        Ok(()) => RpcResponse::success(id, json!("ok")),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

async fn handle_release_intent(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        agent: String,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    match storage.release_intent(&p.id, &p.agent).await {
        Ok(()) => RpcResponse::success(id, json!("ok")),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

async fn handle_conclude_intent(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        result: Value,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    let result_str = match &p.result {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };

    match storage.conclude_intent(&p.id, &result_str).await {
        Ok(fact) => RpcResponse::success(id, json!({"fact": fact})),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

// ── Hint handler ─────────────────────────────────────────────────────────

async fn handle_write_hint(
    id: Value,
    params: &Value,
    storage: &Arc<FihStorage<FsIo>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        content: String,
        creator: String,
    }
    let p: Params = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => return RpcResponse::invalid_params(id, e.to_string()),
    };

    let hint = Hint {
        id: FihHash::from_hex(&p.id),
        content: p.content,
        creator: p.creator,
    };

    match storage.submit_hint(&hint).await {
        Ok(()) => RpcResponse::success(id, json!("ok")),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}
