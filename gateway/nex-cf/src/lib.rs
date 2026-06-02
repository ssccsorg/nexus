// gateway/nex-cf — CF Worker scaffold.
//
// Minimal worker verifying WASM build + CF deployment.
// No storage logic, no routing. Just a health endpoint
// to confirm the worker is alive.
//
// Depends only on `worker`. Zero dependency on `nex`,
// `petgraph`, `interface-cypher`, or `nexus-model`.

use worker::*;
use worker::DurableObject;

#[event(fetch)]
pub async fn main(_req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    Response::ok("nexus-gateway-nex-cf alive")
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
