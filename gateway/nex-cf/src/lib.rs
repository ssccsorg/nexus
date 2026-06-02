// gateway/nex-cf — CF-backed Blackboard implementation.
//
//   KV:  metadata + indices
//   R2:  blob content (Fact.text, etc.)
//   DO:  reserved for future CAS-based Intent claiming
//
// Depends only on `worker`, `serde`, `serde_json`.
// Zero dependency on `nex`, `petgraph`, `interface-cypher`, or `nexus-model`.

use serde::{Deserialize, Serialize};
use worker::DurableObject;
use worker::*;

// ── FIH types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Fact {
    id: String,
    origin: String,
    tags: Vec<String>,
    title: String,
    href: String,
    text_len: usize,
    r2_key: String,
    creator: String,
    created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Intent {
    id: String,
    from_facts: Vec<String>,
    description: String,
    creator: String,
    worker: Option<String>,
    to_fact_id: Option<String>,
    created_at: Option<String>,
    concluded_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Hint {
    id: String,
    content: String,
    creator: String,
}

// ── KV keys ──────────────────────────────────────────────────────────────

fn encode_id(id: &str) -> String {
    id.replace("/", "_").replace("#", "_")
}

fn fact_key(id: &str) -> String {
    format!("fact:{}", encode_id(id))
}
fn intent_key(id: &str) -> String {
    format!("intent:{}", encode_id(id))
}
fn hint_key(id: &str) -> String {
    format!("hint:{}", encode_id(id))
}
fn tag_index_key(tag: &str) -> String {
    format!("tag:{}", encode_id(tag))
}
fn all_index(kind: &str) -> String {
    format!("all:{kind}")
}

fn content_key(id: &str) -> String {
    format!("cont/{}", encode_id(id))
}

// ── Router ───────────────────────────────────────────────────────────────

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/", |_req, _ctx| async {
            Response::ok("nexus-gateway-nex-cf")
        })
        // ── Ingest ───────────────────────────────────────────────────
        .post_async("/ingest", |req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let r2 = ctx.bucket("FIH_R2")?;
            let limit: Option<usize> = req
                .url()?
                .query_pairs()
                .find(|(k, _)| k == "limit")
                .and_then(|(_, v)| v.parse().ok());

            let mut resp = Fetch::Url("https://docs.ssccs.org/search.json".parse().unwrap())
                .send()
                .await
                .map_err(|e| Error::RustError(format!("fetch: {e}")))?;
            let body = resp
                .text()
                .await
                .map_err(|e| Error::RustError(format!("read: {e}")))?;
            let entries: Vec<SearchEntry> =
                serde_json::from_str(&body).map_err(|e| Error::RustError(format!("parse: {e}")))?;

            let batch: &[SearchEntry] = match limit {
                Some(n) if n < entries.len() => &entries[..n],
                _ => &entries,
            };
            let now = Date::now().as_millis().to_string();
            let mut count = 0u64;

            for entry in batch {
                let id = if entry.objectID.is_empty() {
                    format!("doc_{}", count)
                } else {
                    entry.objectID.clone()
                };
                let tags: Vec<String> = if entry.crumbs.is_empty() {
                    vec![entry.section.clone()]
                } else {
                    entry.crumbs.clone()
                };

                let r2_key = content_key(&id);
                r2.put(&r2_key, entry.text.as_bytes().to_vec())
                    .execute()
                    .await?;

                let fact = Fact {
                    id: id.clone(),
                    origin: "docs.ssccs.org".into(),
                    tags: tags.clone(),
                    title: entry.title.clone(),
                    href: entry.href.clone(),
                    text_len: entry.text.len(),
                    r2_key,
                    creator: "system".into(),
                    created_at: now.clone(),
                };
                kv.put(&fact_key(&id), serde_json::to_string(&fact)?)?
                    .execute()
                    .await?;
                for tag in &tags {
                    if !tag.is_empty() {
                        index_append(&kv, &tag_index_key(tag), &id).await?;
                    }
                }
                index_append(&kv, &all_index("fact"), &id).await?;
                count += 1;
            }
            Response::from_json(&serde_json::json!({"ingested": count}))
        })
        // ── Facts ────────────────────────────────────────────────────
        .get_async("/facts", |req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let tag: Option<String> = req
                .url()?
                .query_pairs()
                .find(|(k, _)| k == "tag")
                .map(|(_, v)| v.to_string());
            let ids: Vec<String> = if let Some(ref t) = tag {
                kv.get(&tag_index_key(t))
                    .text()
                    .await?
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default()
            } else {
                kv.get(&all_index("fact"))
                    .text()
                    .await?
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default()
            };
            let mut facts = Vec::new();
            for id in &ids {
                if let Some(raw) = kv.get(&fact_key(id)).text().await?
                    && let Ok(f) = serde_json::from_str::<Fact>(&raw)
                {
                    facts.push(f);
                }
            }
            Response::from_json(&serde_json::json!({"count": facts.len(), "facts": facts}))
        })
        .get_async("/facts/:id", |_req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let kv = ctx.kv("FIH_KV")?;
            match kv.get(&fact_key(&id)).text().await? {
                Some(raw) => {
                    let fact: Fact = serde_json::from_str(&raw)
                        .map_err(|e| Error::RustError(format!("deserialize: {e}")))?;
                    Response::from_json(&serde_json::json!({"fact": &fact}))
                }
                None => Response::error("not found", 404),
            }
        })
        .get_async("/content/:id", |_req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let kv = ctx.kv("FIH_KV")?;
            let r2 = ctx.bucket("FIH_R2")?;
            let raw = kv.get(&fact_key(&id)).text().await?.unwrap_or_default();
            let fact: Fact =
                serde_json::from_str(&raw).map_err(|_| Error::RustError("not found".into()))?;
            match r2.get(&fact.r2_key).execute().await? {
                Some(obj) => {
                    let body = obj
                        .body()
                        .ok_or_else(|| Error::RustError("no body".into()))?;
                    let bytes = body.bytes().await?;
                    Ok(Response::from_bytes(bytes)?)
                }
                None => Response::error("not found", 404),
            }
        })
        // ── Intents (KV-based, no DO) ─────────────────────────────────
        .post_async("/intents", |mut req, ctx| async move {
            let body: IntentInput = req.json().await?;
            let kv = ctx.kv("FIH_KV")?;
            let id = body.id.unwrap_or_else(|| format!("intent_{}", uid()));
            let intent = Intent {
                id: id.clone(),
                from_facts: body.from_facts,
                description: body.description,
                creator: body.creator,
                worker: None,
                to_fact_id: None,
                created_at: Some(Date::now().as_millis().to_string()),
                concluded_at: None,
            };
            kv.put(&intent_key(&id), serde_json::to_string(&intent)?)?
                .execute()
                .await?;
            index_append(&kv, &all_index("intent"), &id).await?;
            Response::from_json(&serde_json::json!({"id": id}))
        })
        .post_async("/intents/:id/claim", |mut req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let body: ClaimInput = req.json().await?;
            let kv = ctx.kv("FIH_KV")?;
            let raw = kv.get(&intent_key(&id)).text().await?.unwrap_or_default();
            let mut intent: Intent = serde_json::from_str(&raw)
                .map_err(|_| Error::RustError("intent not found".into()))?;
            if intent.worker.is_some() {
                return Ok(Response::from_json(&serde_json::json!({
                    "error": "already claimed",
                    "by": intent.worker,
                }))?
                .with_status(409));
            }
            intent.worker = Some(body.agent);
            kv.put(&intent_key(&id), serde_json::to_string(&intent)?)?
                .execute()
                .await?;
            Response::from_json(&serde_json::json!({"status": "claimed"}))
        })
        .post_async("/intents/:id/conclude", |mut req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let body: ConcludeInput = req.json().await?;
            let kv = ctx.kv("FIH_KV")?;
            let r2 = ctx.bucket("FIH_R2")?;
            let raw = kv.get(&intent_key(&id)).text().await?.unwrap_or_default();
            let mut intent: Intent = serde_json::from_str(&raw)
                .map_err(|_| Error::RustError("intent not found".into()))?;

            let fact_id = format!("fact_from_{}", id);
            let r2_key = content_key(&fact_id);
            r2.put(&r2_key, body.result.as_bytes().to_vec())
                .execute()
                .await?;

            let fact = Fact {
                id: fact_id.clone(),
                origin: "gateway".into(),
                tags: intent.from_facts.clone(),
                title: format!("Output of {}", id),
                href: String::new(),
                text_len: body.result.len(),
                r2_key,
                creator: intent.creator.clone(),
                created_at: Date::now().as_millis().to_string(),
            };
            kv.put(&fact_key(&fact_id), serde_json::to_string(&fact)?)?
                .execute()
                .await?;
            index_append(&kv, &all_index("fact"), &fact_id).await?;
            intent.to_fact_id = Some(fact_id);
            intent.concluded_at = Some(Date::now().as_millis().to_string());
            intent.worker = None; // release claim
            kv.put(&intent_key(&id), serde_json::to_string(&intent)?)?
                .execute()
                .await?;
            Response::from_json(&serde_json::json!({"status": "concluded"}))
        })
        .get_async("/intents", |_req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let ids: Vec<String> = kv
                .get(&all_index("intent"))
                .text()
                .await?
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default();
            let mut intents = Vec::new();
            for id in &ids {
                if let Some(raw) = kv.get(&intent_key(id)).text().await?
                    && let Ok(i) = serde_json::from_str::<Intent>(&raw)
                {
                    intents.push(i);
                }
            }
            Response::from_json(&serde_json::json!({"count": intents.len(), "intents": intents}))
        })
        // ── State ────────────────────────────────────────────────────
        .get_async("/state", |_req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let facts = count_index(&kv, "fact").await;
            let intents = count_index(&kv, "intent").await;
            let hints = count_index(&kv, "hint").await;
            Response::from_json(
                &serde_json::json!({"facts": facts, "intents": intents, "hints": hints}),
            )
        })
        .run(req, env)
        .await
}

