// Spin WASI SSCCS docs server (Spin 3.x compatible).
//
// FIH Blackboard + document ingestion + semantic search.
// Targets wasm32-wasip2 (WASI preview 2 / Component Model).
// Deployable to Fermyon Cloud.
//
// tracing events are captured by the Spin runtime; no subscriber is
// needed. For local debugging with `spin up`, use `RUST_LOG=info`.

mod bm25;
mod kv_io;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use http::{Method, Request, Response};
use spin_sdk::http::IntoResponse;
use spin_sdk::http_component;

use nex::io::FileIo;
use nex::EntityStore;
use nex::storage::core::FihStorage;
use nex::storage::semantic::Query as SemanticQuery;
use nexus_model::{
    AsyncFactCapable, AsyncFlushCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead,
    Content, Fact, FihHash, FlushCursor, FlushResult, GovernanceCapable, Hint, Intent,
};
use serde::{Deserialize, Serialize};

use crate::bm25::InMemoryBm25;
use crate::kv_io::KvIo;

const LLMS_TXT_URL: &str = "https://docs.ssccs.org/llms.txt";
const DOCS_BASE_URL: &str = "https://docs.ssccs.org";

type AppStorage = FihStorage<KvIo>;
static STORAGE: OnceLock<AppStorage> = OnceLock::new();
static INITIALIZED: AtomicBool = AtomicBool::new(false);

fn get_storage() -> &'static AppStorage {
    STORAGE.get_or_init(|| {
        tracing::info!("init FIH (KV store) with governance");
        let s = FihStorage::with_governance(KvIo::new().expect("KvIo"), "spin-ssccsdocs");
        s.register_schema("document", b"text/markdown");
        s.register_schema("text", b"text/plain");
        tracing::info!("registered schemas: document, text");
        s.register_semantic_store(Box::new(InMemoryBm25::new()));
        s
    })
}

async fn ensure_initialized() {
    if INITIALIZED.load(Ordering::Acquire) { return; }
    let s = get_storage();
    match restore_from_snapshot(s).await {
        Ok(true) => tracing::info!("restored snapshot ({} facts)", s.fact_store.len().await),
        Ok(false) => {
            tracing::info!("no snapshot found, syncing docs");
            fetch_ssccs_docs(s).await;
        }
        Err(e) => tracing::warn!("snapshot restore failed (proceeding empty): {e}"),
    }
    INITIALIZED.store(true, Ordering::Release);
}

// ── Data types ───────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default, Clone)]
struct DocEntry { content_hash: String }
#[derive(Serialize, Deserialize, Default)]
struct SyncCache { llms_txt_hash: String, docs: std::collections::HashMap<String, DocEntry> }

struct TextQuery { text: String }
impl SemanticQuery for TextQuery {
    fn features(&self) -> Option<Vec<f32>> { None }
    fn text(&self) -> Option<String> { Some(self.text.clone()) }
}

// ── Snapshot ─────────────────────────────────────────────────────────

use nex::storage::core::ChainEntry;
use nex::storage::core::record::{FactRecord, IntentRecord};

/// Cursor key: persisted after each flush_since call.
const CURSOR_KEY: &str = "_snapshot/cursor";
const SNAPSHOT_KEY: &str = "_snapshot/facts.bin";
/// Load the persisted flush cursor, or create a fresh one at epoch.
async fn load_or_create_cursor(s: &FihStorage<impl FileIo>) -> FlushCursor {
    match s.io.read(CURSOR_KEY).await {
        Ok(Some(bytes)) => {
            let s = String::from_utf8_lossy(&bytes);
            let ts: u64 = s.trim().parse().unwrap_or(0);
            FlushCursor { last_flushed_at: ts, partition: "ssccsdocs".into() }
        }
        _ => FlushCursor { last_flushed_at: 0, partition: "ssccsdocs".into() },
    }
}

/// Persist a flush cursor so delta chains are discoverable after restart.
async fn persist_cursor(s: &FihStorage<impl FileIo>, cursor: &FlushCursor) -> Result<(), String> {
    s.io.write(CURSOR_KEY, cursor.last_flushed_at.to_string().as_bytes()).await
}

/// Write a full checkpoint snapshot. Call periodically (not on every ingest).
async fn write_checkpoint(s: &FihStorage<impl FileIo>) -> Result<(), String> {
    let facts: Vec<FactRecord> = s.fact_store.values().await;
    let intents: Vec<IntentRecord> = s.intent_store.values().await;
    let entry = ChainEntry { prev_cursor: 0, records_flushed: facts.len() as u64, facts, intents };
    let bytes = postcard::to_allocvec(&entry).map_err(|e| format!("serialize: {e}"))?;
    s.io.write(SNAPSHOT_KEY, &bytes).await
}

