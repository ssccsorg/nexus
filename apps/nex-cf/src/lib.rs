// apps/nex-cf — Consumes FihStorage<CfFihIo> via async traits.
// All state lives inside a single Durable Object (NexusCfDO) so that
// in-memory indices (BM25, FihCoord) are consistent across requests.
// The #[event(fetch)] handler is a thin proxy that forwards every
// request to the DO stub.

pub mod batch_io;
pub mod cf_io;
pub mod stores;

use std::cell::RefCell;

use worker::*;

use crate::cf_io::CfFihIo;
use crate::stores::vectorize::CfVectorizeStore;
use nex::FihStorage;
use nex::io::AsyncFileIo;
use nexus_model::{AsyncIntentCapable, AsyncStorageRead, Content, Fact, FihHash, Intent};

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

// ── Stores holder ──────────────────────────────────────────────────────

struct CfStores {
    prod: FihStorage<CfFihIo>,
    test: FihStorage<CfFihIo>,
}

fn build_store(bucket: worker::Bucket, _env: &worker::Env, project: &str) -> FihStorage<CfFihIo> {
    let s = FihStorage::with_clock(CfFihIo::new(bucket), project, Box::new(CfClock));
    s.register_semantic_store(Box::new(crate::stores::bm25::InMemoryBm25::new()));
    s.register_semantic_store(Box::new(CfVectorizeStore::with_embedder(Box::new(
        crate::stores::vectorize::LocalTfidfEmbedder,
    ))));
    s
}

