// gateway/nex-cf — FIH directly on R2. Every read/write goes to R2.
// No locks, no DO, no isolate issues — R2 is the single source of truth.

use worker::*;

#[derive(serde::Serialize, serde::Deserialize)]
struct FactRecord { id: String, origin: String, creator: String, submitted_at: u64, }
#[derive(serde::Serialize, serde::Deserialize)]
struct IntentRecord { id: String, from_facts: Vec<String>, creator: String, status: String, created_at: u64, }

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let bucket = env.bucket("FIH_R2")?;
    let url = req.url()?;
    let path = url.path().to_string();
    let q: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let qv = |k: &str| q.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();

    match path.as_str() {
        "/" => Response::ok("nexus-cf (R2 direct)"),

        "/fact" => {
            let id = qv("id");
            let rec = FactRecord { id: id.clone(), origin: qv("origin"), creator: qv("creator"), submitted_at: 0 };
            let bytes = postcard::to_allocvec(&rec).map_err(|e| Error::RustError(e.to_string()))?;
            bucket.put(&format!("facts/f_{}.fact", id), Data::Bytes(bytes)).execute().await
                .map_err(|e| Error::RustError(e.to_string()))?;
            Response::from_json(&serde_json::json!({"id": id}))
        }

        "/intent" => {
            let id = qv("id");
            let from: Vec<String> = qv("from").split(',').filter(|s| !s.is_empty()).map(String::from).collect();
            let rec = IntentRecord { id: id.clone(), from_facts: from, creator: qv("creator"), status: "submitted".into(), created_at: 0 };
            let bytes = postcard::to_allocvec(&rec).map_err(|e| Error::RustError(e.to_string()))?;
            bucket.put(&format!("intents/i_{}.intent", id), Data::Bytes(bytes)).execute().await
                .map_err(|e| Error::RustError(e.to_string()))?;
            Response::from_json(&serde_json::json!({"status":"ok"}))
        }

        "/state" => {
            let mut facts = Vec::new();
            let mut objects = bucket.list().prefix("facts/").execute().await
                .map_err(|e| Error::RustError(e.to_string()))?;
            loop {
                for obj in objects.objects() {
                    if let Some(o) = bucket.get(&obj.key()).execute().await
                        .map_err(|e| Error::RustError(e.to_string()))? {
                        if let Some(body) = o.body() {
                            if let Ok(bytes) = body.bytes().await {
                                if let Ok(r) = postcard::from_bytes::<FactRecord>(&bytes) {
                                    facts.push(serde_json::json!({"id": r.id, "origin": r.origin}));
                                }
                            }
                        }
                    }
                }
                if !objects.truncated() { break; }
                objects = bucket.list().prefix("facts/").cursor(objects.cursor().unwrap()).execute().await
                    .map_err(|e| Error::RustError(e.to_string()))?;
            }
            Response::from_json(&serde_json::json!({"facts": facts}))
        }

        _ => Response::error("not found", 404),
    }
}
