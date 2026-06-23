// Wasmer Edge SSCCS docs server.
//
// FIH Blackboard + document ingestion + semantic search, deployed on Wasmer Edge.
// Uses `WasmerIo` (std::fs via WASIX) as the IO backend instead of Cloudflare R2.
// All CF Workers-specific code (R2, DO, Queue, Vectorize) is removed.
//
// Architecture:
//   FihStorage<BatchIo<WasmerIo>>  ←  in-memory + filesystem persistence
//   InMemoryBm25                   ←  semantic search (BM25)
//   axum HTTP server               ←  request routing
//
// Environment variables:
//   DATA_DIR  — FIH data directory (default: /data/fih)
//   DOCS_DIR  — .llms.md document root (default: /data/docs)
//   PORT      — HTTP listen port (default: 3000)
//   PROJECT   — FIH project ID (default: wasmer-ssccsdocs)
//
// Deployment:
//   cargo build --release --target wasm32-wasix
//   wasmer deploy
//
// Design note: FihStorage uses RefCell internally, making it !Send and !Sync.
// We use Arc<std::sync::Mutex<FihStorage>> as axum state. All async FihStorage
// calls are wrapped via futures-executor::block_on inside the Mutex lock.
// This is acceptable because the filesystem IO (WasmerIo) is sub-millisecond,
// so blocking the tokio thread for each call has negligible impact.
// The tokio runtime runs on a single thread (current_thread), so there is
// no thread-pool starvation.

mod batch_io;
mod bm25;
mod wasmer_io;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use futures_executor::block_on;
use nex::io::AsyncFileIo;
use nex::storage::core::FihStorage;
use nex::storage::semantic::Query as SemanticQuery;
use nexus_model::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead, Content, Fact, FihHash,
    Hint, Intent,
};
use serde::Deserialize;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use crate::batch_io::BatchIo;
use crate::bm25::InMemoryBm25;
use crate::wasmer_io::WasmerIo;

const DEFAULT_DATA_DIR: &str = "/data/fih";

// ── Text query ──────────────────────────────────────────────────────

struct TextQuery {
    text: String,
}

impl SemanticQuery for TextQuery {
    fn features(&self) -> Option<Vec<f32>> {
        None
    }
    fn text(&self) -> Option<String> {
        Some(self.text.clone())
    }
}

// ── Storage wrapper (block_on inside Mutex) ─────────────────────────

/// Thread-safe storage wrapper. Calls async FihStorage methods via block_on
/// inside a Mutex lock.
///
/// SAFETY: FihStorage uses RefCell internally, making it !Send. We wrap it
/// in a Mutex and always access it through Arc<Mutex<Storage>>. The inner
/// RefCell is never accessed concurrently because the Mutex provides exclusive
/// access. The block_on calls are safe because filesystem IO is instant and
/// the tokio runtime never parks while holding the Mutex lock.
struct Storage {
    inner: FihStorage<BatchIo<WasmerIo>>,
}

unsafe impl Send for Storage {}
unsafe impl Sync for Storage {}

impl Storage {
    fn new(inner: FihStorage<BatchIo<WasmerIo>>) -> Self {
        Self { inner }
    }
}

// ── Build storage ────────────────────────────────────────────────────

fn build_storage(data_dir: &str, project_id: &str) -> Storage {
    let fs_io = WasmerIo::new(data_dir).expect("create WasmerIo");
    let io = BatchIo::new(fs_io);
    let s = FihStorage::new(io, project_id);
    s.register_semantic_store(Box::new(InMemoryBm25::new()));
    Storage::new(s)
}

// ── Snapshot ─────────────────────────────────────────────────────────