fn split_test_prefix(path: &str) -> (bool, &str) {
    if path.len() >= 6
        && path.as_bytes()[0] == b'/'
        && path.as_bytes()[1] == b't'
        && path.as_bytes()[2] == b'e'
        && path.as_bytes()[3] == b's'
        && path.as_bytes()[4] == b't'
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

// ── NexusCfDO — Durable Object ─────────────────────────────────────────

/// Durable Object that holds the full in-memory FihStorage state.
///
/// Because CF Durable Objects process requests sequentially (per instance),
/// all in-memory indices (BM25, FihCoord) remain consistent without locks.
/// The single-instance routing (via `id_from_name("default")`) solves the
/// multi-instance problem that plagues stateless Workers.
#[durable_object]
pub struct NexusCfDO {
    /// Stores are initialized once in the constructor, then only borrowed
    /// immutably for requests. RefCell is safe because DOs are single-threaded.
    stores: RefCell<Option<CfStores>>,
    /// Retained for late-bound lookups (DOCS_R2, vectorize sync).
    env: Env,
    docs_bucket: Option<worker::Bucket>,
    docs_bucket_test: Option<worker::Bucket>,
}

impl DurableObject for NexusCfDO {
    fn new(_state: State, env: Env) -> Self {
        let prod_bucket = env
            .bucket("FIH_R2")
            .expect("FIH_R2 bucket binding required");
        let test_bucket = env
            .bucket("FIH_R2_TEST")
            .expect("FIH_R2_TEST bucket binding required");
        let docs_bucket = env.bucket("DOCS_R2").ok();
        let docs_bucket_test = env.bucket("DOCS_R2_TEST").ok();

        let prod = build_store(prod_bucket, &env, "cf-nexus");
        let test = build_store(test_bucket, &env, "cf-nexus-test");

        // No background hydration here: `new()` is synchronous.
        // Cold-start recovery can be triggered with `?rebuild=1` on search
        // requests, matching the pre-DO behavior.

        Self {
            stores: RefCell::new(Some(CfStores { prod, test })),
            env,
            docs_bucket,
            docs_bucket_test,
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path().to_string();
        let q: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let (is_test, path_stripped) = split_test_prefix(&path);

        // Borrow the appropriate store. RefCell is safe in DO (single-threaded).
        let stores_opt = self.stores.borrow();
        let stores = stores_opt
            .as_ref()
            .expect("NexusCfDO stores not initialized");
        let s: &FihStorage<CfFihIo> = if is_test { &stores.test } else { &stores.prod };

        // Cold-start recovery: if fact_store is empty, rebuild from R2.
        // Reads a consolidated snapshot file (_snapshot/facts.bin) in a
        // single R2 GET, avoiding the 30s DO timeout from N sequential
        // gets. If no snapshot exists, this is a fresh store — proceed
        // with empty caches; first ingest will create the snapshot.
        if s.fact_store.is_empty()
            && path_stripped != "/ingest-one"
            && path_stripped != "/ingest"
            && let Ok(Some(bytes)) = s.io.read("_snapshot/facts.bin").await
            && let Ok(entry) = postcard::from_bytes::<nex::storage::core::ChainEntry>(&bytes)
        {
            let facts: Vec<(String, _)> =
                entry.facts.into_iter().map(|r| (r.id.clone(), r)).collect();
            let intents: Vec<(String, _)> = entry
                .intents
                .into_iter()
                .map(|r| (r.id.clone(), r))
                .collect();
            s.fact_store.replace_from(facts);
            s.intent_store.replace_from(intents);
            s.rebuild_coord();
        }
        // rebuild_semantic is intentionally skipped:
        //   - submit_fact already indexed facts into semantic stores
        //   - FihCoord::clear() preserves by_semantic across index rebuilds
        //   - New facts submitted after cold start will be auto-indexed.
        let docs = if is_test {
            self.docs_bucket_test.as_ref()
        } else {
            self.docs_bucket.as_ref()
        };

        match path_stripped {
            "/" | "/fact" | "/intent" | "/claim" | "/conclude" | "/state" | "/flush"
            | "/rebuild" => {
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
                let origin = if origin.is_empty() {
                    "ingest".into()
                } else {
                    origin
                };
                match ingest_document(s, &text, &origin).await {
                    Ok(id) => {
                        let vs = CfVectorizeStore::new(self.env.clone());
                        vs.sync_to_vectorize().await.ok();
                        Response::from_json(&serde_json::json!({
                            "status": "ingested",
                            "id": id
                        }))
                    }
                    Err(e) => Response::error(e, 500),
                }
            }

            "/search" => {
                let query_text = qv(&q, "q");
                if query_text.is_empty() {
                    return Response::error("missing 'q' parameter", 400);
                }
                // Rebuild from R2 if requested (cold-start recovery).
                if qv(&q, "rebuild") == "1" {
                    s.rebuild_cache().await.ok();
                    s.rebuild_semantic().await.ok();
                }
                let query = crate::cf_io::TextQuery { text: query_text };
                let results = match s.semantic_search(&query, 10).await {
                    Ok(r) => r,
                    Err(e) => return Response::error(format!("search: {e}"), 500),
                };
                let items: Vec<serde_json::Value> = results
                    .iter()
                    .map(|(idx, score)| {
                        serde_json::json!({
                            "index": idx,
                            "score": score,
                            "id": s.resolve_semantic_idx(*idx)
                        })
                    })
                    .collect();
                Response::from_json(&serde_json::json!({"results": items}))
            }

            "/debug/stores" => {
                let stores_list = s.semantic_stores();
                let count = stores_list.len();
                worker::console_log!("semantic_stores count: {}", count);
                drop(stores_list);
                Response::ok(format!(
                    "stores={} fact_store={}",
                    count,
                    s.fact_store.len()
                ))
            }

            "/debug/ingest-search" => {
                let text = qv(&q, "text");
                let query = qv(&q, "q");
                if text.is_empty() {
                    return Response::error("missing text", 400);
                }
                if query.is_empty() {
                    return Response::error("missing q", 400);
                }
                match crate::ingest_document(s, &text, "debug").await {
                    Ok(id) => {
                        let search_query = crate::cf_io::TextQuery { text: query };
                        match s.semantic_search(&search_query, 10).await {
                            Ok(results) => {
                                let items: Vec<serde_json::Value> = results
                                    .iter()
                                    .map(|(idx, score)| {
                                        serde_json::json!({"index": idx, "score": score})
                                    })
                                    .collect();
                                Response::from_json(&serde_json::json!({
                                    "ingested": id,
                                    "fact_store": s.fact_store.len(),
                                    "results": items
                                }))
                            }
                            Err(e) => Response::error(format!("search error: {}", e), 500),
                        }
                    }
                    Err(e) => Response::error(e, 500),
                }
            }

            "/debug/list-docs" => match docs {
                Some(b) => {
                    let objects = match b.list().execute().await {
                        Ok(o) => o,
                        Err(e) => return Response::error(format!("list error: {e}"), 500),
                    };
                    let keys: Vec<String> = objects.objects().iter().map(|o| o.key()).collect();
                    Response::from_json(&serde_json::json!({
                        "count": keys.len(),
                        "keys": keys,
                        "truncated": objects.truncated()
                    }))
                }
                None => Response::error("DOCS_R2 not bound", 500),
            },

            "/ingest-all" => {
                let bucket = match docs {
                    Some(b) => b,
                    None => return Response::error("DOCS_R2 not bound", 500),
                };
                let prefix = qv(&q, "prefix");
                let prefix = if prefix.is_empty() { "_llms/" } else { &prefix };
                let max: usize = qv(&q, "max").parse().unwrap_or(5);
                let mut total: usize = 0;
                let mut errors: Vec<String> = Vec::new();
                let list_result = match bucket.list().prefix(prefix).execute().await {
                    Ok(o) => o,
                    Err(e) => return Response::error(format!("list error: {e}"), 500),
                };
                let mut objects = list_result;
                loop {
                    for obj in objects.objects() {
                        let key = obj.key();
                        if !key.ends_with(".llms.md") {
                            continue;
                        }
                        let obj_get = match bucket.get(&key).execute().await {
                            Ok(Some(o)) => o,
                            Ok(None) => {
                                errors.push(format!("{key}: not found"));
                                continue;
                            }
                            Err(e) => {
                                errors.push(format!("{key}: get error: {e}"));
                                continue;
                            }
                        };
                        let data = match obj_get.body() {
                            Some(body) => match body.bytes().await {
                                Ok(b) => b.to_vec(),
                                Err(e) => {
                                    errors.push(format!("{key}: read body: {e}"));
                                    continue;
                                }
                            },
                            None => {
                                errors.push(format!("{key}: no body"));
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
                            .trim_start_matches("_llms/");
                        match crate::ingest_document(s, &text, origin).await {
                            Ok(_) => {
                                total += 1;
                                if total >= max {
                                    break;
                                }
                            }
                            Err(e) => errors.push(format!("{key}: {e}")),
                        }
                    }
                    if total >= max {
                        break;
                    }
                    if !objects.truncated() {
                        break;
                    }
                    let cursor = match objects.cursor() {
                        Some(c) => c,
                        None => break,
                    };
                    objects = match bucket.list().prefix(prefix).cursor(cursor).execute().await {
                        Ok(o) => o,
                        Err(e) => {
                            errors.push(format!("list next page: {e}"));
                            break;
                        }
                    };
                }
                Response::from_json(&serde_json::json!({
                    "ingested": total,
                    "errors": errors
                }))
            }

            "/ingest-one" => {
                let key = qv(&q, "key");
                if key.is_empty() {
                    return Response::error("missing 'key' parameter", 400);
                }
                if !key.ends_with(".llms.md") {
                    return Response::error("not a .llms.md file", 400);
                }
                let bucket = match docs {
                    Some(b) => b,
                    None => return Response::error("DOCS_R2 not bound", 500),
                };
                let obj = match bucket.get(&key).execute().await {
                    Ok(Some(o)) => o,
                    Ok(None) => return Response::error("not found", 404),
                    Err(e) => return Response::error(format!("R2 get: {e}"), 500),
                };
                let data = match obj.body() {
                    Some(body) => body
                        .bytes()
                        .await
                        .map_err(|e| format!("read body: {e}"))?
                        .to_vec(),
                    None => return Response::error("no body", 500),
                };
                let text = match String::from_utf8(data) {
                    Ok(t) => t,
                    Err(_) => return Response::error("not UTF-8", 500),
                };
                let origin = key
                    .trim_end_matches(".llms.md")
                    .trim_start_matches("_llms/")
                    .to_string();

                let do_flush = qv(&q, "flush") != "0";
                if !do_flush {
                    let paragraphs: Vec<&str> = text
                        .split('\n')
                        .map(|p| p.trim())
                        .filter(|p| !p.is_empty())
                        .collect();
                    let mut last_id = String::new();
                    for (i, para) in paragraphs.iter().enumerate() {
                        let para_id = format!("f_{}_{}", sanitize_id(&origin), i);
                        let fact = Fact {
                            id: FihHash::from_hex(&para_id),
                            origin: format!("document:{}", origin),
                            content: Content {
                                mime_type: "text/plain".into(),
                                data: para.as_bytes().to_vec(),
                            },
                            creator: "ingestion-agent".into(),
                        };
                        nexus_model::AsyncFactCapable::submit_fact(s, &fact)
                            .await
                            .map_err(|e| format!("submit para {i}: {e:?}"))?;
                        last_id = para_id;
                    }
                    return Response::from_json(&serde_json::json!({
                        "status": "indexed",
                        "id": last_id
                    }));
                }
                match crate::ingest_document(s, &text, &origin).await {
                    Ok(id) => {
                        let vs = CfVectorizeStore::new(self.env.clone());
                        vs.sync_to_vectorize().await.ok();
                        Response::from_json(&serde_json::json!({
                            "status": "ingested",
                            "id": id
                        }))
                    }
                    Err(e) => Response::error(e, 500),
                }
            }

            _ => Response::error("not found", 404),
        }
    }
}

// ── Entrypoint (thin proxy) ─────────────────────────────────────────────

/// Thin fetch handler that forwards all requests to the single DO instance.
///
/// Every request creates or gets the DO stub via `id_from_name("default")`
/// and delegates to `stub.fetch_request(req)`. This ensures that all
/// requests hit the same DO instance, keeping in-memory indices consistent.
#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let namespace = env.durable_object("STORAGE_DO")?;
    let stub = namespace.id_from_name("default")?.get_stub()?;
    stub.fetch_with_request(req).await
}

// ── Path handlers (called from DO) ──────────────────────────────────────

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
            let hash = match nexus_model::AsyncFactCapable::submit_fact(s, &fact).await {
                Ok(h) => h,
                Err(e) => {
                    return (
                        500,
                        "application/json".into(),
                        serde_json::json!({"error": format!("submit_fact: {:?}", e)}).to_string(),
                    );
                }
            };
            // Flush immediately for single-fact endpoints (caller expects durability).
            if let Err(e) = s.flush_pending().await {
                return (
                    500,
                    "application/json".into(),
                    serde_json::json!({"error": format!("flush: {}", e)}).to_string(),
                );
            }
            (
                200,
                "application/json".into(),
                serde_json::json!({"id": hash.to_string()}).to_string(),
            )
        }

        "/intent" => {
            let intent = Intent {
                id: FihHash::from_hex(&qv(q, "id")),
                from_facts: qv(q, "from")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(FihHash::from_hex)
                    .collect(),
                description: qv(q, "desc"),
                creator: qv(q, "creator"),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            };
            let hash = match s.submit_intent(&intent).await {
                Ok(h) => h,
                Err(e) => {
                    return (
                        500,
                        "application/json".into(),
                        serde_json::json!({"error": format!("submit_intent: {:?}", e)}).to_string(),
                    );
                }
            };
            // Flush immediately for single-intent endpoints (caller expects durability).
            if let Err(e) = s.flush_pending().await {
                return (
                    500,
                    "application/json".into(),
                    serde_json::json!({"error": format!("flush: {}", e)}).to_string(),
                );
            }
            (
                200,
                "application/json".into(),
                serde_json::json!({"id": hash.to_string()}).to_string(),
            )
        }

        "/claim" => match s.claim_intent(&qv(q, "id"), &qv(q, "agent")).await {
            Ok(()) => (
                200,
                "application/json".into(),
                serde_json::json!({"status": "claimed"}).to_string(),
            ),
            Err(e) => {
                let msg = format!("{:?}", e);
                let code = if msg.contains("Conflict") {
                    409
                } else if msg.contains("not found") {
                    404
                } else {
                    500
                };
                (
                    code,
                    "application/json".into(),
                    serde_json::json!({"error": msg}).to_string(),
                )
            }
        },

        "/conclude" => match s.conclude_intent(&qv(q, "id"), &qv(q, "result")).await {
            Ok(fact) => (
                200,
                "application/json".into(),
                serde_json::json!({"status": "concluded", "fact_id": fact.id.to_string()})
                    .to_string(),
            ),
            Err(e) => (
                500,
                "application/json".into(),
                serde_json::json!({"error": format!("{:?}", e)}).to_string(),
            ),
        },

        "/state" => {
            let state = s.read_state().await;
            (
                200,
                "application/json".into(),
                serde_json::to_string(&state).unwrap_or_else(|_| "{}".into()),
            )
        }

        "/flush" => match s.flush_pending().await {
            Ok(()) => (
                200,
                "application/json".into(),
                serde_json::json!({"status": "ok"}).to_string(),
            ),
            Err(e) => (
                500,
                "application/json".into(),
                serde_json::json!({"error": format!("flush: {}", e)}).to_string(),
            ),
        },

        "/rebuild" => match s.rebuild_cache().await {
            Ok(()) => (
                200,
                "application/json".into(),
                serde_json::json!({"status": "ok"}).to_string(),
            ),
            Err(e) => (
                500,
                "application/json".into(),
                serde_json::json!({"error": format!("rebuild: {}", e)}).to_string(),
            ),
        },

        _ => (
            404,
            "application/json".into(),
            serde_json::json!({"error": "not found"}).to_string(),
        ),
    }
}

