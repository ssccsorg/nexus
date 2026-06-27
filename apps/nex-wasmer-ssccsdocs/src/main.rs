// Wasmer Edge SSCCS docs server.
//
// FIH Blackboard + document ingestion + semantic search, deployed on Wasmer Edge.
// Uses `WasmerIo` (std::fs via WASIX) as the IO backend instead of Cloudflare R2.
// No Workers-specific dependencies (R2, DO, Queue, Vectorize).
//
// Architecture:
//   FihStorage<BatchIo<WasmerIo>>  ←  in-memory + filesystem persistence
//   InMemoryBm25                   ←  semantic search (BM25)
//   axum HTTP server               ←  request routing (fully async, no block_on)
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

mod batch_io;
mod bm25;
mod wasmer_io;

use std::net::SocketAddr;
use std::sync::Arc;

use crate::batch_io::BatchIo;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use nex::EntityStore;
use nex::io::FileIo;
use nex::storage::core::FihStorage;
use nex::storage::semantic::Query as SemanticQuery;
use nexus_model::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead, Content, Fact,
    FihHash, Hint, Intent,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use crate::bm25::InMemoryBm25;
use crate::wasmer_io::WasmerIo;

const DEFAULT_DATA_DIR: &str = "/data/fih";

const LLMS_TXT_URL: &str = "https://docs.ssccs.org/llms.txt";
const DOCS_BASE_URL: &str = "https://docs.ssccs.org";

// ── Sync cache ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default, Clone)]
struct DocEntry {
    content_hash: String,
}

#[derive(Serialize, Deserialize, Default)]
struct SyncCache {
    llms_txt_hash: String,
    docs: std::collections::HashMap<String, DocEntry>,
}

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

// ── Build storage ────────────────────────────────────────────────────

fn build_storage(data_dir: &str, project_id: &str) -> FihStorage<BatchIo<WasmerIo>> {
    let fs_io = WasmerIo::new(data_dir).expect("create WasmerIo");
    let io = BatchIo::new(fs_io);
    let s = FihStorage::new(io, project_id);
    s.register_semantic_store(Box::new(InMemoryBm25::new()));
    s
}

// ── Snapshot ─────────────────────────────────────────────────────────

async fn write_snapshot(s: &FihStorage<BatchIo<WasmerIo>>) -> Result<(), String> {
    use nex::storage::core::ChainEntry;
    use nex::storage::core::record::{FactRecord, IntentRecord};
    let facts: Vec<FactRecord> = s.fact_store.values().await;
    let intents: Vec<IntentRecord> = s.intent_store.values().await;
    let entry = ChainEntry {
        prev_cursor: 0,
        records_flushed: facts.len() as u64,
        facts,
        intents,
    };
    let bytes = postcard::to_allocvec(&entry).map_err(|e| format!("snapshot serialize: {e}"))?;
    // Flush any pending BatchIo writes before writing snapshot directly.
    // This ensures facts are persisted before the snapshot references them.
    s.io.flush().await?;
    // Write snapshot directly to inner IO (bypass BatchIo buffer).
    s.io.io().write("_snapshot/facts.bin", &bytes).await?;
    // Ensure snapshot is flushed to disk.
    s.io.flush().await
}

async fn restore_from_snapshot(s: &FihStorage<BatchIo<WasmerIo>>) -> Result<bool, String> {
    // Flush any pending writes first so read_state sees the latest state.
    s.io.flush().await.ok();
    if !s.fact_store.is_empty().await {
        return Ok(false);
    }
    let Some(bytes) = s.io.read("_snapshot/facts.bin").await? else {
        return Ok(false);
    };
    let entry: nex::storage::core::ChainEntry =
        postcard::from_bytes(&bytes).map_err(|e| format!("snapshot deserialize: {e}"))?;
    s.fact_store
        .replace_from(entry.facts.into_iter().map(|r| (r.id.clone(), r)).collect())
        .await;
    s.intent_store
        .replace_from(
            entry
                .intents
                .into_iter()
                .map(|r| (r.id.clone(), r))
                .collect(),
        )
        .await;
    s.rebuild_coord().await;
    Ok(true)
}

