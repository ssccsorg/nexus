// gateway/nex-cf — Consumption Orchestration Layer (CF Worker).
//
// This is a clean-slate Rust Cloudflare Worker that serves as the
// **consumption orchestration layer** for the nexus-sim project. It
// exposes the FIH (Facts-Intents-Hints) HTTP API backed by CF Workers
// KV (facts, intents, hints metadata), R2 (blob content), and Durable
// Objects (CAS-based intent claiming).
//
// Key constraint: this crate depends on `nexus-model` for types only
// (Fact, Intent, Hint, FihHash, Content, BoardState) and on the `worker`
// crate for the CF Workers runtime. It must NOT depend on the `nex` core
// crate, `petgraph`, or `interface-cypher`.

mod bindings;

use nexus_model::fih::{BoardState, Content, Fact, FihHash, Hint, Intent};
use serde::{Deserialize, Serialize};
use worker::*;

use bindings::WorkerEnv;

// ── Error type ───────────────────────────────────────────────────────────

/// Structured error returned by the storage layer and HTTP handlers.
#[derive(Debug)]
pub enum NexError {
    NotFound(String),
    Conflict(String),
    Forbidden(String),
    Internal(String),
}

impl std::fmt::Display for NexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(m) => write!(f, "not found: {m}"),
            Self::Conflict(m) => write!(f, "conflict: {m}"),
            Self::Forbidden(m) => write!(f, "forbidden: {m}"),
            Self::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl From<worker::Error> for NexError {
    fn from(e: worker::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<worker::KvError> for NexError {
    fn from(e: worker::KvError) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<NexError> for worker::Error {
    fn from(e: NexError) -> Self {
        worker::Error::RustError(e.to_string())
    }
}

// ── Request / Response types ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SubmitFactRequest {
    pub id: Option<String>,
    pub origin: String,
    pub content: serde_json::Value,
    pub creator: String,
}

#[derive(Serialize)]
pub struct SubmitFactResponse {
    pub id: String,
}

#[derive(Deserialize)]
pub struct SubmitIntentRequest {
    pub id: Option<String>,
    pub from_facts: Vec<String>,
    pub description: String,
    pub creator: String,
}

#[derive(Serialize)]
pub struct SubmitIntentResponse {
    pub id: String,
}

#[derive(Deserialize)]
pub struct ClaimRequest {
    pub agent: String,
}

// ── KV-backed storage keys ───────────────────────────────────────────────

fn fact_key(project_id: &str, id: &str) -> String {
    format!("{project_id}:fact:{id}")
}

fn intent_key(project_id: &str, id: &str) -> String {
    format!("{project_id}:intent:{id}")
}

fn hint_key(project_id: &str, id: &str) -> String {
    format!("{project_id}:hint:{id}")
}

fn state_index_key(project_id: &str, kind: &str) -> String {
    format!("{project_id}:index:{kind}")
}

// ── Storage helpers ──────────────────────────────────────────────────────

/// Read a JSON-serialized value from KV.
async fn kv_get<T: serde::de::DeserializeOwned>(
    kv: &KvStore,
    key: &str,
) -> Result<Option<T>, NexError> {
    let raw = kv.get(key).text().await?;
    match raw {
        None => Ok(None),
        Some(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| NexError::Internal(format!("deserialize {key}: {e}"))),
    }
}

/// Write a JSON-serialized value to KV.
async fn kv_put<T: serde::Serialize>(
    kv: &KvStore,
    key: &str,
    value: &T,
) -> Result<(), NexError> {
    let text = serde_json::to_string(value)
        .map_err(|e| NexError::Internal(format!("serialize {key}: {e}")))?;
    kv.put(key, text)?.execute().await?;
    Ok(())
}

/// Append an ID to a JSON array index in KV.
async fn kv_index_append(kv: &KvStore, key: &str, id: &str) -> Result<(), NexError> {
    let mut ids: Vec<String> = kv_get(kv, key).await?.unwrap_or_default();
    if !ids.contains(&id.to_string()) {
        ids.push(id.to_string());
        kv_put(kv, key, &ids).await?;
    }
    Ok(())
}

/// Read a full index and fetch all items of a given kind.
async fn fetch_all<T: serde::de::DeserializeOwned>(
    kv: &KvStore,
    project_id: &str,
    kind: &str,
) -> Result<Vec<T>, NexError> {
    let index_key = state_index_key(project_id, kind);
    let ids: Vec<String> = kv_get(kv, &index_key).await?.unwrap_or_default();
    let mut items = Vec::with_capacity(ids.len());
    for id in &ids {
        let item_key = match kind {
            "fact" => fact_key(project_id, id),
            "intent" => intent_key(project_id, id),
            "hint" => hint_key(project_id, id),
            _ => return Err(NexError::Internal(format!("unknown kind: {kind}"))),
        };
        if let Some(item) = kv_get::<T>(kv, &item_key).await? {
            items.push(item);
        }
    }
    Ok(items)
}

/// Build an error Response from a NexError.
fn err_response(e: NexError) -> worker::Result<Response> {
    let (status, label) = match &e {
        NexError::NotFound(_) => (404, "not_found"),
        NexError::Conflict(_) => (409, "conflict"),
        NexError::Forbidden(_) => (403, "forbidden"),
        NexError::Internal(_) => (500, "internal_error"),
    };
    let body = serde_json::json!({
        "error": label,
        "detail": e.to_string(),
    });
    Response::from_json(&body).map(|r| r.with_status(status))
}

// ── Router & entry point ─────────────────────────────────────────────────

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();
    router
        .get_async("/health", health_handler)
        .post_async("/api/v1/fih/facts", |req, ctx| async move {
            submit_fact_handler(req, ctx).await
        })
        .get_async("/api/v1/fih/state", |req, ctx| async move {
            read_state_handler(req, ctx).await
        })
        .post_async("/api/v1/fih/intents", |req, ctx| async move {
            submit_intent_handler(req, ctx).await
        })
        .post_async("/api/v1/fih/intents/:id/claim", |req, ctx| async move {
            claim_intent_handler(req, ctx).await
        })
        .run(req, env)
        .await
}

// ── Handlers ─────────────────────────────────────────────────────────────

/// GET /health
async fn health_handler(_req: Request, _ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&serde_json::json!({
        "status": "ok",
        "service": "nexus-gateway-nex-cf",
    }))
}

