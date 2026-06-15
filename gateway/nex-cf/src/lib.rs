// gateway/nex-cf — Consumes FihStorage<CfFihIo> via async traits.
// No block_on. No locks. Pure async over R2.

use std::cell::UnsafeCell;
use worker::*;

use nexus_model::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead,
    Content, Fact, FihHash, Intent,
};
use nexus_storage_sim::cf_io::CfFihIo;
use nexus_storage_sim::FihStorage;

// ── Static storage (single-threaded Workers isolate) ───────────────────

struct SyncCell(UnsafeCell<Option<FihStorage<CfFihIo>>>);
unsafe impl Sync for SyncCell {}
static STORE: SyncCell = SyncCell(UnsafeCell::new(None));

fn store() -> &'static FihStorage<CfFihIo> {
    unsafe { (&mut *STORE.0.get()).as_ref().expect("FihStorage not initialized") }
}

fn init_store(bucket: worker::Bucket) {
    unsafe {
        let ptr = STORE.0.get();
        if (*ptr).is_none() {
            *ptr = Some(FihStorage::new(CfFihIo::new(bucket), "cf-nexus"));
        }
    }
}

// ── Request helpers ──────────────────────────────────────────────────

fn qv(q: &[(String, String)], k: &str) -> String {
    q.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default()
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    init_store(env.bucket("FIH_R2").expect("FIH_R2 bucket binding required"));

    let url = req.url()?;
    let path = url.path().to_string();
    let q: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let s = store();

    match path.as_str() {
        "/" => Response::ok("nexus-cf"),

        // ── Facts ──────────────────────────────────────────────────
        "/fact" => {
            let fact = Fact {
                id: FihHash(qv(&q, "id")),
                origin: qv(&q, "origin"),
                content: Content { mime_type: "text/plain".into(), data: qv(&q, "content").into_bytes() },
                creator: qv(&q, "creator"),
            };
            match s.submit_fact(&fact).await {
                Ok(hash) => Response::from_json(&serde_json::json!({"id": hash.0})),
                Err(e) => Response::error(format!("submit_fact: {:?}", e), 500),
            }
        }

        // ── Intents ────────────────────────────────────────────────
        "/intent" => {
            let intent = Intent {
                id: FihHash(qv(&q, "id")),
                from_facts: qv(&q, "from").split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
                description: qv(&q, "desc"),
                creator: qv(&q, "creator"),
                worker: None, to_fact_id: None, last_heartbeat_at: None,
                created_at: None, is_concluded: false, concluded_at: None,
            };
            match s.submit_intent(&intent).await {
                Ok(hash) => Response::from_json(&serde_json::json!({"id": hash.0})),
                Err(e) => Response::error(format!("submit_intent: {:?}", e), 500),
            }
        }

        "/claim" => {
            match s.claim_intent(&qv(&q, "id"), &qv(&q, "agent")).await {
                Ok(()) => Response::from_json(&serde_json::json!({"status":"claimed"})),
                Err(e) => Response::error(format!("claim: {:?}", e), 409),
            }
        }

        "/conclude" => {
            match s.conclude_intent(&qv(&q, "id"), &qv(&q, "result")).await {
                Ok(fact) => Response::from_json(&serde_json::json!({"status":"concluded","fact_id": fact.id.0})),
                Err(e) => Response::error(format!("conclude: {:?}", e), 500),
            }
        }

        // ── State ──────────────────────────────────────────────────
        "/state" => {
            let state = s.read_state().await;
            Response::from_json(&state)
        }

        "/len" => {
            let state = s.read_state().await;
            Response::from_json(&serde_json::json!({
                "facts": state.facts.len(), "intents": state.intents.len(), "hints": state.hints.len(),
            }))
        }

        // ── IO persistence ─────────────────────────────────────────
        "/flush" => {
            match s.flush_pending().await {
                Ok(()) => Response::from_json(&serde_json::json!({"status":"ok"})),
                Err(e) => Response::error(format!("flush: {}", e), 500),
            }
        }

        "/rebuild" => {
            match s.rebuild_cache().await {
                Ok(()) => Response::from_json(&serde_json::json!({"status":"ok"})),
                Err(e) => Response::error(format!("rebuild: {}", e), 500),
            }
        }

        _ => Response::error("not found", 404),
    }
}