// ── Helpers ──────────────────────────────────────────────────────────────

async fn index_append(kv: &KvStore, key: &str, id: &str) -> Result<()> {
    let mut ids: Vec<String> = kv
        .get(key)
        .text()
        .await?
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    if !ids.contains(&id.to_string()) {
        ids.push(id.to_string());
        kv.put(key, serde_json::to_string(&ids)?)?.execute().await?;
    }
    Ok(())
}

async fn count_index(kv: &KvStore, kind: &str) -> u64 {
    match kv.get(&all_index(kind)).text().await {
        Ok(Some(v)) => serde_json::from_str::<Vec<String>>(&v)
            .ok()
            .map(|v| v.len() as u64)
            .unwrap_or(0),
        _ => 0,
    }
}

fn uid() -> String {
    Date::now().as_millis().to_string()
}

// ── Input types ──────────────────────────────────────────────────────────

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct SearchEntry {
    #[serde(default)]
    objectID: String,
    #[serde(default)]
    href: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    section: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    crumbs: Vec<String>,
}

#[derive(Deserialize)]
struct IntentInput {
    id: Option<String>,
    from_facts: Vec<String>,
    description: String,
    creator: String,
}

#[derive(Deserialize)]
struct ClaimInput {
    agent: String,
}

#[derive(Deserialize)]
struct ConcludeInput {
    result: String,
}

// ── Durable Object stub ──────────────────────────────────────────────────

#[durable_object]
pub struct IntentClaimDO {
    #[allow(unused)]
    state: State,
}

impl DurableObject for IntentClaimDO {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }
    async fn fetch(&self, _req: Request) -> Result<Response> {
        Response::ok("IntentClaimDO stub")
    }
}
