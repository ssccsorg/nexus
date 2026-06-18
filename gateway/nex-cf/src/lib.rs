// gateway/nex-cf — Consumes FihStorage<CfFihIo> via async traits.
// No block_on. No locks. Pure async over R2.

pub mod stores;
pub mod batch_io;
pub mod cf_io;

use worker::*;

use crate::cf_io::CfFihIo;
use nex::FihStorage;
use nex::io::AsyncFileIo;
use nexus_model::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead, Content, Fact, FihHash, Intent,
};
use std::sync::OnceLock;

// ── CF clock: real timestamps via worker::Date::now() ────────────────

struct CfClock;
impl nexus_model::Now for CfClock {
    fn now_nanos(&self) -> u64 {
        worker::Date::now().as_millis() * 1_000_000
    }
    fn now_secs(&self) -> u64 {
        worker::Date::now().as_millis() / 1_000
    }
}

// ── Static storage (생산 + 테스트) ──────────────────────────────────

/// SAFETY: FihStorage contains RefCell (not Sync), but in the WASM
/// isolate there is no true parallelism — only async concurrency on
/// one thread. Wrapping OnceLock<FihStorage<CfFihIo>> in a newtype to
/// safely implement Sync is sound on wasm32.
struct SyncStore(OnceLock<FihStorage<CfFihIo>>);
unsafe impl Sync for SyncStore {}

static PROD_STORE: SyncStore = SyncStore(OnceLock::new());
static TEST_STORE: SyncStore = SyncStore(OnceLock::new());

fn store(is_test: bool) -> &'static FihStorage<CfFihIo> {
    if is_test {
        TEST_STORE.0.get().expect("TEST FihStorage not initialized")
    } else {
        PROD_STORE.0.get().expect("PROD FihStorage not initialized")
    }
}

fn init_stores(prod_bucket: worker::Bucket, test_bucket: worker::Bucket) {
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
}

/// `/test/...` → true, 나머지 path 반환
fn split_test_prefix(path: &str) -> (bool, &str) {
    if path.len() >= 6 && path.as_bytes()[0] == b'/' && path.as_bytes()[1] == b't'
        && path.as_bytes()[2] == b'e' && path.as_bytes()[3] == b's' && path.as_bytes()[4] == b't'
    {
        let rest = &path[5..];
        if rest.is_empty() || rest == "/" {
            (true, "/")
        } else {
            (true, rest)
        }
    } else {
        (false, path)
    }
}