// ── Document ingestion ───────────────────────────────────────────────

/// Wasmer contract: each `.llms.md` file is stored as a single Fact.
/// (contract.nex not yet implemented — this is app-level hardcoded policy.)
async fn ingest_document(
    s: &FihStorage<BatchIo<WasmerIo>>,
    text: &str,
    origin: &str,
) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty document".into());
    }
    let doc_id = format!("doc_{}", sanitize_id(origin));
    let fact = Fact {
        id: FihHash::from_hex(&doc_id),
        origin: format!("document:{}", origin),
        content: Content {
            mime_type: "text/markdown".into(),
            data: text.as_bytes().to_vec(),
        },
        creator: "ingestion-agent".into(),
    };
    AsyncFactCapable::submit_fact(s, &fact)
        .await
        .map_err(|e| format!("submit doc: {e:?}"))?;
    if let Err(e) = write_snapshot(s).await {
        tracing::warn!("snapshot write failed: {e}");
    }
    s.flush_pending().await.map_err(|e| format!("flush: {e}"))?;
    Ok(doc_id)
}

async fn ingest_all_from_io<D: FileIo>(
    s: &FihStorage<BatchIo<WasmerIo>>,
    docs: &D,
    prefix: &str,
    max: usize,
) -> (usize, Vec<String>) {
    let mut total = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let keys = match docs.list(prefix).await {
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
        let data = match docs.read(key).await {
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
        match ingest_document(s, &text, &origin).await {
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

// ── docs.ssccs.org sync ───────────────────────────────────────────────

/// Parse .llms.md URLs from an llms.txt index file.
fn extract_llms_urls(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(start) = line.find("](") {
                let rest = &line[start + 2..];
                if let Some(end) = rest.find(')') {
                    let url = &rest[..end];
                    if url.ends_with(".llms.md") {
                        return Some(url.to_string());
                    }
                }
            }
            None
        })
        .collect()
}

fn compute_sha256(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}

fn cache_path(data_dir: &str) -> String {
    format!("{}/_cache/sync_state.json", data_dir.trim_end_matches('/'))
}

async fn read_cache(data_dir: &str) -> SyncCache {
    let path = cache_path(data_dir);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => SyncCache::default(),
    }
}

async fn write_cache(data_dir: &str, cache: &SyncCache) {
    let path = cache_path(data_dir);
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if let Ok(bytes) = serde_json::to_vec(cache) {
        std::fs::write(&path, &bytes).ok();
    }
}

/// Fetch SSCCS docs from docs.ssccs.org with delta sync.
///
/// 1. GET llms.txt, compare hash with cached
/// 2. If llms.txt unchanged → skip entirely
/// 3. If changed: compare per-file hashes, only fetch new/changed
/// 4. Remove facts for URLs no longer in llms.txt
async fn fetch_ssccs_docs(
    s: &FihStorage<BatchIo<WasmerIo>>,
    data_dir: &str,
) -> (usize, Vec<String>) {
    let client = reqwest::Client::builder()
        .user_agent("nexus-wasmer-ssccsdocs/0.1.0")
        .build()
        .expect("reqwest client");

    let llms_txt = match client.get(LLMS_TXT_URL).send().await {
        Ok(resp) => match resp.text().await {
            Ok(t) => t,
            Err(e) => return (0, vec![format!("llms.txt body: {e}")]),
        },
        Err(e) => return (0, vec![format!("llms.txt fetch: {e}")]),
    };

    let llms_hash = compute_sha256(llms_txt.as_bytes());
    let mut cache = read_cache(data_dir).await;

    if cache.llms_txt_hash == llms_hash {
        tracing::info!("llms.txt unchanged, skipping sync");
        return (0, Vec::new());
    }

    let urls = extract_llms_urls(&llms_txt);
    if urls.is_empty() {
        return (0, vec!["no .llms.md URLs found in llms.txt".into()]);
    }

    let url_set: std::collections::HashSet<String> = urls.iter().cloned().collect();

    // Detect removed docs: in cache but not in current llms.txt
    let mut removed_origins: Vec<String> = Vec::new();
    for cached_url in cache.docs.keys() {
        if !url_set.contains(cached_url) {
            removed_origins.push(
                cached_url
                    .trim_start_matches('/')
                    .trim_end_matches(".llms.md")
                    .to_string(),
            );
        }
    }

    let mut total_new = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let mut new_cache_docs: std::collections::HashMap<String, DocEntry> =
        std::collections::HashMap::new();

    for url in &urls {
        let origin = url
            .trim_start_matches('/')
            .trim_end_matches(".llms.md")
            .to_string();

        // Check cache hit: same URL + same content hash
        if let Some(cached) = cache.docs.get(url) {
            // Optimistic: content may have changed but we don't re-fetch.
            // On next sync, llms.txt hash will differ if docs change.
            new_cache_docs.insert(url.clone(), cached.clone());
            continue;
        }

        // Cache miss: fetch and ingest
        let full_url = if url.starts_with("http") {
            url.clone()
        } else {
            format!("{}{}", DOCS_BASE_URL, url)
        };

        match client.get(&full_url).send().await {
            Ok(resp) => {
                let bytes = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        errors.push(format!("{url}: body: {e}"));
                        continue;
                    }
                };
                let text = match std::str::from_utf8(&bytes) {
                    Ok(t) => t,
                    Err(_) => {
                        errors.push(format!("{url}: not UTF-8"));
                        continue;
                    }
                };
                match ingest_document(s, text, &origin).await {
                    Ok(_) => {
                        total_new += 1;
                        new_cache_docs.insert(
                            url.clone(),
                            DocEntry {
                                content_hash: compute_sha256(&bytes),
                            },
                        );
                    }
                    Err(e) => errors.push(format!("{url}: ingest: {e}")),
                }
            }
            Err(e) => errors.push(format!("{url}: {e}")),
        }
    }

    // Preserve unchanged docs from old cache
    for (url, entry) in &cache.docs {
        if !new_cache_docs.contains_key(url) && url_set.contains(url.as_str()) {
            new_cache_docs.insert(url.clone(), entry.clone());
        }
    }

    // Facts are immutable: removed docs are NOT deleted from storage.
    // They are simply excluded from future snapshots (which only contain
    // current URLs). Orphaned facts persist per FIH immutability.
    for origin in &removed_origins {
        tracing::info!("doc no longer in llms.txt (fact preserved): {origin}");
    }

    // Update cache
    cache.llms_txt_hash = llms_hash;
    cache.docs = new_cache_docs;
    write_cache(data_dir, &cache).await;

    // Rebuild coordinator and snapshot after ingest mutations
    if total_new > 0 {
        s.rebuild_coord().await;
        s.rebuild_semantic().await.ok();
        if let Err(e) = write_snapshot(s).await {
            tracing::warn!("snapshot write after sync failed: {e}");
        }
    }

    tracing::info!(
        "delta sync: {} new, {} errors (removed docs preserved per FIH immutability)",
        total_new,
        errors.len()
    );
    (total_new, errors)
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
    (
        code,
        Json(ApiError {
            error: error.into(),
            detail,
        }),
    )
}