/// Write a delta chain since the last cursor. O(delta) instead of O(n).
async fn write_delta(s: &FihStorage<impl FileIo>) -> Result<(), String> {
    let cursor = load_or_create_cursor(s).await;
    let result = s.flush_since(&cursor).await?;
    // Persist the new cursor so next delta starts from here
    let new_cursor = FlushCursor {
        last_flushed_at: result.new_cursor.last_flushed_at,
        partition: "ssccsdocs".into(),
    };
    persist_cursor(s, &new_cursor).await
    // Delta chains are already written to IO by flush_since:
    //   flush/ssccsdocs/cursor_{ts}.chain
}

/// Load state from full checkpoint + replay delta chains written after it.
async fn restore_from_snapshot(s: &AppStorage) -> Result<bool, String> {
    let Some(bytes) = s.io.read(SNAPSHOT_KEY).await? else { return Ok(false); };
    let entry: nex::storage::core::ChainEntry =
        postcard::from_bytes(&bytes).map_err(|e| format!("deserialize: {e}"))?;
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
    let cursor = load_or_create_cursor(s).await;
    // Replay any delta chains between cursor and now
    if cursor.last_flushed_at > 0 {
        let result = s.flush_since(&cursor).await?;
        tracing::info!(
            "replayed {} records from delta chains",
            result.records_flushed
        );
    }
    s.rebuild_coord().await;
    Ok(true)
}

// ── Ingestion ────────────────────────────────────────────────────────

async fn ingest_document(s: &FihStorage<impl FileIo>, text: &str, origin: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() { return Err("empty".into()); }
    let doc_id = format!("doc_{}", sanitize_id(origin));
    let fact = Fact {
        id: FihHash::from_hex(&doc_id),
        origin: format!("document:{origin}"),
        content: Content { mime_type: "text/markdown".into(), data: text.as_bytes().to_vec() },
        creator: "ingestion-agent".into(),
    };
    AsyncFactCapable::submit_fact(s, &fact).await.map_err(|e| format!("submit: {e:?}"))?;
    // Write delta chain (O(delta)) and persist cursor
    if let Err(e) = write_delta(s).await { tracing::warn!("delta: {e}"); }
    s.flush_pending().await.map_err(|e| format!("flush: {e}"))?;
    Ok(doc_id)
}

fn sanitize_id(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

// ── Docs sync (uses spin_sdk::http::get) ─────────────────────────────

fn extract_llms_urls(text: &str) -> Vec<String> {
    text.lines().filter_map(|line| {
        let line = line.trim();
        if let Some(start) = line.find("](") {
            let rest = &line[start + 2..];
            if let Some(end) = rest.find(')') {
                let url = &rest[..end];
                if url.ends_with(".llms.md") { return Some(url.to_string()); }
            }
        }
        None
    }).collect()
}

fn compute_sha256(data: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(data))
}

fn cache_key() -> String {
    "fih:_cache/sync_state.json".to_string()
}

async fn read_cache() -> SyncCache {
    let store = match spin_sdk::key_value::Store::open_default() {
        Ok(s) => s,
        Err(_) => return SyncCache::default(),
    };
    match store.get(&cache_key()) {
        Ok(Some(b)) => serde_json::from_slice(&b).unwrap_or_default(),
        _ => SyncCache::default(),
    }
}