// ── Document helpers ─────────────────────────────────────────────────

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
        // Async FactCapable: enqueue in pending buffer only (no R2 PUT per paragraph).
        nexus_model::AsyncFactCapable::submit_fact(s, &fact)
            .await
            .map_err(|e| format!("submit para {i}: {e:?}"))?;
        last_id = para_id;
    }
    // Flush all pending writes to R2 in a single apply_batch call.
    s.flush_pending().await.map_err(|e| format!("flush: {e}"))?;
    // Write consolidated snapshot for fast cold-start recovery.
    if let Err(e) = write_snapshot(s).await {
        worker::console_log!("snapshot write failed: {e}");
    }
    Ok(last_id)
}

async fn write_snapshot<I: nex::io::AsyncFileIo>(s: &FihStorage<I>) -> Result<(), String> {
    use nex::storage::core::ChainEntry;
    use nex::storage::core::record::FactRecord;
    use nex::storage::core::record::IntentRecord;
    let facts: Vec<FactRecord> = s.fact_store.values();
    let intents: Vec<IntentRecord> = s.intent_store.values();
    let entry = ChainEntry {
        prev_cursor: 0,
        records_flushed: facts.len() as u64,
        facts,
        intents,
    };
    let bytes = postcard::to_allocvec(&entry).map_err(|e| format!("snapshot serialize: {e}"))?;
    s.io.write("_snapshot/facts.bin", &bytes).await
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
