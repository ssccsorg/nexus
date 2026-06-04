using Workerd = import "/workerd/workerd.capnp";

const config :Workerd.Config = (
  services = [
    (name = "nexus-gateway-nex-cf", worker = (
      modules = [
        (name = "worker", esModule = embed "../build/worker/shim.mjs"),
        (name = "index.wasm", wasm = embed "../build/worker/nexus_gateway_nex_cf_bg.wasm"),
      ],
      compatibilityDate = "2026-05-01",
      compatibilityFlags = ["nodejs_compat"],
    )),
  ],
  sockets = [
    (name = "main", address = "*:8769", http = (), service = "nexus-gateway-nex-cf"),
  ],
);