async fn write_cache(cache: &SyncCache) {
    let store = match spin_sdk::key_value::Store::open_default() {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Ok(b) = serde_json::to_vec(cache) {
        store.set(&cache_key(), &b).ok();
    }
}

async fn fetch_ssccs_docs(s: &AppStorage) -> (usize, Vec<String>) {
    let llms_txt = match fetch_url(LLMS_TXT_URL).await {
        Ok(t) => t, Err(e) => return (0, vec![format!("llms.txt: {e}")]),
    };
    let llms_hash = compute_sha256(llms_txt.as_bytes());
    let mut cache = read_cache().await;
    if cache.llms_txt_hash == llms_hash { tracing::info!("llms.txt unchanged"); return (0, vec![]); }
    let urls = extract_llms_urls(&llms_txt);
    if urls.is_empty() { return (0, vec!["no .llms.md URLs".into()]); }
    let _url_set: std::collections::HashSet<String> = urls.iter().cloned().collect();

    let mut total = 0usize; let mut errors = vec![]; let mut new_cache = std::collections::HashMap::new();
    for url in &urls {
        let origin = url.trim_start_matches('/').trim_end_matches(".llms.md").to_string();
        if let Some(cached) = cache.docs.get(url) { new_cache.insert(url.clone(), cached.clone()); continue; }
        let full_url = if url.starts_with("http") { url.clone() } else { format!("{DOCS_BASE_URL}{url}") };
        match fetch_url(&full_url).await {
            Ok(text) => match ingest_document(s, &text, &origin).await {
                Ok(_) => { total += 1; new_cache.insert(url.clone(), DocEntry { content_hash: compute_sha256(text.as_bytes()) }); }
                Err(e) => errors.push(format!("{url}: {e}")),
            }
            Err(e) => errors.push(format!("{url}: {e}")),
        }
    }
    cache.llms_txt_hash = llms_hash; cache.docs = new_cache; write_cache(&cache).await;
    if total > 0 {
        s.rebuild_coord().await; s.rebuild_semantic().await.ok();
        if let Err(e) = write_checkpoint(s).await { tracing::warn!("checkpoint: {e}"); }
    }
    (total, errors)
}

async fn fetch_url(url: &str) -> Result<String, String> {
    let req = spin_sdk::http::Request::new(spin_sdk::http::Method::Get, url);
    let res: spin_sdk::http::Response = spin_sdk::http::send(req).await.map_err(|e| format!("http: {e}"))?;
    let status = *res.status();
    let body = String::from_utf8(res.into_body()).map_err(|e| format!("utf8: {e}"))?;
    if status == 200u16 { Ok(body) } else { Err(format!("HTTP {status}: {body}")) }
}

// ── Request types ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct IngestParams { text: String, origin: Option<String> }
#[derive(Deserialize)]
struct FactParams { id: Option<String>, origin: String, content: String, creator: String }
#[derive(Deserialize)]
struct IntentParams { id: Option<String>, from: Option<String>, desc: String, creator: String }
#[derive(Deserialize)]
struct ClaimParams { agent: String }
#[derive(Deserialize)]
struct ConcludeParams { result: String }
#[derive(Deserialize)]
struct HintParams { id: Option<String>, content: String, creator: String }

fn ok_json(value: serde_json::Value) -> Result<Response<Vec<u8>>, anyhow::Error> {
    Ok(Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(value.to_string().into_bytes())?)
}

fn err_json(code: u16, error: &str, detail: String) -> Result<Response<Vec<u8>>, anyhow::Error> {
    let body = serde_json::json!({"error": error, "detail": detail}).to_string();
    Ok(Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .body(body.into_bytes())?)
}

fn timestamp_id() -> String {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{ts:x}")
}

// ── Spin HTTP entry point ────────────────────────────────────────────

#[http_component]
async fn handler(req: Request<Vec<u8>>) -> anyhow::Result<impl IntoResponse> {
    get_storage();
    ensure_initialized().await;

    let method_str = req.method().to_string().to_uppercase();
    let path = req.uri().path().to_string();
    let query_str = req.uri().query().unwrap_or("").to_string();
    let method = req.method().clone();
    let body = req.into_body();

    let resp = match (method, path.as_str()) {
        (Method::GET, "/") => ok_json(serde_json::json!("ok")),
        (Method::GET, "/version") => ok_json(serde_json::json!("1")),
        (Method::GET, "/debug/stores") => {
            let s = get_storage();
            ok_json(serde_json::json!({"stores": s.semantic_stores().len(), "fact_store": s.fact_store.len().await, "service": "nexus-spin-ssccsdocs"}))
        }
        (Method::POST, "/ingest") => {
            let params: IngestParams = match serde_json::from_slice(&body) { Ok(p) => p, Err(e) => return err_json(400, "invalid_json", format!("{e}")) };
            let origin = params.origin.unwrap_or_else(|| "ingest".into());
            match ingest_document(get_storage(), &params.text, &origin).await {
                Ok(id) => ok_json(serde_json::json!({"status": "ingested", "id": id})),
                Err(e) => err_json(500, "ingest_error", e),
            }
        }
        (Method::GET, "/search") => {
            let q = query_str.split('&').find_map(|p| p.strip_prefix("q=")).map(urldecode).unwrap_or_default();
            let top_k = query_str.split('&').find_map(|p| p.strip_prefix("top_k=")).and_then(|v| v.parse().ok()).unwrap_or(10);
            if q.is_empty() { return err_json(400, "validation_error", "missing q".into()); }
            let query = TextQuery { text: q };
            match get_storage().semantic_search(&query, top_k).await {
                Ok(results) => {
                    let items: Vec<_> = results.iter().map(|(i, s)| serde_json::json!({"index": i, "score": s, "id": get_storage().resolve_semantic_idx(*i)})).collect();
                    ok_json(serde_json::json!({"results": items}))
                }
                Err(e) => err_json(500, "search_error", e),
            }
        }
        (Method::GET | Method::POST, "/sync-docs") => {
            let (total, errors) = fetch_ssccs_docs(get_storage()).await;
            ok_json(serde_json::json!({"status": if errors.is_empty() { "ok" } else { "partial" }, "ingested": total, "errors": errors}))
        }
        (Method::GET, "/state") => {
            let state = get_storage().read_state().await;
            ok_json(serde_json::to_value(state).unwrap_or_default())
        }
        (Method::POST, "/fact") => {
            let params: FactParams = match serde_json::from_slice(&body) { Ok(p) => p, Err(e) => return err_json(400, "invalid_json", format!("{e}")) };
            let id = params.id.unwrap_or_else(|| format!("fact_{}", timestamp_id()));
            let fact = Fact { id: FihHash::from_hex(&id), origin: params.origin, content: Content { mime_type: "text/plain".into(), data: params.content.into_bytes() }, creator: params.creator };
            let hash = match get_storage().submit_fact(&fact).await { Ok(h) => h, Err(e) => return err_json(500, "fact_error", format!("{e:?}")) };
            if let Err(e) = get_storage().flush_pending().await { return err_json(500, "flush_error", e); }
            ok_json(serde_json::json!({"id": hash.to_string()}))
        }
        (Method::POST, "/intent") => {
            let params: IntentParams = match serde_json::from_slice(&body) { Ok(p) => p, Err(e) => return err_json(400, "invalid_json", format!("{e}")) };
            let id = params.id.unwrap_or_else(|| format!("intent_{}", timestamp_id()));
            let from_facts: Vec<FihHash> = params.from.as_deref().unwrap_or("").split(',').filter(|s| !s.is_empty()).map(FihHash::from_hex).collect();
            if from_facts.is_empty() { return err_json(400, "validation_error", "intent needs at least one fact".into()); }
            let intent = Intent { id: FihHash::from_hex(&id), from_facts, description: params.desc, creator: params.creator, worker: None, to_fact_id: None, last_heartbeat_at: None, created_at: None, is_concluded: false, concluded_at: None };
            let hash = match get_storage().submit_intent(&intent).await { Ok(h) => h, Err(e) => return err_json(500, "intent_error", format!("{e:?}")) };
            ok_json(serde_json::json!({"id": hash.to_string()}))
        }
        (Method::POST, p) if p.starts_with("/intent/") => {
            let s = p.trim_start_matches("/intent/");
            let params: ClaimParams = match serde_json::from_slice(&body) { Ok(p) => p, Err(e) => return err_json(400, "invalid_json", format!("{e}")) };
            if let Some(id) = s.strip_suffix("/claim") {
                match get_storage().claim_intent(id, &params.agent).await {
                    Ok(_) => ok_json(serde_json::json!({"status": "claimed"})),
                    Err(e) => { let m = format!("{e:?}"); err_json(if m.contains("Conflict") { 409 } else if m.contains("not found") { 404 } else { 500 }, "claim_error", m) }
                }
            } else if let Some(id) = s.strip_suffix("/heartbeat") {
                match get_storage().heartbeat(id, &params.agent).await { Ok(_) => ok_json(serde_json::json!({"status": "ok"})), Err(e) => err_json(500, "heartbeat_error", format!("{e:?}")) }
            } else if let Some(id) = s.strip_suffix("/release") {
                match get_storage().release_intent(id, &params.agent).await { Ok(_) => ok_json(serde_json::json!({"status": "released"})), Err(e) => err_json(500, "release_error", format!("{e:?}")) }
            } else if let Some(id) = s.strip_suffix("/conclude") {
                let cp: ConcludeParams = match serde_json::from_slice(&body) { Ok(p) => p, Err(e) => return err_json(400, "invalid_json", format!("{e}")) };
                match get_storage().conclude_intent(id, &cp.result).await { Ok(f) => ok_json(serde_json::json!({"status": "concluded", "fact_id": f.id.to_string()})), Err(e) => err_json(500, "conclude_error", format!("{e:?}")) }
            } else { err_json(404, "not_found", format!("no handler for {p}")) }
        }
        (Method::POST, "/hint") => {
            let params: HintParams = match serde_json::from_slice(&body) { Ok(p) => p, Err(e) => return err_json(400, "invalid_json", format!("{e}")) };
            let id = params.id.unwrap_or_else(|| format!("hint_{}", timestamp_id()));
            let hint = Hint { id: FihHash::from_hex(&id), content: params.content, creator: params.creator };
            if let Err(e) = get_storage().submit_hint(&hint).await { return err_json(500, "hint_error", format!("{e:?}")); }
            ok_json(serde_json::json!({"status": "ok"}))
        }
        (Method::GET | Method::POST, "/flush") => {
            if let Err(e) = get_storage().flush_pending().await { return err_json(500, "flush_error", e); }
            ok_json(serde_json::json!({"status": "ok"}))
        }
        (Method::GET | Method::POST, "/rebuild") => {
            if let Err(e) = get_storage().rebuild_cache().await { return err_json(500, "rebuild_error", e); }
            if let Err(e) = get_storage().rebuild_semantic().await { return err_json(500, "rebuild_semantic_error", e); }
            ok_json(serde_json::json!({"status": "ok"}))
        }
        // ── Delta sync (cursor-based, Copia/DeltaMCP pattern) ──────
        (Method::GET, "/delta") => {
            let cursor_str = query_str.split('&').find_map(|p| p.strip_prefix("cursor=")).map(urldecode).unwrap_or_default();
            let has_more;
            let result = if cursor_str.is_empty() {
                // No cursor: return checkpoint cursor + full state marker
                let cursor = load_or_create_cursor(get_storage()).await;
                has_more = true;
                FlushResult { records_flushed: 0, new_cursor: cursor }
            } else {
                let ts: u64 = cursor_str.parse().unwrap_or(0);
                let cursor = FlushCursor { last_flushed_at: ts, partition: "ssccsdocs".into() };
                match get_storage().flush_since(&cursor).await {
                    Ok(r) => {
                        has_more = r.records_flushed > 0;
                        r
                    }
                    Err(e) => return err_json(500, "delta_error", e),
                }
            };
            ok_json(serde_json::json!({
                "cursor": result.new_cursor.last_flushed_at.to_string(),
                "hasMore": has_more,
            }))
        }
        // ── Full checkpoint (SurrealKV snapshot pattern) ────────────
        (Method::POST, "/checkpoint") => {
            match write_checkpoint(get_storage()).await {
                Ok(()) => ok_json(serde_json::json!({"status": "checkpointed"})),
                Err(e) => err_json(500, "checkpoint_error", e),
            }
        }
        _ => err_json(404, "not_found", format!("no route for {method_str} {path}")),
    };
    resp
}

fn urldecode(s: &str) -> String {
    let mut bytes: Vec<u8> = Vec::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '+' => bytes.push(b' '),
            '%' => {
                let hi = chars.next().and_then(|x| x.to_digit(16)).unwrap_or(0);
                let lo = chars.next().and_then(|x| x.to_digit(16)).unwrap_or(0);
                bytes.push((hi * 16 + lo) as u8);
            }
            c => {
                let mut buf = [0u8; 4];
                let encoded = c.encode_utf8(&mut buf);
                bytes.extend_from_slice(encoded.as_bytes());
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nex::io::FsIo;

    fn test_storage() -> FihStorage<FsIo> {
        let dir = tempfile::tempdir().unwrap();
        let io = FsIo::new(dir.path()).unwrap();
        let s = FihStorage::new(io, "test");
        s.register_semantic_store(Box::new(InMemoryBm25::new()));
        s
    }

    #[tokio::test]
    async fn test_ingest_and_search() {
        let s = test_storage();
        let id = ingest_document(&s, "hello world", "test").await.unwrap();
        assert!(!id.is_empty());
        let q = TextQuery { text: "hello".into() };
        let r = s.semantic_search(&q, 10).await.unwrap();
        assert!(!r.is_empty());
    }

    #[tokio::test]
    async fn test_search_empty() {
        let s = test_storage();
        let q = TextQuery { text: "x".into() };
        let r = s.semantic_search(&q, 10).await.unwrap();
        assert!(r.is_empty());
    }
}
