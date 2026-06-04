// gateway/nex-cf — Thin HTTP adapter. GET-only. No Router.
//
// WASM single-threaded. We use a once-cell wrapper that relaxes Sync
// on wasm32 targets. CF Workers isolate reuse ensures the same
// DefaultBlackboard instance persists across requests.
//
// Blackboard trait is &self throughout, so no Mutex/RefCell is needed.
// PetgraphStorage owns its internal synchronization.

use nex::{Blackboard, BlackboardError, Content, DefaultBlackboard, Fact, FihHash, Intent};
use worker::*;

/// A once-cell that is Sync on wasm32 (single-threaded) but not on native.
/// This lets us store DefaultBlackboard (which contains Rc<RefCell<>> on
/// wasm32) in a static variable.
struct SyncOnce<T> {
    inner: std::sync::OnceLock<T>,
}

// Safety: wasm32 is single-threaded; Sync is a no-op.
// Native uses Arc<RwLock<>> internally so DefaultBlackboard is Sync there.
#[cfg(target_arch = "wasm32")]
unsafe impl<T> Sync for SyncOnce<T> {}

impl<T> SyncOnce<T> {
    const fn new() -> Self {
        Self {
            inner: std::sync::OnceLock::new(),
        }
    }

    fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        self.inner.get_or_init(f)
    }
}

static BB: SyncOnce<DefaultBlackboard> = SyncOnce::new();

fn bb() -> &'static DefaultBlackboard {
    BB.get_or_init(DefaultBlackboard::new)
}

#[event(fetch)]
pub async fn main(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let path = url.path().to_string();
    let q: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    fn qv(q: &[(String, String)], k: &str) -> String {
        for (key, val) in q {
            if key == k {
                return val.clone();
            }
        }
        String::new()
    }

    // Root
    if path == "/" || path.len() <= 1 {
        return Response::ok("nexus-gateway-nex-cf");
    }

    // State
    if path == "/state" {
        return Response::from_json(&bb().read_state());
    }

    // Fact
    if path == "/fact" {
        let fact = Fact {
            id: FihHash(qv(&q, "id")),
            origin: qv(&q, "origin"),
            content: Content {
                mime_type: "application/json".into(),
                data: qv(&q, "content").into_bytes(),
            },
            creator: qv(&q, "creator"),
        };
        let hash = bb()
            .submit_fact(&fact)
            .map_err(|e| Error::RustError(e.to_string()))?;
        return Response::from_json(&serde_json::json!({"id": hash.0}));
    }

    // Intent
    if path == "/intent" {
        let intent = Intent {
            id: FihHash(qv(&q, "id")),
            from_facts: qv(&q, "from")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            description: qv(&q, "desc"),
            creator: qv(&q, "creator"),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        };
        bb()
            .submit_intent(&intent)
            .map_err(|e| Error::RustError(e.to_string()))?;
        return Response::from_json(&serde_json::json!({"id": intent.id.0}));
    }

    // Claim
    if path == "/claim" {
        match bb().claim_intent(&qv(&q, "id"), &qv(&q, "agent")) {
            Ok(()) => {
                return Response::from_json(&serde_json::json!({"status":"claimed"}));
            }
            Err(BlackboardError::Conflict(m)) => {
                return Ok(Response::from_json(&serde_json::json!({"error":m}))?.with_status(409));
            }
            Err(BlackboardError::NotFound(m)) => {
                return Ok(Response::from_json(&serde_json::json!({"error":m}))?.with_status(404));
            }
            Err(BlackboardError::Forbidden(m)) => {
                return Ok(Response::from_json(&serde_json::json!({"error":m}))?.with_status(403));
            }
            Err(e) => {
                return Err(Error::RustError(e.to_string()));
            }
        }
    }

    // Conclude
    if path == "/conclude" {
        match bb().conclude_intent(&qv(&q, "id"), &qv(&q, "result")) {
            Ok(f) => {
                return Response::from_json(&serde_json::json!({"status":"concluded","fact":f}));
            }
            Err(BlackboardError::NotFound(m)) => {
                return Ok(Response::from_json(&serde_json::json!({"error":m}))?.with_status(404));
            }
            Err(BlackboardError::Conflict(m)) => {
                return Ok(Response::from_json(&serde_json::json!({"error":m}))?.with_status(409));
            }
            Err(e) => {
                return Err(Error::RustError(e.to_string()));
            }
        }
    }

    Response::error("not found", 404)
}

#[durable_object]
pub struct IntentClaimDO {
    #[allow(unused)]
    state: State,
}
impl worker::DurableObject for IntentClaimDO {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }
    async fn fetch(&self, _req: Request) -> Result<Response> {
        Response::ok("stub")
    }
}
