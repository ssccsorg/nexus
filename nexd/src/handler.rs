// ── Handler — JSON-RPC method dispatch ─────────────────────────────────
//
// Maps JSON-RPC method names to Blackboard trait calls. Each handler
// receives deserialized params, performs the operation, and returns
// a JSON Value for serialization.

use std::sync::{Arc, Mutex};

use nexus_model::{
    BlackboardError, Content, Fact, FactCapable, FihHash, Hint,
    HintCapable, Intent, IntentCapable, StorageRead,
};
use nexus_storage_composite::HybridBlackboard;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::manager::ProcessManager;

/// Deserialized JSON-RPC request body.
#[derive(Deserialize)]
pub struct RpcRequest {
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

/// Serialized JSON-RPC response.
#[derive(Serialize)]
pub struct RpcResponse {
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }

    pub fn invalid_request(id: Value) -> Self {
        Self::error(id, -32600, "Invalid Request")
    }

    pub fn method_not_found(id: Value, method: &str) -> Self {
        Self::error(id, -32601, format!("Method not found: {method}"))
    }
}

pub async fn dispatch(
    req: RpcRequest,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
    process_manager: &Arc<Mutex<ProcessManager>>,
) -> RpcResponse {
    let id = req.id;
    let params = req.params.unwrap_or(json!({}));

    match req.method.as_str() {
        "write_fact" => handle_write_fact(id, params, blackboard),
        "read_state" => handle_read_state(id, blackboard),
        "write_intent" => handle_write_intent(id, params, blackboard),
        "claim_intent" => handle_claim_intent(id, params, blackboard),
        "heartbeat_intent" => handle_heartbeat_intent(id, params, blackboard),
        "release_intent" => handle_release_intent(id, params, blackboard),
        "conclude_intent" => handle_conclude_intent(id, params, blackboard),
        "write_hint" => handle_write_hint(id, params, blackboard),
        "spawn_agent" => handle_spawn_agent(id, params, process_manager),
        "list_agents" => handle_list_agents(id, process_manager),
        "kill_agent" => handle_kill_agent(id, params, process_manager),
        _ => RpcResponse::method_not_found(id, &req.method),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn map_error(e: BlackboardError) -> (i64, String) {
    match e {
        BlackboardError::NotFound(m) => (-32001, format!("not found: {m}")),
        BlackboardError::Conflict(m) => (-32002, format!("conflict: {m}")),
        BlackboardError::Forbidden(m) => (-32003, format!("forbidden: {m}")),
        BlackboardError::Internal(m) => (-32000, format!("internal: {m}")),
    }
}

fn lock_bb<'a>(
    bb: &'a Arc<Mutex<HybridBlackboard>>,
) -> Result<std::sync::MutexGuard<'a, HybridBlackboard>, RpcResponse> {
    bb.lock().map_err(|e| {
        RpcResponse::error(Value::Null, -32000, format!("lock poisoned: {e}"))
    })
}

fn lock_pm<'a>(
    pm: &'a Arc<Mutex<ProcessManager>>,
) -> Result<std::sync::MutexGuard<'a, ProcessManager>, RpcResponse> {
    pm.lock().map_err(|e| {
        RpcResponse::error(Value::Null, -32000, format!("lock poisoned: {e}"))
    })
}

// ── Fact handlers ────────────────────────────────────────────────────────

