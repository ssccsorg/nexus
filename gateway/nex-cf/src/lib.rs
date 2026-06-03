// gateway/nex-cf — CF Worker with nex AsyncStore-backed Blackboard.
//
// Architecture:
//   HTTP handler (async):
//     hydrate CF KV → AsyncStoreKv (sync HashMap)     ← async I/O
//     use Blackboard trait (sync logic)                ← pure logic
//     drain AsyncStoreKv → CF KV                       ← async I/O
//
// Key design: Core traits are sync with interior mutability (Arc<RwLock<>>).
// Async boundary exists only at hydrate/drain — never during trait operations.

use nex::storage::composite::AsyncStoreKv;
use nexus_model::{
    Blackboard, BlackboardError, BoardState, Content, Fact, FactCapable, FihHash, Intent,
    IntentCapable, MetaStore, StorageRead,
};
use serde::Deserialize;
use worker::*;

// ── CF-backed Blackboard ─────────────────────────────────────────────────

struct CfBlackboard {
    facts: AsyncStoreKv,
    intents: AsyncStoreKv,
}

impl CfBlackboard {
    fn new() -> Self {
        Self {
            facts: AsyncStoreKv::new(),
            intents: AsyncStoreKv::new(),
        }
    }

    async fn hydrate(&self, kv: &KvStore, kind: &str) -> Result<u64> {
        let store = match kind {
            "fact" => &self.facts,
            _ => &self.intents,
        };
        let ids: Vec<String> = kv
            .get(&format!("index:{kind}"))
            .text()
            .await?
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let mut count = 0;
        for id in &ids {
            let key = format!("{kind}:{id}");
            if let Some(raw) = kv.get(&key).text().await? {
                store.set(&key, &raw).unwrap();
                count += 1;
            }
        }
        Ok(count)
    }

    async fn drain(&self, kv: &KvStore, kind: &str) -> Result<u64> {
        let store = match kind {
            "fact" => &self.facts,
            _ => &self.intents,
        };
        let ids: Vec<String> = kv
            .get(&format!("index:{kind}"))
            .text()
            .await?
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let mut count = 0;
        for id in &ids {
            let key = format!("{kind}:{id}");
            if let Ok(Some(val)) = store.get(&key) {
                kv.put(&key, val)?.execute().await?;
                count += 1;
            }
        }
        Ok(count)
    }
}

// ── FactCapable (sync, &self via Arc<RwLock<HashMap>>) ─────────────────

impl FactCapable for CfBlackboard {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let id = fact.id.0.clone();
        let json =
            serde_json::to_string(fact).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.facts
            .set(&format!("fact:{id}"), &json)
            .map_err(BlackboardError::Internal)?;
        Ok(fact.id.clone())
    }
}

