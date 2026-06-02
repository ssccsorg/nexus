// gateway/nex-cf — CF Worker scaffold.
//
// Minimal worker verifying WASM build + CF deployment.
// KV temp data storage + R2 blob read for end-to-end verification.
//
// Depends only on `worker`. Zero dependency on `nex`,
// `petgraph`, `interface-cypher`, or `nexus-model`.

use worker::*;
use worker::DurableObject;

#[event(fetch)]
pub async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let router = Router::new();

    router
        // Health check
        .get_async("/", |_req, _ctx| async {
            Response::ok("nexus-gateway-nex-cf alive")
        })
        // KV: write a value
        .put_async("/kv/:key", |mut req, ctx| async move {
            let key = ctx.param("key").map_or("", |v| v.as_str()).to_string();
            let value = req.text().await?;
            let kv = ctx.kv("FIH_KV")?;
            kv.put(&key, value)?.execute().await?;
            Response::ok("stored")
        })
        // KV: read a value
        .get_async("/kv/:key", |_req, ctx| async move {
            let key = ctx.param("key").map_or("", |v| v.as_str()).to_string();
            let kv = ctx.kv("FIH_KV")?;
            match kv.get(&key).text().await? {
                Some(v) => Response::ok(v),
                None => Response::error("not found", 404),
            }
        })
        // R2: write a blob
        .put_async("/r2/:key", |mut req, ctx| async move {
            let key = ctx.param("key").map_or("", |v| v.as_str()).to_string();
            let bucket = ctx.bucket("FIH_R2")?;
            let body = req.bytes().await?;
            bucket.put(&key, body).execute().await?;
            Response::ok("stored")
        })
        // R2: read a blob
        .get_async("/r2/:key", |_req, ctx| async move {
            let key = ctx.param("key").map_or("", |v| v.as_str()).to_string();
            let bucket = ctx.bucket("FIH_R2")?;
            match bucket.get(&key).execute().await? {
                Some(obj) => {
                    match obj.body() {
                        Some(body) => {
                            let bytes = body.bytes().await?;
                            Ok(Response::from_bytes(bytes)?)
                        }
                        None => Response::error("body consumed", 500),
                    }
                }
                None => Response::error("not found", 404),
            }
        })
        .run(req, env)
        .await
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