fn write_snapshot(s: &FihStorage<BatchIo<WasmerIo>>) -> Result<(), String> {
    use nex::storage::core::record::{FactRecord, IntentRecord};
    use nex::storage::core::ChainEntry;
    let facts: Vec<FactRecord> = s.fact_store.values();
    let intents: Vec<IntentRecord> = s.intent_store.values();
    let entry = ChainEntry {
        prev_cursor: 0,
        records_flushed: facts.len() as u64,
        facts,
        intents,
    };
    let bytes = postcard::to_allocvec(&entry).map_err(|e| format!("snapshot serialize: {e}"))?;
    // Write through IO directly (bypasses BatchIo pending buffer).
    // Use block_on for the inner WasmerIo write.
    block_on(s.io.write("_snapshot/facts.bin", &bytes))?;
    // Flush BatchIo eagerly so the snapshot is immediately durable.
    block_on(s.io.flush())
}

fn restore_from_snapshot(s: &FihStorage<BatchIo<WasmerIo>>) -> Result<bool, String> {
    if !s.fact_store.is_empty() {
        return Ok(false);
    }
    let Some(bytes) = block_on(s.io.read("_snapshot/facts.bin"))? else {
        return Ok(false);
    };
    let entry: nex::storage::core::ChainEntry =
        postcard::from_bytes(&bytes).map_err(|e| format!("snapshot deserialize: {e}"))?;
    s.fact_store
        .replace_from(entry.facts.into_iter().map(|r| (r.id.clone(), r)).collect());
    s.intent_store
        .replace_from(entry.intents.into_iter().map(|r| (r.id.clone(), r)).collect());
    s.rebuild_coord();
    Ok(true)
}

// ── Document ingestion ───────────────────────────────────────────────

fn ingest_document(s: &FihStorage<BatchIo<WasmerIo>>, text: &str, origin: &str) -> Result<String, String> {
    let paragraphs: Vec<&str> = text
        .split('\n')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if paragraphs.is_empty() {
        return Err("empty document".into());
    }
    let mut last_id = String::new();
    for (i, para) in paragraphs.iter().enumerate() {
        let para_id = format!("f_{}_{}", sanitize_id(origin), i);
        let fact = Fact {
            id: FihHash::from_hex(&para_id),
            origin: format!("document:{}", origin),
            content: Content {
                mime_type: "text/plain".into(),
                data: para.as_bytes().to_vec(),
            },
            creator: "ingestion-agent".into(),
        };
        block_on(AsyncFactCapable::submit_fact(s, &fact))
            .map_err(|e| format!("submit para {i}: {e:?}"))?;
        last_id = para_id;
    }
    block_on(s.flush_pending()).map_err(|e| format!("flush: {e}"))?;
    if let Err(e) = write_snapshot(s) {
        tracing::warn!("snapshot write failed: {e}");
    }
    Ok(last_id)
}

fn ingest_all_from_io<D: AsyncFileIo>(
    s: &FihStorage<BatchIo<WasmerIo>>,
    docs: &D,
    prefix: &str,
    max: usize,
) -> (usize, Vec<String>) {
    let mut total = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let keys = match block_on(docs.list(prefix)) {
        Ok(keys) => keys,
        Err(e) => {
            errors.push(format!("list '{prefix}': {e}"));
            return (total, errors);
        }
    };
    for key in &keys {
        if total >= max {
            break;
        }
        if !key.ends_with(".llms.md") {
            continue;
        }
        let data = match block_on(docs.read(key)) {
            Ok(Some(d)) => d,
            Ok(None) => {
                errors.push(format!("{key}: empty"));
                continue;
            }
            Err(e) => {
                errors.push(format!("{key}: {e}"));
                continue;
            }
        };
        let text = match String::from_utf8(data) {
            Ok(t) => t,
            Err(_) => {
                errors.push(format!("{key}: not UTF-8"));
                continue;
            }
        };
        let origin = key
            .trim_end_matches(".llms.md")
            .trim_start_matches("_llms/")
            .to_string();
        match ingest_document(s, &text, &origin) {
            Ok(_) => total += 1,
            Err(e) => errors.push(format!("{key}: {e}")),
        }
    }
    (total, errors)
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── Request types ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct IngestParams {
    text: String,
    origin: Option<String>,
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    top_k: Option<usize>,
}

#[derive(Deserialize)]
struct IngestAllParams {
    prefix: Option<String>,
    max: Option<usize>,
}

#[derive(Deserialize)]
struct FactParams {
    id: Option<String>,
    origin: String,
    content: String,
    creator: String,
}

#[derive(Deserialize)]
struct IntentParams {
    id: Option<String>,
    from: Option<String>,
    desc: String,
    creator: String,
}

#[derive(Deserialize)]
struct ClaimParams {
    agent: String,
}

#[derive(Deserialize)]
struct ConcludeParams {
    result: String,
}

#[derive(Deserialize)]
struct HintParams {
    id: Option<String>,
    content: String,
    creator: String,
}

#[derive(serde::Serialize)]
struct ApiError {
    error: String,
    detail: String,
}

fn err_response(code: StatusCode, error: &str, detail: String) -> (StatusCode, Json<ApiError>) {
    (code, Json(ApiError { error: error.into(), detail }))
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ── Route handlers ───────────────────────────────────────────────────

type AppState = Arc<Mutex<Storage>>;

async fn handle_root() -> &'static str {
    "nexus-wasmer-ssccsdocs"
}

async fn handle_version() -> &'static str {
    "1"
}

