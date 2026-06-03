// gateway/nex-cf — CF Worker. nexus-model types + KV persistence.
// Zero dependency on nex/petgraph. Works on CF Workers Free tier.

use serde::Deserialize;
use worker::*;

use nexus_model::fih::{Content, Fact, FihHash, Intent};

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/", |_req, _ctx| async { Response::ok("nexus-gateway-nex-cf") })
        .get_async("/state", |_req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let facts = index_count(&kv, "fact").await;
            let intents = index_count(&kv, "intent").await;
            Response::from_json(&serde_json::json!({
                "facts": facts, "intents": intents, "hints": 0
            }))
        })
        .post_async("/facts", |mut req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
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
            kv.put(&format!("fact:{}", b.id), serde_json::to_string(&fact)?)?
                .execute().await?;
            index_append(&kv, "fact", &b.id).await?;
            Response::from_json(&serde_json::json!({"id": b.id}))
        })
        .post_async("/intents", |mut req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let b: SubmitIntentRequest = req.json().await?;
            let id = b.id.unwrap_or_else(|| format!("intent_{}", Date::now().as_millis()));
            let intent = Intent {
                id: FihHash(id.clone()), from_facts: b.from_facts,
                description: b.description, creator: b.creator,
                worker: None, to_fact_id: None,
                last_heartbeat_at: None, created_at: None, concluded_at: None,
            };
            kv.put(&format!("intent:{id}"), serde_json::to_string(&intent)?)?
                .execute().await?;
            index_append(&kv, "intent", &id).await?;
            Response::from_json(&serde_json::json!({"id": id}))
        })
        .post_async("/intents/:id/claim", |mut req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let body: ClaimRequest = req.json().await?;
            let kv = ctx.kv("FIH_KV")?;
            let raw = kv.get(&format!("intent:{id}")).text().await?.unwrap_or_default();
            let mut intent: Intent = serde_json::from_str(&raw)
                .map_err(|_| Error::RustError("not found".into()))?;
            if let Some(ref w) = intent.worker {
                return Ok(Response::from_json(&serde_json::json!({"error": format!("claimed by {w}")}))?
                    .with_status(409));
            }
            intent.worker = Some(body.agent);
            kv.put(&format!("intent:{id}"), serde_json::to_string(&intent)?)?.execute().await?;
            Response::from_json(&serde_json::json!({"status": "claimed"}))
        })
        .post_async("/intents/:id/conclude", |mut req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let body: ConcludeRequest = req.json().await?;
            let kv = ctx.kv("FIH_KV")?;
            let raw = kv.get(&format!("intent:{id}")).text().await?.unwrap_or_default();
            let mut intent: Intent = serde_json::from_str(&raw)
                .map_err(|_| Error::RustError("not found".into()))?;
            let fact_id = format!("from_{id}");
            let fact = Fact {
                id: FihHash(fact_id.clone()), origin: "nex-cf".into(),
                content: Content { mime_type: "text/plain".into(), data: body.result.as_bytes().to_vec() },
                creator: intent.creator.clone(),
            };
            kv.put(&format!("fact:{fact_id}"), serde_json::to_string(&fact)?)?.execute().await?;
            index_append(&kv, "fact", &fact_id).await?;
            intent.to_fact_id = Some(fact_id); intent.worker = None;
            kv.put(&format!("intent:{id}"), serde_json::to_string(&intent)?)?.execute().await?;
            Response::from_json(&serde_json::json!({"status": "concluded", "fact": fact}))
        })
        .run(req, env).await
}

async fn index_append(kv: &KvStore, kind: &str, id: &str) -> Result<()> {
    let key = format!("index:{kind}");
    let mut ids: Vec<String> = kv.get(&key).text().await?
        .and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    if !ids.contains(&id.to_string()) {
        ids.push(id.to_string());
        kv.put(&key, serde_json::to_string(&ids)?)?.execute().await?;
    }
    Ok(())
}

async fn index_count(kv: &KvStore, kind: &str) -> u64 {
    kv.get(&format!("index:{kind}")).text().await
        .ok().flatten()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .map(|v| v.len() as u64).unwrap_or(0)
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
