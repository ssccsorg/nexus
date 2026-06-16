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

fn qv(q: &[(String, String)], k: &str) -> String {
    q.iter()
        .find(|(key, _)| key == k)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
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
        "/" => Response::ok("nexus-cf"),

        "/fact" => {
            let fact = Fact {
                id: FihHash::from_hex(&qv(&q, "id")),
                origin: qv(&q, "origin"),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: qv(&q, "content").into_bytes(),
                },
                creator: qv(&q, "creator"),
            };
            match s.submit_fact(&fact).await {
                Ok(hash) => Response::from_json(&serde_json::json!({"id": hash.to_string()})),
                Err(e) => Response::error(format!("submit_fact: {:?}", e), 500),
            }
        }

        "/intent" => {
            let intent = Intent {
                id: FihHash::from_hex(&qv(&q, "id")),
                from_facts: qv(&q, "from")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(FihHash::from_hex)
                    .collect(),
                description: qv(&q, "desc"),
                creator: qv(&q, "creator"),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            };
            match s.submit_intent(&intent).await {
                Ok(hash) => Response::from_json(&serde_json::json!({"id": hash.to_string()})),
                Err(e) => Response::error(format!("submit_intent: {:?}", e), 500),
            }
        }

        "/claim" => match s.claim_intent(&qv(&q, "id"), &qv(&q, "agent")).await {
            Ok(()) => Response::from_json(&serde_json::json!({"status":"claimed"})),
            Err(e) => {
                let msg = format!("{:?}", e);
                let code = if msg.contains("Conflict") {
                    409
                } else if msg.contains("not found") {
                    404
                } else {
                    500
                };
                Response::error(msg, code)
            }
        },

        "/conclude" => match s.conclude_intent(&qv(&q, "id"), &qv(&q, "result")).await {
            Ok(fact) => Response::from_json(
                &serde_json::json!({"status":"concluded","fact_id": fact.id.to_string()}),
            ),
            Err(e) => Response::error(format!("{:?}", e), 500),
        },

        "/state" => {
            let state = s.read_state().await;
            Response::from_json(&state)
        }

        "/flush" => match s.flush_pending().await {
            Ok(()) => Response::from_json(&serde_json::json!({"status":"ok"})),
            Err(e) => Response::error(format!("flush: {}", e), 500),
        },

        "/rebuild" => match s.rebuild_cache().await {
            Ok(()) => Response::from_json(&serde_json::json!({"status":"ok"})),
            Err(e) => Response::error(format!("rebuild: {}", e), 500),
        },

        _ => Response::error("not found", 404),
    }
}
