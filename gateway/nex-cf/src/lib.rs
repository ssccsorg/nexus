// gateway/nex-cf — Thin HTTP adapter over NativeBlackboard (FihStorage + R2).
//
// Replaces the previous Petgraph-based DefaultBlackboard with native
// FihStorage over Cloudflare R2. Uses nex::NativeBlackboard for all
// Blackboard trait methods. No Petgraph, no CompositeColdStorage.
//
// All endpoints are identical to the original gateway/nex-cf:
//   GET  /state          → full BoardState
//   GET  /fact?id=&...   → submit Fact
//   GET  /intent?id=&... → submit Intent
//   GET  /claim?id=&...  → claim Intent
//   GET  /conclude?id=&..→ conclude Intent

use nex::{
    BlackboardError, Content, Fact, FactCapable, FihHash, Intent, IntentCapable, NativeBlackboard,
    StorageRead,
};
use worker::*;

/// Once-cell that holds the NativeBlackboard<CfFihIo> instance.
/// Initialized once on first request from the Env bindings.
static STORE: std::sync::OnceLock<NativeBlackboard<CfFihIo>> = std::sync::OnceLock::new();

fn store() -> &'static NativeBlackboard<CfFihIo> {
    STORE.get().expect("NativeBlackboard not initialized")
}

use nexus_storage_sim::cf_io::CfFihIo;

pub fn init_store(env: &Env) -> Result<()> {
    STORE.get_or_init(|| {
        let bucket = env.bucket("FIH_R2").expect("FIH_R2 bucket binding required");
        NativeBlackboard::new("cf-nexus", bucket)
    });
    Ok(())
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    init_store(&env)?;

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

    if path == "/" || path.len() <= 1 {
        return Response::ok("nexus-gateway-nex-cf (native storage)");
    }

    if path == "/state" {
        return Response::from_json(&store().read_state());
    }

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
        let hash = store()
            .submit_fact(&fact)
            .map_err(|e| Error::RustError(e.to_string()))?;
        return Response::from_json(&serde_json::json!({"id": hash.0}));
    }

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
            is_concluded: false,
            concluded_at: None,
        };
        store()
            .submit_intent(&intent)
            .map_err(|e| Error::RustError(e.to_string()))?;
        return Response::from_json(&serde_json::json!({"id": intent.id.0}));
    }

    if path == "/claim" {
        match store().claim_intent(&qv(&q, "id"), &qv(&q, "agent")) {
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
            Err(e) => return Err(Error::RustError(e.to_string())),
        }
    }

    if path == "/conclude" {
        match store().conclude_intent(&qv(&q, "id"), &qv(&q, "result")) {
            Ok(f) => {
                return Response::from_json(&serde_json::json!({"status":"concluded","fact":f}));
            }
            Err(BlackboardError::NotFound(m)) => {
                return Ok(Response::from_json(&serde_json::json!({"error":m}))?.with_status(404));
            }
            Err(BlackboardError::Conflict(m)) => {
                return Ok(Response::from_json(&serde_json::json!({"error":m}))?.with_status(409));
            }
            Err(e) => return Err(Error::RustError(e.to_string())),
        }
    }

    Response::error("not found", 404)
}
