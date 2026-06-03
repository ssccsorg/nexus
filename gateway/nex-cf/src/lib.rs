// gateway/nex-cf — Thin HTTP adapter. GET-only. No Router.
//
// PetgraphStorage uses conditional graph storage (Arc<RwLock<>> on native,
// Rc<RefCell<>> on WASM). WASM runs single-threaded so no Send/Sync bound
// is needed. Thread-local storage provides state persistence across requests
// within the same isolate.

use nex::{Blackboard, BlackboardError, Content, DefaultBlackboard, Fact, FihHash, Intent};
use std::cell::RefCell;
use worker::*;

thread_local! {
    static BB: RefCell<DefaultBlackboard> = RefCell::new(DefaultBlackboard::new());
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
        let state = BB.with(|bb| bb.borrow().read_state());
        return Response::from_json(&state);
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
        let hash = BB
            .with(|bb| bb.borrow().submit_fact(&fact))
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
        BB.with(|bb| bb.borrow().submit_intent(&intent))
            .map_err(|e| Error::RustError(e.to_string()))?;
        return Response::from_json(&serde_json::json!({"id": intent.id.0}));
    }

    // Claim
    if path == "/claim" {
        match BB.with(|bb| bb.borrow().claim_intent(&qv(&q, "id"), &qv(&q, "agent"))) {
            Ok(()) => {
                return Response::from_json(&serde_json::json!({"status":"claimed"}));
            }
            Err(BlackboardError::Conflict(m)) => {
                return Ok(Response::from_json(&serde_json::json!({"error":m}))?.with_status(409));
            }
            Err(e) => {
                return Err(Error::RustError(e.to_string()));
            }
        }
    }

    // Conclude
    if path == "/conclude" {
        let f = BB
            .with(|bb| {
                bb.borrow()
                    .conclude_intent(&qv(&q, "id"), &qv(&q, "result"))
            })
            .map_err(|e| Error::RustError(e.to_string()))?;
        return Response::from_json(&serde_json::json!({"status":"concluded","fact":f}));
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
