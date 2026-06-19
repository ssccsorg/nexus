// gateway/nex-cf — Consumes FihStorage<CfFihIo> via async traits.
// No block_on. No locks. Pure async over R2.

pub mod stores;
pub mod batch_io;
pub mod cf_io;

use worker::*;

use crate::cf_io::CfFihIo;
use crate::stores::vectorize::CfVectorizeStore;
use nex::FihStorage;
use nex::io::AsyncFileIo;
use nexus_model::{
    AsyncIntentCapable, AsyncStorageRead, Content, Fact, FihHash, Intent,
};
use std::sync::OnceLock;

// ── CF clock ────────────────────────────────────────────────────────────

struct CfClock;
impl nexus_model::Now for CfClock {
    fn now_nanos(&self) -> u64 {
        worker::Date::now().as_millis() * 1_000_000
    }
    fn now_secs(&self) -> u64 {
        worker::Date::now().as_millis() / 1_000
    }
}

// ── Static storage (prod + test) ────────────────────────────────────────

struct SyncStore(OnceLock<FihStorage<CfFihIo>>);
unsafe impl Sync for SyncStore {}
static PROD_STORE: SyncStore = SyncStore(OnceLock::new());
static TEST_STORE: SyncStore = SyncStore(OnceLock::new());
static PROD_FIH_BUCKET: OnceLock<worker::Bucket> = OnceLock::new();
static TEST_FIH_BUCKET: OnceLock<worker::Bucket> = OnceLock::new();

struct SyncVectorizeStore(OnceLock<CfVectorizeStore>);
unsafe impl Sync for SyncVectorizeStore {}
static PROD_VECTORIZE: SyncVectorizeStore = SyncVectorizeStore(OnceLock::new());
static TEST_VECTORIZE: SyncVectorizeStore = SyncVectorizeStore(OnceLock::new());

fn store(is_test: bool) -> &'static FihStorage<CfFihIo> {
    if is_test { TEST_STORE.0.get().expect("TEST FihStorage not initialized") }
    else { PROD_STORE.0.get().expect("PROD FihStorage not initialized") }
}

fn vectorize_store(is_test: bool) -> Option<&'static CfVectorizeStore> {
    if is_test { TEST_VECTORIZE.0.get() }
    else { PROD_VECTORIZE.0.get() }
}

fn init_stores(env: &worker::Env, prod_bucket: worker::Bucket, test_bucket: worker::Bucket) {
    let _ = PROD_FIH_BUCKET.set(prod_bucket.clone());
    let _ = TEST_FIH_BUCKET.set(test_bucket.clone());
    PROD_STORE.0.get_or_init(|| {
        let s = FihStorage::with_clock(CfFihIo::new(prod_bucket), "cf-nexus", Box::new(CfClock));
        s.register_semantic_store(Box::new(crate::stores::bm25::InMemoryBm25::new()));
        s
    });
    TEST_STORE.0.get_or_init(|| {
        let s = FihStorage::with_clock(CfFihIo::new(test_bucket), "cf-nexus-test", Box::new(CfClock));
        s.register_semantic_store(Box::new(crate::stores::bm25::InMemoryBm25::new()));
        s
    });
    let _ = PROD_VECTORIZE.0.set(CfVectorizeStore::new(env.clone()));
    let _ = TEST_VECTORIZE.0.set(CfVectorizeStore::new(env.clone()));
}

fn split_test_prefix(path: &str) -> (bool, &str) {
    if path.len() >= 6 && path.as_bytes()[0] == b'/' && path.as_bytes()[1] == b't'
        && path.as_bytes()[2] == b'e' && path.as_bytes()[3] == b's' && path.as_bytes()[4] == b't'
    {
        let rest = &path[5..];
        if rest.is_empty() || rest == "/" { (true, "/") }
        else { (true, rest) }
    } else { (false, path) }
}

pub fn qv(q: &[(String, String)], k: &str) -> String {
    q.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default()
}

