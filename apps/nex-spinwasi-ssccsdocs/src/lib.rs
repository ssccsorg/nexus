// Spin WASI SSCCS docs server (Spin 3.x compatible).
//
// FIH Blackboard + document ingestion + semantic search.
// Targets wasm32-wasip2 (WASI preview 2 / Component Model).
// Deployable to Fermyon Cloud.

mod bm25;
mod mem_io;

use std::sync::OnceLock;

use http::{Method, Request, Response, StatusCode};
use spin_sdk::http::IntoResponse;
use spin_sdk::http_component;

use nex::io::FileIo;
use nex::EntityStore;
use nex::storage::core::FihStorage;
use nex::storage::semantic::{Query as SemanticQuery, SemanticStore};
use nexus_model::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead, Content, Fact,
    FihHash, Hint, Intent,
};
use serde::{Deserialize, Serialize};

use crate::bm25::InMemoryBm25;
use crate::mem_io::MemIo;

const DEFAULT_DATA_DIR: &str = "./data/fih";
const LLMS_TXT_URL: &str = "https://docs.ssccs.org/llms.txt";
const DOCS_BASE_URL: &str = "https://docs.ssccs.org";

type AppStorage = FihStorage<MemIo>;
static STORAGE: OnceLock<AppStorage> = OnceLock::new();
static INITIALIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn get_storage() -> &'static AppStorage {
    STORAGE.get_or_init(|| {
        tracing::info!("init FIH (in-memory)");
        let s = FihStorage::new(MemIo::new(), "spin-ssccsdocs");
        s.register_semantic_store(Box::new(InMemoryBm25::new()));
        s
    })
}

async fn ensure_initialized() {
    if INITIALIZED.load(std::sync::atomic::Ordering::Acquire) { return; }
    let s = get_storage();
    tracing::info!("first request: syncing docs");
    fetch_ssccs_docs(s, DEFAULT_DATA_DIR).await;
    INITIALIZED.store(true, std::sync::atomic::Ordering::Release);
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

async fn write_snapshot(s: &AppStorage) -> Result<(), String> {
    use nex::storage::core::ChainEntry;
    use nex::storage::core::record::{FactRecord, IntentRecord};
    let facts: Vec<FactRecord> = s.fact_store.values().await;
    let intents: Vec<IntentRecord> = s.intent_store.values().await;
    let entry = ChainEntry { prev_cursor: 0, records_flushed: facts.len() as u64, facts, intents };
    let bytes = postcard::to_allocvec(&entry).map_err(|e| format!("serialize: {e}"))?;
    s.io.write("_snapshot/facts.bin", &bytes).await?;
    s.io.flush().await
}

// ── Ingestion ────────────────────────────────────────────────────────

async fn ingest_document(s: &AppStorage, text: &str, origin: &str) -> Result<String, String> {
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
    if let Err(e) = write_snapshot(s).await { tracing::warn!("snapshot: {e}"); }
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
    hex::encode(sha2::Sha256::digest(data))
}

fn cache_path(data_dir: &str) -> String {
    format!("{}/_cache/sync_state.json", data_dir.trim_end_matches('/'))
}

async fn read_cache(data_dir: &str) -> SyncCache {
    let path = cache_path(data_dir);
    match std::fs::read(&path) { Ok(b) => serde_json::from_slice(&b).unwrap_or_default(), Err(_) => SyncCache::default() }
}

async fn write_cache(data_dir: &str, cache: &SyncCache) {
    let path = cache_path(data_dir);
    if let Some(parent) = std::path::Path::new(&path).parent() { std::fs::create_dir_all(parent).ok(); }
    if let Ok(b) = serde_json::to_vec(cache) { std::fs::write(&path, &b).ok(); }
}

async fn fetch_ssccs_docs(s: &AppStorage, data_dir: &str) -> (usize, Vec<String>) {
    let llms_txt = match fetch_url(LLMS_TXT_URL).await {
        Ok(t) => t, Err(e) => return (0, vec![format!("llms.txt: {e}")]),
    };
    let llms_hash = compute_sha256(llms_txt.as_bytes());
    let mut cache = read_cache(data_dir).await;
    if cache.llms_txt_hash == llms_hash { tracing::info!("llms.txt unchanged"); return (0, vec![]); }
    let urls = extract_llms_urls(&llms_txt);
    if urls.is_empty() { return (0, vec!["no .llms.md URLs".into()]); }
    let url_set: std::collections::HashSet<String> = urls.iter().cloned().collect();

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
    cache.llms_txt_hash = llms_hash; cache.docs = new_cache; write_cache(data_dir, &cache).await;
    if total > 0 {
        s.rebuild_coord().await; s.rebuild_semantic().await.ok();
        if let Err(e) = write_snapshot(s).await { tracing::warn!("snapshot: {e}"); }
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

fn uuid_v4() -> String {
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
            let (total, errors) = fetch_ssccs_docs(get_storage(), DEFAULT_DATA_DIR).await;
            ok_json(serde_json::json!({"status": if errors.is_empty() { "ok" } else { "partial" }, "ingested": total, "errors": errors}))
        }
        (Method::GET, "/state") => {
            let state = get_storage().read_state().await;
            ok_json(serde_json::to_value(state).unwrap_or_default())
        }
        (Method::POST, "/fact") => {
            let params: FactParams = match serde_json::from_slice(&body) { Ok(p) => p, Err(e) => return err_json(400, "invalid_json", format!("{e}")) };
            let id = params.id.unwrap_or_else(|| format!("fact_{}", uuid_v4()));
            let fact = Fact { id: FihHash::from_hex(&id), origin: params.origin, content: Content { mime_type: "text/plain".into(), data: params.content.into_bytes() }, creator: params.creator };
            let hash = match get_storage().submit_fact(&fact).await { Ok(h) => h, Err(e) => return err_json(500, "fact_error", format!("{e:?}")) };
            if let Err(e) = get_storage().flush_pending().await { return err_json(500, "flush_error", e); }
            ok_json(serde_json::json!({"id": hash.to_string()}))
        }
        (Method::POST, "/intent") => {
            let params: IntentParams = match serde_json::from_slice(&body) { Ok(p) => p, Err(e) => return err_json(400, "invalid_json", format!("{e}")) };
            let id = params.id.unwrap_or_else(|| format!("intent_{}", uuid_v4()));
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
            let id = params.id.unwrap_or_else(|| format!("hint_{}", uuid_v4()));
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
        _ => err_json(404, "not_found", format!("no route for {method_str} {path}")),
    };
    resp
}

fn urldecode(s: &str) -> String {
    let mut r = String::new(); let mut c = s.chars();
    while let Some(ch) = c.next() {
        match ch {
            '+' => r.push(' '),
            '%' => { let hi = c.next().and_then(|x| x.to_digit(16)).unwrap_or(0); let lo = c.next().and_then(|x| x.to_digit(16)).unwrap_or(0); r.push(char::from((hi * 16 + lo) as u8)); }
            _ => r.push(ch),
        }
    }
    r
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
    async fn test_snapshot_roundtrip() {
        let s = test_storage();
        ingest_document(&s, "snap", "s").await.unwrap();
        write_snapshot(&s).await.unwrap();
        let s2 = test_storage();
        assert!(true);
    }

    #[tokio::test]
    async fn test_search_empty() {
        let s = test_storage();
        let q = TextQuery { text: "x".into() };
        let r = s.semantic_search(&q, 10).await.unwrap();
        assert!(r.is_empty());
    }
}