fn storage_lock(state: &AppState) -> impl std::ops::Deref<Target = Storage> + '_ {
    state.lock().unwrap()
}

async fn handle_debug_stores(State(state): State<AppState>) -> Json<serde_json::Value> {
    let s = storage_lock(&state);
    Json(serde_json::json!({
        "stores": s.inner.semantic_stores().len(),
        "fact_store": s.inner.fact_store.len(),
        "service": "nexus-wasmer-ssccsdocs"
    }))
}

async fn handle_ingest(
    State(state): State<AppState>,
    Json(params): Json<IngestParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let origin = params.origin.unwrap_or_else(|| "ingest".into());
    let result = {
        let s = storage_lock(&state);
        ingest_document(&s.inner, &params.text, &origin)
    };
    match result {
        Ok(id) => Ok(Json(serde_json::json!({"status": "ingested", "id": id}))),
        Err(e) => Err(err_response(StatusCode::INTERNAL_SERVER_ERROR, "ingest_error", e)),
    }
}

async fn handle_search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let query = TextQuery { text: params.q.clone() };
    let top_k = params.top_k.unwrap_or(10);
    let results = {
        let s = storage_lock(&state);
        block_on(s.inner.semantic_search(&query, top_k))
    };
    match results {
        Ok(results) => {
            let items: Vec<serde_json::Value> = {
                let s = storage_lock(&state);
                results.iter().map(|(idx, score)| {
                    serde_json::json!({
                        "index": idx,
                        "score": score,
                        "id": s.inner.resolve_semantic_idx(*idx)
                    })
                }).collect()
            };
            Ok(Json(serde_json::json!({"results": items})))
        }
        Err(e) => Err(err_response(StatusCode::INTERNAL_SERVER_ERROR, "search_error", e)),
    }
}

async fn handle_ingest_all(
    State(state): State<AppState>,
    Query(params): Query<IngestAllParams>,
) -> Json<serde_json::Value> {
    let prefix = params.prefix.unwrap_or_else(|| "_llms/".into());
    let max = params.max.unwrap_or(5);
    let docs_dir = std::env::var("DOCS_DIR").unwrap_or_else(|_| "/data/docs".into());
    let docs_io = match WasmerIo::new(&docs_dir) {
        Ok(io) => io,
        Err(e) => {
            return Json(serde_json::json!({
                "ingested": 0,
                "errors": [format!("DOCS_DIR '{}': {e}", docs_dir)]
            }));
        }
    };
    let (total, errors) = {
        let s = storage_lock(&state);
        ingest_all_from_io(&s.inner, &docs_io, &prefix, max)
    };
    Json(serde_json::json!({"ingested": total, "errors": errors}))
}

