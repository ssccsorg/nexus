// gateway/nex-cf — Consumes FihStorage<CfFihIo> via async traits.
// No block_on. No locks. Pure async over R2.

pub mod cf_io;

use worker::*;

use crate::cf_io::CfFihIo;
use nex::FihStorage;
use nexus_model::{AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead};
use nexus_model::{Content, Fact, FihHash, Intent};

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

// ── Static storage ───────────────────────────────────────────────────

/// SAFETY: Only used on the single-threaded Workers isolate.
/// `FihStorage` contains `RefCell` (from `EntityStore` and `pending`)
/// and `Box<dyn EntityStore>` trait objects that are `!Send + !Sync`,
/// but in the WASM isolate there is no true parallelism — only async
/// concurrency on one thread — so treating the inner type as `Sync`
/// is sound. The `OnceLock` eliminates the previous data race on
/// initialization from the old `UnsafeCell`-based approach.
struct SyncStore(std::sync::OnceLock<FihStorage<CfFihIo>>);
unsafe impl Sync for SyncStore {}
static STORE: SyncStore = SyncStore(std::sync::OnceLock::new());

fn store() -> &'static FihStorage<CfFihIo> {
    STORE.0.get().expect("FihStorage not initialized")
}

fn init_store(bucket: worker::Bucket) {
    STORE.0.get_or_init(|| {
        FihStorage::with_clock(CfFihIo::new(bucket), "cf-nexus", Box::new(CfClock))
    });
}

pub fn qv(q: &[(String, String)], k: &str) -> String {
    q.iter()
        .find(|(key, _)| key == k)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// 요청 경로와 쿼리 파라미터를 받아서 FihStorage 핸들러를 호출합니다.
///
/// worker-rs의 `Request`/`Response` 타입 대신 문자열 기반 인터페이스를 사용하므로
/// CF Worker와 로컬 시뮬레이션 서버(mock/)에서 동일한 로직을 재사용할 수 있습니다.
pub async fn handle_path<I: nex::io::AsyncFileIo>(
    s: &nex::FihStorage<I>,
    path: &str,
    q: &[(String, String)],
) -> (u16, String, String) {
    // Returns (status_code, content_type, body)
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
                Ok(hash) => (
                    200,
                    "application/json".into(),
                    serde_json::json!({"id": hash.to_string()}).to_string(),
                ),
                Err(e) => (
                    500,
                    "application/json".into(),
                    serde_json::json!({"error": format!("submit_fact: {:?}", e)}).to_string(),
                ),
            }
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
            match s.submit_intent(&intent).await {
                Ok(hash) => (
                    200,
                    "application/json".into(),
                    serde_json::json!({"id": hash.to_string()}).to_string(),
                ),
                Err(e) => (
                    500,
                    "application/json".into(),
                    serde_json::json!({"error": format!("submit_intent: {:?}", e)}).to_string(),
                ),
            }
        }

        "/claim" => match s.claim_intent(&qv(q, "id"), &qv(q, "agent")).await {
            Ok(()) => (
                200,
                "application/json".into(),
                serde_json::json!({"status":"claimed"}).to_string(),
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
                serde_json::json!({"status":"concluded","fact_id": fact.id.to_string()})
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
                serde_json::json!({"status":"ok"}).to_string(),
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
                serde_json::json!({"status":"ok"}).to_string(),
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

// ── Entrypoint ───────────────────────────────────────────────────────
// ── 문서 수집 헬퍼 ─────────────────────────────────

/// 문서 텍스트를 청크로 나누어 FIH Facts로 제출합니다.
/// 각 청크는 자동으로 등록된 SemanticStore에 인덱싱됩니다.
pub async fn ingest_document<I: nex::io::AsyncFileIo>(
    s: &nex::FihStorage<I>,
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
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── Entrypoint ───────────────────────────────────────────────────────

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    init_store(
        env.bucket("FIH_R2")
            .expect("FIH_R2 bucket binding required"),
    );

    let url = req.url()?;
    let path = url.path().to_string();
    let q: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let s = store();

    match path.as_str() {
        // Standard FIH endpoints — delegated to generic handle_path
        "/" | "/fact" | "/intent" | "/claim" | "/conclude" | "/state" | "/flush" | "/rebuild" => {
            let (code, _content_type, body) = handle_path(s, &path, &q).await;
            Ok(Response::from_bytes(body.into_bytes())?.with_status(code))
        }

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
                Ok(id) => Response::from_json(&serde_json::json!({"status":"ingested","id": id})),
                Err(e) => Response::error(e, 500),
            }
        }

        "/search" => {
            let query_text = qv(&q, "q");
            if query_text.is_empty() {
                return Response::error("missing 'q' (query text) parameter", 400);
            }
            // Try to use semantic_search with text query
            let query = crate::cf_io::TextQuery { text: query_text };
            match s.semantic_search(&query, 10) {
                Ok(results) => {
                    let items: Vec<serde_json::Value> = results
                        .iter()
                        .map(|(idx, score)| {
                            let hex = s.resolve_semantic_idx(*idx);
                            serde_json::json!({
                                "index": idx,
                                "score": score,
                                "id": hex,
                            })
                        })
                        .collect();
                    Response::from_json(&serde_json::json!({"results": items}))
                }
                Err(e) => Response::error(format!("search: {e}"), 500),
            }
        }

        _ => Response::error("not found", 404),
    }
}
