// CF Worker environment bindings for the nexus-gateway-nex-cf worker.
//
// This module provides typed access to KV, R2, and Durable Object bindings
// declared in wrangler.jsonc. Each binding is read once from the Env on
// worker startup and cached.

use worker::*;

// ── Binding names (must match wrangler.jsonc) ────────────────────────────

const KV_BINDING: &str = "FIH_KV";
const R2_BINDING: &str = "FIH_R2";
const DO_BINDING: &str = "INTENT_DO";

// ── Environment handle ───────────────────────────────────────────────────

/// Wraps all CF Worker bindings in a single cloneable handle.
///
/// Each method returns a fresh binding handle on every call — appropriate for
/// stateless use in a single-threaded Workers runtime. If performance
/// profiling reveals overhead, the bindings can be cached behind
/// `wasm-bindgen` references.
#[derive(Clone)]
pub struct WorkerEnv {
    env: Env,
}

impl WorkerEnv {
    pub fn new(env: &Env) -> Self {
        Self { env: env.clone() }
    }

    // ── KV ──────────────────────────────────────────────────────────────

    pub fn kv(&self) -> Result<KvStore> {
        self.env.kv(KV_BINDING)
    }

    // ── R2 ──────────────────────────────────────────────────────────────

    pub fn r2(&self) -> Result<Bucket> {
        self.env.bucket(R2_BINDING)
    }

    // ── Durable Object stub ─────────────────────────────────────────────

    pub fn intent_do_stub(&self, id: &str) -> Result<Stub> {
        let namespace = self.env.durable_object(DO_BINDING)?;
        let do_id = namespace.id_from_string(id)?;
        do_id.get_stub()
    }
}
