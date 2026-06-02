// gateway/nex-cf — CF Worker scaffold.
//
// Minimal worker verifying WASM build + CF deployment. KV-backed temp
// data storage for basic read/write verification. IntentClaimDO is an
// empty DO stub for future CAS coordination.
//
// Depends only on `worker` + `nexus-model` (types). Zero dependency on
// `nex`, `petgraph`, or `interface-cypher`.

use serde::Deserialize;
use worker::*;
use worker::DurableObject;

// ── Endpoints ────────────────────────────────────────────────────────────

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/health", |_req, _ctx| async {
            Response::from_json(&serde_json::json!({"status": "ok"}))
        })
        .post_async("/kv/put", |mut req, ctx| async move {
            let body: KvPutRequest = req.json().await?;
            let kv = ctx.kv("FIH_KV")?;
            kv.put(&body.key, body.value)?.execute().await?;
            Response::ok("stored")
        })
        .get_async("/kv/get", |req, ctx| async move {
            let url = req.url()?;
            let key = url
                .query_pairs()
                .find(|(k, _)| k == "key")
                .map(|(_, v)| v.to_string())
                .unwrap_or_default();
            let kv = ctx.kv("FIH_KV")?;
            match kv.get(&key).text().await? {
                Some(v) => Response::ok(v),
                None => Response::error("not found", 404),
            }
        })
        .run(req, env)
        .await
}

#[derive(Deserialize)]
struct KvPutRequest {
    key: String,
    value: String,
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