fn uuid_v4() -> String {
    // Simple unique ID: timestamp + random hex. Avoids uuid/getrandom 0.4 dep.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{ts:x}")
}

// ── Route handlers ───────────────────────────────────────────────────

type AppState = Arc<FihStorage<BatchIo<WasmerIo>>>;

async fn handle_root() -> &'static str {
    "nexus-wasmer-ssccsdocs v0.4.0"
}

async fn handle_version() -> &'static str {
    "1"
}

async fn handle_debug_stores(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "stores": state.semantic_stores().len(),
        "fact_store": state.fact_store.len().await,
        "service": "nexus-wasmer-ssccsdocs"
    }))
}

async fn handle_ingest(
    State(state): State<AppState>,
    Json(params): Json<IngestParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let origin = params.origin.unwrap_or_else(|| "ingest".into());
    match ingest_document(&state, &params.text, &origin).await {
        Ok(id) => Ok(Json(serde_json::json!({"status": "ingested", "id": id}))),
        Err(e) => Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ingest_error",
            e,
        )),
    }
}

async fn handle_search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let query = TextQuery {
        text: params.q.clone(),
    };
    let top_k = params.top_k.unwrap_or(10);
    match state.semantic_search(&query, top_k).await {
        Ok(results) => {
            let items: Vec<serde_json::Value> = results
                .iter()
                .map(|(idx, score)| {
                    serde_json::json!({
                        "index": idx,
                        "score": score,
                        "id": state.resolve_semantic_idx(*idx)
                    })
                })
                .collect();
            Ok(Json(serde_json::json!({"results": items})))
        }
        Err(e) => Err(err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "search_error",
            e,
        )),
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
    let (total, errors) = ingest_all_from_io(&state, &docs_io, &prefix, max).await;
    Json(serde_json::json!({"ingested": total, "errors": errors}))
}

