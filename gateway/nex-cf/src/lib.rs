// gateway/nex-cf — CF Worker running nex DefaultBlackboard.

use std::cell::RefCell;

use nex::DefaultBlackboard;
use nexus_model::{Blackboard, BlackboardError, Content, Fact, FihHash, Intent};
use serde::Deserialize;
use worker::*;

thread_local! {
    static BB: RefCell<DefaultBlackboard> = RefCell::new(DefaultBlackboard::new());
}

#[event(fetch)]
pub async fn main(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/", |_req, _ctx| async { Response::ok("nexus-gateway-nex-cf") })
        .get_async("/state", |_req, _ctx| async move {
            BB.with(|bb| Response::from_json(&bb.borrow().read_state()))
        })
        .post_async("/facts", |mut req, _ctx| async move {
            let b: SubmitFactRequest = req.json().await?;
            let fact = Fact {
                id: FihHash(b.id.clone()), origin: b.origin,
                content: Content {
                    mime_type: "application/json".into(),
                    data: serde_json::to_vec(&serde_json::json!({"text": b.text, "tags": b.tags}))
                        .map_err(|e| Error::RustError(e.to_string()))?,
                },
                creator: b.creator,
            };
            BB.with(|bb| {
                let hash = bb.borrow_mut().submit_fact(&fact)
                    .map_err(|e| Error::RustError(e.to_string()))?;
                Response::from_json(&serde_json::json!({"id": hash.0}))
            })
        })
        .post_async("/intents", |mut req, _ctx| async move {
            let b: SubmitIntentRequest = req.json().await?;
            let id = b.id.unwrap_or_else(|| format!("intent_{}", Date::now().as_millis()));
            let intent = Intent {
                id: FihHash(id.clone()), from_facts: b.from_facts,
                description: b.description, creator: b.creator,
                worker: None, to_fact_id: None,
                last_heartbeat_at: None, created_at: None, concluded_at: None,
            };
            BB.with(|bb| {
                bb.borrow_mut().submit_intent(&intent)
                    .map_err(|e| Error::RustError(e.to_string()))?;
                Response::from_json(&serde_json::json!({"id": id}))
            })
        })
        .post_async("/intents/:id/claim", |mut req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let body: ClaimRequest = req.json().await?;
            BB.with(|bb| {
                match bb.borrow_mut().claim_intent(&id, &body.agent) {
                    Ok(()) => Response::from_json(&serde_json::json!({"status": "claimed"})),
                    Err(BlackboardError::Conflict(m)) =>
                        Ok(Response::from_json(&serde_json::json!({"error": m}))?.with_status(409)),
                    Err(e) => Err(Error::RustError(e.to_string())),
                }
            })
        })
        .post_async("/intents/:id/conclude", |mut req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let body: ConcludeRequest = req.json().await?;
            BB.with(|bb| {
                let fact = bb.borrow_mut().conclude_intent(&id, &body.result)
                    .map_err(|e| Error::RustError(e.to_string()))?;
                Response::from_json(&serde_json::json!({"status": "concluded", "fact": fact}))
            })
        })
        .run(req, _env).await
}

#[derive(Deserialize)]
struct SubmitFactRequest { id: String, origin: String, text: String, tags: Vec<String>, creator: String }
#[derive(Deserialize)]
struct SubmitIntentRequest { id: Option<String>, from_facts: Vec<String>, description: String, creator: String }
#[derive(Deserialize)]
struct ClaimRequest { agent: String }
#[derive(Deserialize)]
struct ConcludeRequest { result: String }

#[durable_object]
pub struct IntentClaimDO { #[allow(unused)] state: worker::State }
impl worker::DurableObject for IntentClaimDO {
    fn new(state: worker::State, _env: Env) -> Self { Self { state } }
    async fn fetch(&self, _req: Request) -> Result<Response> { Response::ok("IntentClaimDO stub") }
}