async fn handle_state(State(state): State<AppState>) -> Json<nexus_model::BoardState> {
    let board_state = {
        let s = storage_lock(&state);
        block_on(s.inner.read_state())
    };
    Json(board_state)
}

async fn handle_fact(
    State(state): State<AppState>,
    Json(params): Json<FactParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let id = params.id.unwrap_or_else(|| format!("fact_{}", uuid_v4()));
    let fact = Fact {
        id: FihHash::from_hex(&id),
        origin: params.origin,
        content: Content {
            mime_type: "text/plain".into(),
            data: params.content.into_bytes(),
        },
        creator: params.creator,
    };
    let hash = {
        let s = storage_lock(&state);
        let h = block_on(s.inner.submit_fact(&fact))
            .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "fact_error", format!("{e:?}")))?;
        block_on(s.inner.flush_pending())
            .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "flush_error", e))?;
        h
    };
    Ok(Json(serde_json::json!({"id": hash.to_string()})))
}

async fn handle_intent(
    State(state): State<AppState>,
    Json(params): Json<IntentParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let id = params.id.unwrap_or_else(|| format!("intent_{}", uuid_v4()));
    let from_facts: Vec<FihHash> = params.from.as_deref().unwrap_or("").split(',')
        .filter(|s| !s.is_empty()).map(FihHash::from_hex).collect();
    if from_facts.is_empty() {
        return Err(err_response(StatusCode::BAD_REQUEST, "validation_error",
            "intent must be grounded in at least one fact".into()));
    }
    let intent = Intent {
        id: FihHash::from_hex(&id), from_facts, description: params.desc,
        creator: params.creator, worker: None, to_fact_id: None,
        last_heartbeat_at: None, created_at: None,
        is_concluded: false, concluded_at: None,
    };
    let hash = {
        let s = storage_lock(&state);
        let h = block_on(s.inner.submit_intent(&intent))
            .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "intent_error", format!("{e:?}")))?;
        block_on(s.inner.flush_pending())
            .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "flush_error", e))?;
        h
    };
    Ok(Json(serde_json::json!({"id": hash.to_string()})))
}

async fn handle_claim(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(params): Json<ClaimParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let result = {
        let s = storage_lock(&state);
        block_on(s.inner.claim_intent(&intent_id, &params.agent))
    };
    result.map_err(|e| {
        let msg = format!("{e:?}");
        let code = if msg.contains("Conflict") { StatusCode::CONFLICT }
            else if msg.contains("not found") { StatusCode::NOT_FOUND }
            else { StatusCode::INTERNAL_SERVER_ERROR };
        err_response(code, "claim_error", msg)
    })?;
    Ok(Json(serde_json::json!({"status": "claimed"})))
}

async fn handle_heartbeat(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(params): Json<ClaimParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let result = {
        let s = storage_lock(&state);
        block_on(s.inner.heartbeat(&intent_id, &params.agent))
    };
    result.map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "heartbeat_error", format!("{e:?}")))?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

async fn handle_release(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(params): Json<ClaimParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let result = {
        let s = storage_lock(&state);
        block_on(s.inner.release_intent(&intent_id, &params.agent))
    };
    result.map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "release_error", format!("{e:?}")))?;
    Ok(Json(serde_json::json!({"status": "released"})))
}

async fn handle_conclude(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(params): Json<ConcludeParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let fact = {
        let s = storage_lock(&state);
        block_on(s.inner.conclude_intent(&intent_id, &params.result))
    };
    let fact = fact.map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "conclude_error", format!("{e:?}")))?;
    Ok(Json(serde_json::json!({"status": "concluded", "fact_id": fact.id.to_string()})))
}

async fn handle_hint(
    State(state): State<AppState>,
    Json(params): Json<HintParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let id = params.id.unwrap_or_else(|| format!("hint_{}", uuid_v4()));
    let hint = Hint { id: FihHash::from_hex(&id), content: params.content, creator: params.creator };
    let result = {
        let s = storage_lock(&state);
        block_on(s.inner.submit_hint(&hint))
    };
    result.map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "hint_error", format!("{e:?}")))?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