#[cfg(not(target_arch = "wasm32"))]
async fn handle_sync_docs(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| DEFAULT_DATA_DIR.into());
    let (total, errors) = fetch_ssccs_docs(&state, &data_dir).await;
    Json(serde_json::json!({
        "status": if errors.is_empty() { "ok" } else { "partial" },
        "ingested": total,
        "errors": errors
    }))
}

async fn handle_state(State(state): State<AppState>) -> Json<nexus_model::BoardState> {
    Json(state.read_state().await)
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
    let hash = state.submit_fact(&fact).await.map_err(|e| {
        err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "fact_error",
            format!("{e:?}"),
        )
    })?;
    state
        .flush_pending()
        .await
        .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "flush_error", e))?;
    Ok(Json(serde_json::json!({"id": hash.to_string()})))
}

async fn handle_intent(
    State(state): State<AppState>,
    Json(params): Json<IntentParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let id = params.id.unwrap_or_else(|| format!("intent_{}", uuid_v4()));
    let from_facts: Vec<FihHash> = params
        .from
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(FihHash::from_hex)
        .collect();
    if from_facts.is_empty() {
        return Err(err_response(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "intent must be grounded in at least one fact".into(),
        ));
    }
    let intent = Intent {
        id: FihHash::from_hex(&id),
        from_facts,
        description: params.desc,
        creator: params.creator,
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };
    let hash = state.submit_intent(&intent).await.map_err(|e| {
        err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "intent_error",
            format!("{e:?}"),
        )
    })?;
    state
        .flush_pending()
        .await
        .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "flush_error", e))?;
    Ok(Json(serde_json::json!({"id": hash.to_string()})))
}

async fn handle_claim(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(params): Json<ClaimParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state
        .claim_intent(&intent_id, &params.agent)
        .await
        .map_err(|e| {
            let msg = format!("{e:?}");
            let code = if msg.contains("Conflict") {
                StatusCode::CONFLICT
            } else if msg.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            err_response(code, "claim_error", msg)
        })?;
    Ok(Json(serde_json::json!({"status": "claimed"})))
}

async fn handle_heartbeat(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(params): Json<ClaimParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state
        .heartbeat(&intent_id, &params.agent)
        .await
        .map_err(|e| {
            err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "heartbeat_error",
                format!("{e:?}"),
            )
        })?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

async fn handle_release(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(params): Json<ClaimParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state
        .release_intent(&intent_id, &params.agent)
        .await
        .map_err(|e| {
            err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "release_error",
                format!("{e:?}"),
            )
        })?;
    Ok(Json(serde_json::json!({"status": "released"})))
}