pub async fn handle_path<I: AsyncFileIo>(s: &FihStorage<I>, path: &str, q: &[(String, String)]) -> (u16, String, String) {
    match path {
        "/" => (200, "text/plain".into(), "nexus-cf".into()),
        "/fact" => {
            let fact = Fact {
                id: FihHash::from_hex(&qv(q, "id")),
                origin: qv(q, "origin"),
                content: Content { mime_type: "text/plain".into(), data: qv(q, "content").into_bytes() },
                creator: qv(q, "creator"),
            };
            let hash = match nexus_model::AsyncFactCapable::submit_fact(s, &fact).await {
                Ok(h) => h,
                Err(e) => return (500, "application/json".into(), serde_json::json!({"error": format!("submit_fact: {:?}", e)}).to_string()),
            };
            // Flush immediately for single-fact endpoints (caller expects durability).
            if let Err(e) = s.flush_pending().await {
                return (500, "application/json".into(), serde_json::json!({"error": format!("flush: {}", e)}).to_string());
            }
            (200, "application/json".into(), serde_json::json!({"id": hash.to_string()}).to_string())
        }
        "/intent" => {
            let intent = Intent {
                id: FihHash::from_hex(&qv(q, "id")),
                from_facts: qv(q, "from").split(',').filter(|s| !s.is_empty()).map(FihHash::from_hex).collect(),
                description: qv(q, "desc"), creator: qv(q, "creator"),
                worker: None, to_fact_id: None, last_heartbeat_at: None, created_at: None,
                is_concluded: false, concluded_at: None,
            };
            let hash = match s.submit_intent(&intent).await {
                Ok(h) => h,
                Err(e) => return (500, "application/json".into(), serde_json::json!({"error": format!("submit_intent: {:?}", e)}).to_string()),
            };
            // Flush immediately for single-intent endpoints (caller expects durability).
            if let Err(e) = s.flush_pending().await {
                return (500, "application/json".into(), serde_json::json!({"error": format!("flush: {}", e)}).to_string());
            }
            (200, "application/json".into(), serde_json::json!({"id": hash.to_string()}).to_string())
        }
        "/claim" => match s.claim_intent(&qv(q, "id"), &qv(q, "agent")).await {
            Ok(()) => (200, "application/json".into(), serde_json::json!({"status":"claimed"}).to_string()),
            Err(e) => {
                let msg = format!("{:?}", e);
                let code = if msg.contains("Conflict") { 409 } else if msg.contains("not found") { 404 } else { 500 };
                (code, "application/json".into(), serde_json::json!({"error": msg}).to_string())
            }
        },
        "/conclude" => match s.conclude_intent(&qv(q, "id"), &qv(q, "result")).await {
            Ok(fact) => (200, "application/json".into(), serde_json::json!({"status":"concluded","fact_id": fact.id.to_string()}).to_string()),
            Err(e) => (500, "application/json".into(), serde_json::json!({"error": format!("{:?}", e)}).to_string()),
        },
        "/state" => {
            let state = s.read_state().await;
            (200, "application/json".into(), serde_json::to_string(&state).unwrap_or_else(|_| "{}".into()))
        }
        "/flush" => match s.flush_pending().await {
            Ok(()) => (200, "application/json".into(), serde_json::json!({"status":"ok"}).to_string()),
            Err(e) => (500, "application/json".into(), serde_json::json!({"error": format!("flush: {}", e)}).to_string()),
        },
        "/rebuild" => match s.rebuild_cache().await {
            Ok(()) => (200, "application/json".into(), serde_json::json!({"status":"ok"}).to_string()),
            Err(e) => (500, "application/json".into(), serde_json::json!({"error": format!("rebuild: {}", e)}).to_string()),
        },
        _ => (404, "application/json".into(), serde_json::json!({"error": "not found"}).to_string()),
    }
}

// ── Document helpers ─────────────────────────────────────────────────

pub async fn ingest_document<I: AsyncFileIo>(s: &FihStorage<I>, text: &str, origin: &str) -> Result<String, String> {
    let paragraphs: Vec<&str> = text.split('\n').map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    if paragraphs.is_empty() { return Err("empty document".into()); }
    let mut last_id = String::new();
    for (i, para) in paragraphs.iter().enumerate() {
        let para_id = format!("f_{}_{}", sanitize_id(origin), i);
        let fact = Fact {
            id: FihHash::from_hex(&para_id),
            origin: format!("document:{}", origin),
            content: Content { mime_type: "text/plain".into(), data: para.as_bytes().to_vec() },
            creator: "ingestion-agent".into(),
        };
        // Async FactCapable: enqueue in pending buffer only (no R2 PUT per paragraph).
        // In-memory indices (BM25, FihCoord) are updated immediately for subsequent
        // semantic_search calls within the same request.
        nexus_model::AsyncFactCapable::submit_fact(s, &fact).await
            .map_err(|e| format!("submit para {i}: {e:?}"))?;
        last_id = para_id;
    }
    // Flush all pending writes to R2 in a single apply_batch call.
    s.flush_pending().await.map_err(|e| format!("flush: {e}"))?;
    Ok(last_id)
}

