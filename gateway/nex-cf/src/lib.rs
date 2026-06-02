// gateway/nex-cf — FIH knowledge graph ingestion from search.json.
//
// Reads https://docs.ssccs.org/search.json, parses each entry into a Fact,
// and stores in KV (FIH_KV). Tags/crumbs are stored as structured metadata.
// R2 is reserved for future blob content.
//
// Endpoints:
//   GET  /                     — health check
//   POST /ingest               — fetch search.json, parse, store Facts to KV
//   GET  /facts                 — list all Facts
//   GET  /facts/:id             — get a single Fact by ID
//   GET  /facts?tag=:tag        — filter Facts by tag/crumb

use serde::{Deserialize, Serialize};
use worker::*;
use worker::DurableObject;

// ── Search index entry ───────────────────────────────────────────────────

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

// ── Fact storage model ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StoredFact {
    id: String,
    title: String,
    section: String,
    tags: Vec<String>,
    href: String,
    text_snippet: String,
    text_len: usize,
    ingested_at: String,
}

// ── KV keys ──────────────────────────────────────────────────────────────

fn fact_key(id: &str) -> String {
    format!("fact:{id}")
}

fn tag_index_key(tag: &str) -> String {
    format!("tag:{tag}")
}

fn all_facts_key() -> String {
    "facts:all".to_string()
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();

    router
        .get_async("/", |_req, _ctx| async {
            Response::ok("nexus-gateway-nex-cf alive")
        })
        // POST /ingest — fetch search.json, parse, store Facts
        // Optional query param: ?limit=N  — ingest only first N entries
        .post_async("/ingest", |req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let url = req.url()?;

            let limit: Option<usize> = url
                .query_pairs()
                .find(|(k, _)| k == "limit")
                .and_then(|(_, v)| v.parse().ok());

            // Fetch search.json
            let mut resp = Fetch::Url("https://docs.ssccs.org/search.json".parse().unwrap())
                .send()
                .await
                .map_err(|e| Error::RustError(format!("fetch failed: {e}")))?;

            let body = resp
                .text()
                .await
                .map_err(|e| Error::RustError(format!("read body failed: {e}")))?;

            let entries: Vec<SearchEntry> = serde_json::from_str(&body)
                .map_err(|e| Error::RustError(format!("parse failed: {e}")))?;

            // Apply optional limit
            let batch: &[SearchEntry] = match limit {
                Some(n) if n < entries.len() => &entries[..n],
                _ => &entries,
            };

            let now = timestamp();
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

                let stored = StoredFact {
                    id: id.clone(),
                    title: entry.title.clone(),
                    section: entry.section.clone(),
                    tags: tags.clone(),
                    href: entry.href.clone(),
                    text_snippet: if entry.text.len() > 200 {
                        format!("{}...", &entry.text[..200])
                    } else {
                        entry.text.clone()
                    },
                    text_len: entry.text.len(),
                    ingested_at: now.clone(),
                };

                // Store fact
                kv.put(&fact_key(&id), serde_json::to_string(&stored)?)?
                    .execute()
                    .await?;

                // Update tag index
                for tag in &tags {
                    if !tag.is_empty() {
                        let index_key = tag_index_key(tag);
                        let mut ids: Vec<String> = kv
                            .get(&index_key)
                            .text()
                            .await?
                            .and_then(|v| serde_json::from_str(&v).ok())
                            .unwrap_or_default();
                        if !ids.contains(&id) {
                            ids.push(id.clone());
                            kv.put(&index_key, serde_json::to_string(&ids)?)?
                                .execute()
                                .await?;
                        }
                    }
                }

                // Update all-facts index
                let mut all_ids: Vec<String> = kv
                    .get(&all_facts_key())
                    .text()
                    .await?
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default();
                if !all_ids.contains(&id) {
                    all_ids.push(id);
                    kv.put(&all_facts_key(), serde_json::to_string(&all_ids)?)?
                        .execute()
                        .await?;
                }

                count += 1;
            }

            Response::from_json(&serde_json::json!({
                "ingested": count,
                "source": "https://docs.ssccs.org/search.json",
            }))
        })
        // GET /facts — list all Facts, optionally filtered by ?tag=
        .get_async("/facts", |req, ctx| async move {
            let kv = ctx.kv("FIH_KV")?;
            let url = req.url()?;

            // Check for ?tag= filter
            let tag_param: Option<String> = url
                .query_pairs()
                .find(|(k, _)| k == "tag")
                .map(|(_, v)| v.to_string());

            let ids: Vec<String> = if let Some(ref tag) = tag_param {
                kv.get(&tag_index_key(tag))
                    .text()
                    .await?
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default()
            } else {
                kv.get(&all_facts_key())
                    .text()
                    .await?
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default()
            };

            let mut facts = Vec::with_capacity(ids.len());
            for id in &ids {
                if let Some(raw) = kv.get(&fact_key(id)).text().await? {
                    if let Ok(fact) = serde_json::from_str::<StoredFact>(&raw) {
                        facts.push(fact);
                    }
                }
            }

            Response::from_json(&serde_json::json!({
                "tag": tag_param,
                "count": facts.len(),
                "facts": facts,
            }))
        })
        // GET /facts/:id — get a single Fact
        .get_async("/facts/:id", |_req, ctx| async move {
            let id = ctx.param("id").map_or("", |v| v.as_str()).to_string();
            let kv = ctx.kv("FIH_KV")?;

            match kv.get(&fact_key(&id)).text().await? {
                Some(raw) => {
                    match serde_json::from_str::<StoredFact>(&raw) {
                        Ok(fact) => Response::from_json(&fact),
                        Err(e) => Response::error(format!("deserialize: {e}"), 500),
                    }
                }
                None => Response::error("not found", 404),
            }
        })
        .run(req, env)
        .await
}

fn timestamp() -> String {
    Date::now().as_millis().to_string()
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