/// POST /api/v1/fih/facts
#[worker::send]
async fn submit_fact_handler(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let body: SubmitFactRequest = match req.json().await {
        Ok(b) => b,
        Err(e) => {
            return err_response(NexError::Internal(format!(
                "failed to parse request body: {e}"
            )));
        }
    };

    let project_id = "default";
    let id = body.id.unwrap_or_else(|| format!("fact_{}", uuid_v4()));

    let content = match &body.content {
        serde_json::Value::String(s) => Content {
            mime_type: "text/plain".into(),
            data: s.clone().into_bytes(),
        },
        other => Content {
            mime_type: "application/json".into(),
            data: serde_json::to_string(other)
                .unwrap_or_default()
                .into_bytes(),
        },
    };

    let fact = Fact {
        id: FihHash(id.clone()),
        origin: body.origin,
        content,
        creator: body.creator,
    };

    let wenv = WorkerEnv::new(&ctx.env);
    let kv = wenv.kv()?;
    let fk = fact_key(project_id, &id);
    kv_put(&kv, &fk, &fact)
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;
    kv_index_append(&kv, &state_index_key(project_id, "fact"), &id)
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;

    Ok(Response::from_json(&SubmitFactResponse { id })?
        .with_status(201))
}

/// GET /api/v1/fih/state
#[worker::send]
async fn read_state_handler(
    _req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let project_id = "default";
    let wenv = WorkerEnv::new(&ctx.env);
    let kv = wenv.kv()?;

    let facts = fetch_all::<Fact>(&kv, project_id, "fact")
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;
    let intents = fetch_all::<Intent>(&kv, project_id, "intent")
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;
    let hints = fetch_all::<Hint>(&kv, project_id, "hint")
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;

    let state = BoardState {
        facts,
        intents,
        hints,
    };
    Ok(Response::from_json(&state)?)
}

/// POST /api/v1/fih/intents
#[worker::send]
async fn submit_intent_handler(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let body: SubmitIntentRequest = match req.json().await {
        Ok(b) => b,
        Err(e) => {
            return err_response(NexError::Internal(format!(
                "failed to parse request body: {e}"
            )));
        }
    };

    if body.from_facts.is_empty() {
        return err_response(NexError::Forbidden(
            "intent must be grounded in at least one fact".into(),
        ));
    }

    let project_id = "default";
    let id = body.id.unwrap_or_else(|| format!("intent_{}", uuid_v4()));

    let intent = Intent {
        id: FihHash(id.clone()),
        from_facts: body.from_facts,
        description: body.description,
        creator: body.creator,
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    };

    let wenv = WorkerEnv::new(&ctx.env);
    let kv = wenv.kv()?;
    let ik = intent_key(project_id, &id);
    kv_put(&kv, &ik, &intent)
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;
    kv_index_append(&kv, &state_index_key(project_id, "intent"), &id)
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?;

    Ok(Response::from_json(&SubmitIntentResponse { id })?
        .with_status(201))
}

/// POST /api/v1/fih/intents/{id}/claim
#[worker::send]
async fn claim_intent_handler(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let id = ctx
        .param("id")
        .map(|s| s.to_owned())
        .unwrap_or_default();
    let body: ClaimRequest = match req.json().await {
        Ok(b) => b,
        Err(e) => {
            return err_response(NexError::Internal(format!(
                "failed to parse request body: {e}"
            )));
        }
    };

    let wenv = WorkerEnv::new(&ctx.env);

    // Use Durable Object for CAS-based claiming.
    let stub = wenv.intent_do_stub(&id)?;
    let mut claim_res = stub
        .fetch_with_str(&format!("https://do/claim/{}", body.agent))
        .await?;

    let status = claim_res.status_code();
    if status != 200 {
        let text = claim_res.text().await.unwrap_or_default();
        return err_response(NexError::Conflict(format!("claim rejected: {text}")));
    }

    // Update the intent record in KV to mark the worker assignment.
    let kv = wenv.kv()?;
    let project_id = "default";
    let ik = intent_key(project_id, &id);
    if let Some(mut intent) = kv_get::<Intent>(&kv, &ik)
        .await
        .map_err(|e| worker::Error::RustError(e.to_string()))?
    {
        intent.worker = Some(body.agent.clone());
        kv_put(&kv, &ik, &intent)
            .await
            .map_err(|e| worker::Error::RustError(e.to_string()))?;
    }

    Ok(Response::empty()?.with_status(200))
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Generate a unique-ish identifier using a timestamp-based approach.
///
/// In WASM/Workers context, this provides a simple unique identifier.
/// For production use, consider the `uuid` crate.
fn uuid_v4() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:020x}", nanos)
}