pub fn qv(q: &[(String, String)], k: &str) -> String {
    q.iter()
        .find(|(key, _)| key == k)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// Generic request router — shared between CF Worker and mock server.
pub async fn handle_path<I: AsyncFileIo>(
    s: &FihStorage<I>,
    path: &str,
    q: &[(String, String)],
) -> (u16, String, String) {
    match path {
        "/" => (200, "text/plain".into(), "nexus-cf".into()),

        "/fact" => {
            let fact = Fact {
                id: FihHash::from_hex(&qv(q, "id")),
                origin: qv(q, "origin"),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: qv(q, "content").into_bytes(),
                },
                creator: qv(q, "creator"),
            };
            match s.submit_fact(&fact).await {
                Ok(hash) => (200, "application/json".into(), serde_json::json!({"id": hash.to_string()}).to_string()),
                Err(e) => (500, "application/json".into(), serde_json::json!({"error": format!("submit_fact: {:?}", e)}).to_string()),
            }
        }

        "/intent" => {
            let intent = Intent {
                id: FihHash::from_hex(&qv(q, "id")),
                from_facts: qv(q, "from").split(',').filter(|s| !s.is_empty()).map(FihHash::from_hex).collect(),
                description: qv(q, "desc"),
                creator: qv(q, "creator"),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            };
            match s.submit_intent(&intent).await {
                Ok(hash) => (200, "application/json".into(), serde_json::json!({"id": hash.to_string()}).to_string()),
                Err(e) => (500, "application/json".into(), serde_json::json!({"error": format!("submit_intent: {:?}", e)}).to_string()),
            }
        }

        "/claim" => match s.claim_intent(&qv(q, "id"), &qv(q, "agent")).await {
            Ok(()) => (200, "application/json".into(), serde_json::json!({"status":"claimed"}).to_string()),
            Err(e) => {
                let msg = format!("{:?}", e);
                let code = if msg.contains("Conflict") { 409 }
                    else if msg.contains("not found") { 404 }
                    else { 500 };
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

/// Split document text into paragraphs and submit each as a Fact.
pub async fn ingest_document<I: AsyncFileIo>(
    s: &FihStorage<I>,
    text: &str,
    origin: &str,
) -> Result<String, String> {
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
        s.submit_fact(&fact)
            .await
            .map_err(|e| format!("submit para {i}: {e:?}"))?;
        last_id = para_id;
    }
    Ok(last_id)
}

fn sanitize_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

/// Scan an `AsyncFileIo`-backed document store for `.llms.md` files
/// and ingest each one as Facts into the given FihStorage.
pub async fn ingest_all_from_io<I: AsyncFileIo, D: AsyncFileIo>(
    s: &FihStorage<I>,
    docs: &D,
    prefix: &str,
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
        if !key.ends_with(".llms.md") {
            continue;
        }
        let data = match docs.read(key).await {
            Ok(Some(d)) => d,
            Ok(None) => { errors.push(format!("{key}: empty")); continue; }
            Err(e) => { errors.push(format!("{key}: {e}")); continue; }
        };
        let text = match String::from_utf8(data) {
            Ok(t) => t,
            Err(_) => { errors.push(format!("{key}: not UTF-8")); continue; }
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

// ── Entrypoint ───────────────────────────────────────────────────────

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    init_stores(
        env.bucket("FIH_R2")
            .expect("FIH_R2 bucket binding required"),
        env.bucket("FIH_R2_TEST")
            .expect("FIH_R2_TEST bucket binding required"),
    );

    let docs_bucket = env.bucket("DOCS_R2").ok();
    let docs_bucket_test = env.bucket("DOCS_R2_TEST").ok();

    let url = req.url()?;
    let path = url.path().to_string();
    let q: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let (is_test, path_stripped) = split_test_prefix(&path);

    let s = store(is_test);

    match path_stripped {
        "/" | "/fact" | "/intent" | "/claim" | "/conclude" | "/state" | "/flush" | "/rebuild" => {
            let (code, _content_type, body) = handle_path(s, path_stripped, &q).await;
            Ok(Response::from_bytes(body.into_bytes())?.with_status(code))
        }

        "/version" => Response::ok("3"),

        "/ingest" => {
            let text = qv(&q, "text");
            let origin = qv(&q, "origin");
            if text.is_empty() {
                return Response::error("missing 'text' parameter", 400);
            }
            let origin = if origin.is_empty() { "ingest".into() } else { origin };
            match ingest_document(s, &text, &origin).await {
                Ok(id) => Response::from_json(&serde_json::json!({"status":"ingested","id": id})),
                Err(e) => Response::error(e, 500),
            }
        }

        "/search" => {
            let query_text = qv(&q, "q");
            if query_text.is_empty() {
                return Response::error("missing 'q' parameter", 400);
            }
            let query = crate::cf_io::TextQuery { text: query_text };
            match s.semantic_search(&query, 10) {
                Ok(results) => {
                    let items: Vec<serde_json::Value> = results.iter().map(|(idx, score)| {
                        serde_json::json!({"index": idx, "score": score, "id": s.resolve_semantic_idx(*idx)})
                    }).collect();
                    Response::from_json(&serde_json::json!({"results": items}))
                }
                Err(e) => Response::error(format!("search: {e}"), 500),
            }
        }

        "/debug/list-docs" => {
            let bucket = if is_test { docs_bucket_test.as_ref() } else { docs_bucket.as_ref() };
            match bucket {
                Some(b) => {
                    let objects = match b.list().execute().await {
                        Ok(o) => o,
                        Err(e) => return Response::error(format!("list error: {e}"), 500),
                    };
                    let keys: Vec<String> = objects.objects().iter().map(|o| o.key()).collect();
                    Response::from_json(&serde_json::json!({
                        "count": keys.len(),
                        "keys": keys,
                        "truncated": objects.truncated(),
                    }))
                }
                None => Response::error("DOCS_R2 not bound", 500),
            }
        }
        "/ingest-all" => {
            let bucket = if is_test { docs_bucket_test.as_ref() } else { docs_bucket.as_ref() };
            let bucket = match bucket {
                Some(b) => b,
                None => return Response::error("DOCS_R2 bucket not bound", 500),
            };
            let prefix = qv(&q, "prefix");
            let filter_suffix = qv(&q, "suffix");
            let filter_suffix = if filter_suffix.is_empty() { ".llms.md".into() } else { filter_suffix };

            // R2 문서 목록 조회
            let objects = match bucket.list().prefix(&prefix).execute().await {
                Ok(o) => o,
                Err(e) => return Response::error(format!("list docs: {e}"), 500),
            };
            let keys: Vec<String> = objects.objects().iter().map(|o| o.key()).collect();

            // BatchIo로 감싼 storage — submit_fact에서 R2 쓰기를 enqueue만 함
            let batch_io = crate::batch_io::BatchIo::new(CfFihIo::new(
                env.bucket("FIH_R2").expect("FIH_R2 required"),
            ));
            let batch_storage = nex::FihStorage::with_clock(
                batch_io,
                s.project_id(),
                Box::new(CfClock),
            );
            batch_storage.register_semantic_store(Box::new(crate::stores::bm25::InMemoryBm25::new()));

            let mut total = 0usize;
            let mut errors: Vec<String> = Vec::new();

            for key in &keys {
                if !key.ends_with(&filter_suffix) {
                    continue;
                }
                let obj = match bucket.get(key).execute().await {
                    Ok(Some(o)) => o,
                    Ok(None) => { errors.push(format!("{key}: empty")); continue; }
                    Err(e) => { errors.push(format!("{key}: {e}")); continue; }
                };
                let data = match obj.body() {
                    Some(b) => match b.bytes().await {
                        Ok(bytes) => bytes.to_vec(),
                        Err(e) => { errors.push(format!("{key}: {e}")); continue; }
                    },
                    None => { errors.push(format!("{key}: no body")); continue; }
                };
                let text = match String::from_utf8(data) {
                    Ok(t) => t,
                    Err(_) => { errors.push(format!("{key}: not UTF-8")); continue; }
                };
                let origin = key
                    .trim_end_matches(".llms.md")
                    .trim_start_matches("_llms/")
                    .to_string();
                match crate::ingest_document(&batch_storage, &text, &origin).await {
                    Ok(_) => total += 1,
                    Err(e) => errors.push(format!("{key}: {e}")),
                }
            }

            // 모든 pending R2 쓰기를 한 번에 flush (BatchIo가 apply_batch에서 자체 pending도 처리)
            if let Err(e) = batch_storage.flush_pending().await {
                errors.push(format!("flush: {e}"));
            }

            Response::from_json(&serde_json::json!({
                "status": "ok", "total": total, "errors": errors,
            }))
        }

        _ => Response::error("not found", 404),
    }
}