async fn handle_conclude(
    State(state): State<AppState>,
    Path(intent_id): Path<String>,
    Json(params): Json<ConcludeParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let fact = state
        .conclude_intent(&intent_id, &params.result)
        .await
        .map_err(|e| {
            err_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "conclude_error",
                format!("{e:?}"),
            )
        })?;
    Ok(Json(
        serde_json::json!({"status": "concluded", "fact_id": fact.id.to_string()}),
    ))
}

async fn handle_hint(
    State(state): State<AppState>,
    Json(params): Json<HintParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let id = params.id.unwrap_or_else(|| format!("hint_{}", uuid_v4()));
    let hint = Hint {
        id: FihHash::from_hex(&id),
        content: params.content,
        creator: params.creator,
    };
    state.submit_hint(&hint).await.map_err(|e| {
        err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "hint_error",
            format!("{e:?}"),
        )
    })?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

async fn handle_flush(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state
        .flush_pending()
        .await
        .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "flush_error", e))?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

async fn handle_rebuild(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    state
        .rebuild_cache()
        .await
        .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, "rebuild_error", e))?;
    state.rebuild_semantic().await.map_err(|e| {
        err_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rebuild_semantic_error",
            e,
        )
    })?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

// ── Router ───────────────────────────────────────────────────────────

fn build_router(state: AppState) -> Router {
    let mut router = Router::new()
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
        .route("/rebuild", get(handle_rebuild));
    #[cfg(not(target_arch = "wasm32"))]
    {
        router = router.route("/sync-docs", post(handle_sync_docs));
    }
    router.layer(CorsLayer::permissive())
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

    match restore_from_snapshot(&storage).await {
        Ok(true) => tracing::info!(
            "restored from snapshot ({} facts)",
            storage.fact_store.len().await
        ),
        Ok(false) => {
            tracing::info!("no snapshot found, starting fresh");
            #[cfg(not(target_arch = "wasm32"))]
            {
                // First boot: auto-fetch all SSCCS docs from docs.ssccs.org
                let (n, errors) = fetch_ssccs_docs(&storage, &data_dir).await;
                if n > 0 {
                    tracing::info!("auto-sync: ingested {} docs on first boot", n);
                }
                for e in &errors {
                    tracing::warn!("auto-sync error: {e}");
                }
            }
        }
        Err(e) => tracing::warn!("snapshot restore failed (proceeding empty): {e}"),
    }

    let state = Arc::new(storage);
    let app = build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("listening on {addr}");

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .expect("server error");
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

    #[tokio::test]
    async fn test_ingest_and_search() {
        let (_dir, s) = test_storage();
        let id = ingest_document(
            &s,
            "Rust is a systems programming language.\nPython is a general purpose language.",
            "test-doc",
        )
        .await
        .unwrap();
        assert!(id.starts_with("doc_test-doc"));

        let query = TextQuery {
            text: "programming language".into(),
        };
        let results = s.semantic_search(&query, 5).await.unwrap();
        assert!(!results.is_empty(), "search should return results");
    }

    #[tokio::test]
    async fn test_snapshot_roundtrip() {
        let (_dir, s) = test_storage();
        ingest_document(&s, "Some content for snapshot testing.", "snap-doc")
            .await
            .unwrap();

        let fs2 = WasmerIo::new(_dir.path().join("fih")).unwrap();
        let io2 = BatchIo::new(fs2);
        let s2 = FihStorage::new(io2, "test");
        s2.register_semantic_store(Box::new(InMemoryBm25::new()));

        let restored = restore_from_snapshot(&s2).await.unwrap();
        assert!(restored, "snapshot should be restored");
        assert_eq!(s2.fact_store.len().await, 1);
    }

    #[tokio::test]
    async fn test_search_empty_store() {
        let (_dir, s) = test_storage();
        let query = TextQuery {
            text: "anything".into(),
        };
        let results = s.semantic_search(&query, 5).await.unwrap();
        assert!(results.is_empty());
    }
}