fn sanitize_id(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

pub async fn ingest_all_from_io<I: AsyncFileIo, D: AsyncFileIo>(s: &FihStorage<I>, docs: &D, prefix: &str) -> (usize, Vec<String>) {
    let mut total = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let keys = match docs.list(prefix).await {
        Ok(keys) => keys,
        Err(e) => { errors.push(format!("list '{prefix}': {e}")); return (total, errors); }
    };
    for key in &keys {
        if !key.ends_with(".llms.md") { continue; }
        let data = match docs.read(key).await {
            Ok(Some(d)) => d,
            Ok(None) => { errors.push(format!("{key}: empty")); continue; }
            Err(e) => { errors.push(format!("{key}: {e}")); continue; }
        };
        let text = match String::from_utf8(data) {
            Ok(t) => t,
            Err(_) => { errors.push(format!("{key}: not UTF-8")); continue; }
        };
        let origin = key.trim_end_matches(".llms.md").trim_start_matches("_llms/").to_string();
        match ingest_document(s, &text, &origin).await {
            Ok(_) => total += 1,
            Err(e) => errors.push(format!("{key}: {e}")),
        }
    }
    (total, errors)
}

// ── Entrypoint ───────────────────────────────────────────────────────

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    init_stores(
        &env,
        env.bucket("FIH_R2").expect("FIH_R2 bucket binding required"),
        env.bucket("FIH_R2_TEST").expect("FIH_R2_TEST bucket binding required"),
    );

    let docs_bucket = env.bucket("DOCS_R2").ok();
    let docs_bucket_test = env.bucket("DOCS_R2_TEST").ok();

    let url = req.url()?;
    let path = url.path().to_string();
    let q: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let (is_test, path_stripped) = split_test_prefix(&path);

    let s = store(is_test);
    let docs = if is_test { docs_bucket_test.as_ref() } else { docs_bucket.as_ref() };

    match path_stripped {
        "/" | "/fact" | "/intent" | "/claim" | "/conclude" | "/state" | "/flush" | "/rebuild" => {
            let (code, _content_type, body) = handle_path(s, path_stripped, &q).await;
            Ok(Response::from_bytes(body.into_bytes())?.with_status(code))
        }

        "/version" => Response::ok("3"),

        "/ingest" => {
            let text = qv(&q, "text");
            let origin = qv(&q, "origin");
            if text.is_empty() { return Response::error("missing 'text' parameter", 400); }
            let origin = if origin.is_empty() { "ingest".into() } else { origin };
            match ingest_document(s, &text, &origin).await {
                Ok(id) => {
                    // Sync semantic stores from memory to Vectorize index (async).
                    if let Some(vs) = vectorize_store(is_test) {
                        if let Err(e) = vs.sync_to_vectorize().await {
                            worker::console_log!("vectorize sync error: {e}");
                        }
                    }
                    Response::from_json(&serde_json::json!({"status":"ingested","id": id}))
                }
                Err(e) => Response::error(e, 500),
            }
        }

        "/search" => {
            let query_text = qv(&q, "q");
            if query_text.is_empty() { return Response::error("missing 'q' parameter", 400); }

            let results = match vectorize_store(is_test) {
                Some(vs) => match vs.search_vectorize_async(&query_text, 10).await {
                    Ok(r) if !r.is_empty() => r,
                    _ => {
                        let query = crate::cf_io::TextQuery { text: query_text };
                        match s.semantic_search(&query, 10).await {
                            Ok(r) => r,
                            Err(e) => return Response::error(format!("search: {e}"), 500),
                        }
                    }
                },
                None => {
                    let query = crate::cf_io::TextQuery { text: query_text };
                    match s.semantic_search(&query, 10).await {
                        Ok(r) => r,
                        Err(e) => return Response::error(format!("search: {e}"), 500),
                    }
                }
            };

            let items: Vec<serde_json::Value> = results.iter().map(|(idx, score)| {
                serde_json::json!({"index": idx, "score": score, "id": s.resolve_semantic_idx(*idx)})
            }).collect();
            Response::from_json(&serde_json::json!({"results": items}))
        }

        "/debug/list-docs" => {
            match docs {
                Some(b) => {
                    let objects = match b.list().execute().await {
                        Ok(o) => o,
                        Err(e) => return Response::error(format!("list error: {e}"), 500),
                    };
                    let keys: Vec<String> = objects.objects().iter().map(|o| o.key()).collect();
                    Response::from_json(&serde_json::json!({"count": keys.len(), "keys": keys, "truncated": objects.truncated()}))
                }
                None => Response::error("DOCS_R2 not bound", 500),
            }
        }

        "/ingest-one" => {
            let key = qv(&q, "key");
            if key.is_empty() { return Response::error("missing 'key' parameter", 400); }
            if !key.ends_with(".llms.md") { return Response::error("not a .llms.md file", 400); }

            let bucket = match docs { Some(b) => b, None => return Response::error("DOCS_R2 not bound", 500) };

            let obj = match bucket.get(&key).execute().await {
                Ok(Some(o)) => o,
                Ok(None) => return Response::error("not found", 404),
                Err(e) => return Response::error(format!("R2 get: {e}"), 500),
            };
            let data = match obj.body() {
                Some(body) => body.bytes().await.map_err(|e| format!("read body: {e}"))?.to_vec(),
                None => return Response::error("no body", 500),
            };
            let text = match String::from_utf8(data) { Ok(t) => t, Err(_) => return Response::error("not UTF-8", 500) };
            let origin = key.trim_end_matches(".llms.md").trim_start_matches("_llms/").to_string();

            // ingest_document now enqueues writes across paragraphs and flushes
            // all pending writes in a single apply_batch call (no individual R2 PUTs).
            match crate::ingest_document(s, &text, &origin).await {
                Ok(id) => {
                    // Sync semantic stores from memory to Vectorize index (async).
                    if let Some(vs) = vectorize_store(is_test) {
                        if let Err(e) = vs.sync_to_vectorize().await {
                            worker::console_log!("vectorize sync error: {e}");
                        }
                    }
                    Response::from_json(&serde_json::json!({"status":"ingested","id": id}))
                }
                Err(e) => Response::error(e, 500),
            }
        }

        _ => Response::error("not found", 404),
    }
}
