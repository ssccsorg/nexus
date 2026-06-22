// CF Worker environment bindings for nexus-gateway-nex-cf.
//
// Currently the worker handlers use ctx.kv("FIH_KV") and
// ctx.durable_object("INTENT_DO") directly (worker-rs 0.8 pattern).
// This module exists as a typed wrapper placeholder for R2 blob
// content operations and future binding abstractions.

use worker::*;

#[derive(Clone)]
pub struct WorkerEnv {
    env: Env,
}

impl WorkerEnv {
    pub fn new(env: &Env) -> Self {
        Self { env: env.clone() }
    }

    pub fn kv(&self, binding: &str) -> Result<KvStore> {
        self.env.kv(binding)
    }

    pub fn r2(&self) -> Result<Bucket> {
        self.env.bucket("FIH_R2")
    }

    pub fn intent_do(&self, id: &str) -> Result<Stub> {
        let namespace = self.env.durable_object("INTENT_DO")?;
        let do_id = namespace.id_from_string(id)?;
        do_id.get_stub()
    }
}