fn handle_write_fact(
    id: Value,
    params: Value,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        origin: String,
        content: serde_json::Value,
        creator: String,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };

    let fact = Fact {
        id: FihHash::from_hex(&format!("fact_{}", uuid::Uuid::new_v4())),
        origin: p.origin,
        content: match &p.content {
            serde_json::Value::String(s) => Content {
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

    let bb = match lock_bb(blackboard) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match bb.submit_fact(&fact) {
        Ok(hash) => RpcResponse::success(id, json!({"id": hash.to_string()})),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

fn handle_read_state(
    id: Value,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
) -> RpcResponse {
    let bb = match lock_bb(blackboard) {
        Ok(g) => g,
        Err(e) => return e,
    };
    let state = bb.read_state();
    RpcResponse::success(id, serde_json::to_value(state).unwrap_or_default())
}

// ── Intent handlers ──────────────────────────────────────────────────────

fn handle_write_intent(
    id: Value,
    params: Value,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        from_facts: Vec<String>,
        description: String,
        creator: String,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
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

    let bb = match lock_bb(blackboard) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match bb.submit_intent(&intent) {
        Ok(hash) => RpcResponse::success(id, json!({"id": hash.to_string()})),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

fn handle_claim_intent(
    id: Value,
    params: Value,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        agent: String,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };
    let bb = match lock_bb(blackboard) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match bb.claim_intent(&p.id, &p.agent) {
        Ok(()) => RpcResponse::success(id, json!({})),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

fn handle_heartbeat_intent(
    id: Value,
    params: Value,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        agent: String,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };
    let bb = match lock_bb(blackboard) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match bb.heartbeat(&p.id, &p.agent) {
        Ok(()) => RpcResponse::success(id, json!({})),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

fn handle_release_intent(
    id: Value,
    params: Value,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        agent: String,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };
    let bb = match lock_bb(blackboard) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match bb.release_intent(&p.id, &p.agent) {
        Ok(()) => RpcResponse::success(id, json!({})),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

fn handle_conclude_intent(
    id: Value,
    params: Value,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        id: String,
        result: String,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };
    let bb = match lock_bb(blackboard) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match bb.conclude_intent(&p.id, &p.result) {
        Ok(fact) => RpcResponse::success(
            id,
            serde_json::to_value(fact).unwrap_or_default(),
        ),
        Err(e) => {
            let (code, msg) = map_error(e);
            RpcResponse::error(id, code, msg)
        }
    }
}

// ── Hint handlers ────────────────────────────────────────────────────────

fn handle_write_hint(
    id: Value,
    params: Value,
    blackboard: &Arc<Mutex<HybridBlackboard>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        content: String,
        creator: String,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };

    let hint = Hint {
        id: FihHash::from_hex(&format!("hint_{}", uuid::Uuid::new_v4())),
        content: p.content,
        creator: p.creator,
    };

    let bb = match lock_bb(blackboard) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match bb.submit_hint(&hint) {
        Ok(()) => RpcResponse::success(id, json!({})),
        Err(e) => RpcResponse::error(id, -32000, e.to_string()),
    }
}

// ── Process Manager handlers ─────────────────────────────────────────────

fn handle_spawn_agent(
    id: Value,
    params: Value,
    process_manager: &Arc<Mutex<ProcessManager>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };

    let mut pm = match lock_pm(process_manager) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match pm.spawn(&p.command, &p.args) {
        Ok(handle) => RpcResponse::success(id, json!({"pid": handle.pid})),
        Err(e) => RpcResponse::error(id, -32000, e),
    }
}

fn handle_list_agents(
    id: Value,
    process_manager: &Arc<Mutex<ProcessManager>>,
) -> RpcResponse {
    let pm = match lock_pm(process_manager) {
        Ok(g) => g,
        Err(e) => return e,
    };
    let agents: Vec<Value> = pm
        .list_agents()
        .into_iter()
        .map(|a| json!({"pid": a.pid, "command": a.command}))
        .collect();
    RpcResponse::success(id, json!({"agents": agents}))
}

fn handle_kill_agent(
    id: Value,
    params: Value,
    process_manager: &Arc<Mutex<ProcessManager>>,
) -> RpcResponse {
    #[derive(Deserialize)]
    struct Params {
        pid: u32,
    }
    let p: Params = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return RpcResponse::error(id, -32602, e.to_string()),
    };

    let mut pm = match lock_pm(process_manager) {
        Ok(g) => g,
        Err(e) => return e,
    };
    match pm.kill(p.pid) {
        Ok(()) => RpcResponse::success(id, json!({})),
        Err(e) => RpcResponse::error(id, -32000, e),
    }
}