async fn handle_flush(State(state): State<AppState>) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let result = {
        let s = storage_lock(&state);
        block_on(s.inner.flush_pending())
    };
    result.map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "flush_error", e))?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

async fn handle_rebuild(State(state): State<AppState>) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let result = {
        let s = storage_lock(&state);
        let r1 = block_on(s.inner.rebuild_cache());
        r1.and_then(|_| block_on(s.inner.rebuild_semantic()))
    };
    result.map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "rebuild_error", e))?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

// ── Router ───────────────────────────────────────────────────────────

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handle_root))
        .route("/version", get(handle_version))
        .route("/debug/stores", get(handle_debug_stores))
        .route("/ingest", post(handle_ingest))
        .route("/search", get(handle_search))
        .route("/ingest-all", get(handle_ingest_all))
        .route("/state", get(handle_state))
        .route("/fact", post(handle_fact))
        .route("/intent", post(handle_intent))
        .route("/intent/{id}/claim", post(handle_claim))
        .route("/intent/{id}/heartbeat", post(handle_heartbeat))
        .route("/intent/{id}/release", post(handle_release))
        .route("/intent/{id}/conclude", post(handle_conclude))
        .route("/hint", post(handle_hint))
        .route("/flush", get(handle_flush))
        .route("/rebuild", get(handle_rebuild))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ── Entry point ──────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("nexus_gateway_wasmer_ssccsdocs=info")),
        )
        .init();

    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| DEFAULT_DATA_DIR.into());
    let project = std::env::var("PROJECT").unwrap_or_else(|_| "wasmer-ssccsdocs".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    tracing::info!("initializing FIH storage at {data_dir}, project={project}");

    let storage = build_storage(&data_dir, &project);

    match restore_from_snapshot(&storage.inner) {
        Ok(true) => tracing::info!("restored from snapshot ({} facts)", storage.inner.fact_store.len()),
        Ok(false) => tracing::info!("no snapshot found, starting fresh"),
        Err(e) => tracing::warn!("snapshot restore failed (proceeding empty): {e}"),
    }

    let state = Arc::new(Mutex::new(storage));
    let app = build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server error");
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_storage() -> (TempDir, FihStorage<BatchIo<WasmerIo>>) {
        let dir = TempDir::new().unwrap();
        let fs = WasmerIo::new(dir.path().join("fih")).unwrap();
        let io = BatchIo::new(fs);
        let s = FihStorage::new(io, "test");
        s.register_semantic_store(Box::new(InMemoryBm25::new()));
        (dir, s)
    }

    #[test]
    fn test_ingest_and_search() {
        let (_dir, s) = test_storage();
        let id = ingest_document(&s, "Rust is a systems programming language.\nPython is a general purpose language.", "test-doc").unwrap();
        assert!(id.starts_with("f_test-doc_"));
        let query = TextQuery { text: "programming language".into() };
        let results = block_on(s.semantic_search(&query, 5)).unwrap();
        assert!(!results.is_empty(), "search should return results");
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let (_dir, s) = test_storage();
        ingest_document(&s, "Some content for snapshot testing.", "snap-doc").unwrap();

        let fs2 = WasmerIo::new(_dir.path().join("fih")).unwrap();
        let io2 = BatchIo::new(fs2);
        let s2 = FihStorage::new(io2, "test");
        s2.register_semantic_store(Box::new(InMemoryBm25::new()));

        let restored = restore_from_snapshot(&s2).unwrap();
        assert!(restored, "snapshot should be restored");
        assert_eq!(s2.fact_store.len(), 1);
    }

    #[test]
    fn test_search_empty_store() {
        let (_dir, s) = test_storage();
        let query = TextQuery { text: "anything".into() };
        let results = block_on(s.semantic_search(&query, 5)).unwrap();
        assert!(results.is_empty());
    }
}