impl IntentCapable for CfBlackboard {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let id = intent.id.0.clone();
        let json =
            serde_json::to_string(intent).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.intents
            .set(&format!("intent:{id}"), &json)
            .map_err(BlackboardError::Internal)?;
        Ok(intent.id.clone())
    }
    fn claim_intent(&self, id: &str, agent: &str) -> Result<(), BlackboardError> {
        let key = format!("intent:{id}");
        let raw = self
            .intents
            .get(&key)
            .map_err(BlackboardError::Internal)?
            .ok_or(BlackboardError::NotFound(id.into()))?;
        let mut intent: Intent =
            serde_json::from_str(&raw).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        if let Some(ref c) = intent.worker {
            return Err(BlackboardError::Conflict(format!("claimed by {c}")));
        }
        intent.worker = Some(agent.into());
        let json =
            serde_json::to_string(&intent).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.intents
            .set(&key, &json)
            .map_err(BlackboardError::Internal)
    }
    fn heartbeat(&self, _id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn release_intent(&self, _id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn conclude_intent(&self, id: &str, result: &str) -> Result<Fact, BlackboardError> {
        let key = format!("intent:{id}");
        let raw = self
            .intents
            .get(&key)
            .map_err(BlackboardError::Internal)?
            .ok_or(BlackboardError::NotFound(id.into()))?;
        let mut intent: Intent =
            serde_json::from_str(&raw).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let fact_id = format!("fact_from_{id}");
        let fact = Fact {
            id: FihHash(fact_id.clone()),
            origin: "nex-cf".into(),
            content: Content {
                mime_type: "text/plain".into(),
                data: result.as_bytes().to_vec(),
            },
            creator: intent.creator.clone(),
        };
        let json =
            serde_json::to_string(&fact).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.facts
            .set(&format!("fact:{fact_id}"), &json)
            .map_err(BlackboardError::Internal)?;
        intent.to_fact_id = Some(fact_id);
        intent.worker = None;
        let json =
            serde_json::to_string(&intent).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.intents
            .set(&key, &json)
            .map_err(BlackboardError::Internal)?;
        Ok(fact)
    }
}

impl StorageRead for CfBlackboard {
    fn project_id(&self) -> &str {
        "default"
    }
    fn read_state(&self) -> BoardState {
        BoardState {
            facts: vec![],
            intents: vec![],
            hints: vec![],
        }
    }
}

// ── Blackboard (delegates to Capable traits via &*self coercion) ────────

impl Blackboard for CfBlackboard {
    fn project_id(&self) -> &str {
        "default"
    }
    fn submit_fact(&mut self, f: &Fact) -> Result<FihHash, BlackboardError> {
        FactCapable::submit_fact(&*self, f)
    }
    fn submit_intent(&mut self, i: &Intent) -> Result<FihHash, BlackboardError> {
        IntentCapable::submit_intent(&*self, i)
    }
    fn submit_hint(&mut self, _h: &nexus_model::Hint) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn claim_intent(&mut self, id: &str, a: &str) -> Result<(), BlackboardError> {
        IntentCapable::claim_intent(&*self, id, a)
    }
    fn heartbeat(&mut self, id: &str, a: &str) -> Result<(), BlackboardError> {
        IntentCapable::heartbeat(&*self, id, a)
    }
    fn release_intent(&mut self, id: &str, a: &str) -> Result<(), BlackboardError> {
        IntentCapable::release_intent(&*self, id, a)
    }
    fn conclude_intent(&mut self, id: &str, r: &str) -> Result<Fact, BlackboardError> {
        IntentCapable::conclude_intent(&*self, id, r)
    }
    fn read_state(&self) -> BoardState {
        StorageRead::read_state(self)
    }
}

// ── Router ───────────────────────────────────────────────────────────────

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/", |_req, _ctx| async {
            Response::ok("nexus-gateway-nex-cf")
        })
        .post_async("/facts", |mut req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let bb = CfBlackboard::new();
            let body: SubmitFactRequest = req.json().await?;
            let fact = Fact {
                id: FihHash(body.id.clone()),
                origin: body.origin,
                content: Content {
                    mime_type: "application/json".into(),
                    data: serde_json::to_vec(&serde_json::json!({
                        "text": body.text, "tags": body.tags,
                    }))
                    .map_err(|e| Error::RustError(e.to_string()))?,
                },
                creator: body.creator,
            };
            bb.hydrate(&kv, "fact")
                .await
                .map_err(|e| Error::RustError(e.to_string()))?;
            let id = bb
                .submit_fact(&fact)
                .map_err(|e| Error::RustError(e.to_string()))?;
            bb.drain(&kv, "fact")
                .await
                .map_err(|e| Error::RustError(e.to_string()))?;
            let index_key = "index:fact";
            let mut ids: Vec<String> = kv
                .get(index_key)
                .text()
                .await?
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default();
            if !ids.contains(&id.0) {
                ids.push(id.0.clone());
            }
            kv.put(index_key, serde_json::to_string(&ids)?)?
                .execute()
                .await?;
            Response::from_json(&serde_json::json!({"id": id.0}))
        })
        .get_async("/state", |_req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let mut res = serde_json::json!({"facts": 0, "intents": 0, "hints": 0});
            for kind in &["fact", "intent"] {
                let ids: Vec<String> = kv
                    .get(&format!("index:{kind}"))
                    .text()
                    .await?
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default();
                res[*kind] = serde_json::json!(ids.len());
            }
            Response::from_json(&res)
        })
        .run(req, env)
        .await
}

#[derive(Deserialize)]
struct SubmitFactRequest {
    id: String,
    origin: String,
    text: String,
    tags: Vec<String>,
    creator: String,
}

// ── Durable Object stub ──────────────────────────────────────────────────

#[durable_object]
pub struct IntentClaimDO {
    #[allow(unused)]
    state: worker::State,
}
impl worker::DurableObject for IntentClaimDO {
    fn new(state: worker::State, _env: Env) -> Self {
        Self { state }
    }
    async fn fetch(&self, _req: Request) -> Result<Response> {
        Response::ok("IntentClaimDO stub")
    }
}
