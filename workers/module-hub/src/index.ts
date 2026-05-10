// src/index.ts
// Nexus Module Hub — the DJ mixing desk.
//
// Endpoints:
//   GET  /modules              — list all registered modules with manifests
//   POST /modules/run          — run one or more modules, return findings
//   GET  /modules/findings     — retrieve persisted findings by module name
//
// Each module is a self-contained cognitive function. Modules communicate
// through the shared knowledge space, not by calling each other directly.

import { runModules, listModules, runModule } from "./modules/registry";
import "./modules/gap-detector";
import type { ModuleContext } from "./modules/types";

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------
export interface Env {
  MODULE_HUB_KV: KVNamespace;
  ARTIFACT_BUCKET: R2Bucket;
  AI: any;
  MODULE_HUB_API_KEY: string;
  // Optional: Memgraph (for future Cypher-based analysis)
  MEMGRAPH_API_HOST?: string;
  MEMGRAPH_API_KEY?: string;
}

// ---------------------------------------------------------------------------
// Build runtime context from Env
// ---------------------------------------------------------------------------
function buildContext(env: Env): ModuleContext {
  return {
    kv: env.MODULE_HUB_KV,
    bucket: env.ARTIFACT_BUCKET,
    ai: env.AI,
    env: {
      MEMGRAPH_API_HOST: env.MEMGRAPH_API_HOST || "",
      MEMGRAPH_API_KEY: env.MEMGRAPH_API_KEY || "",
    },
  };
}

// ---------------------------------------------------------------------------
// Auth guard
// ---------------------------------------------------------------------------
function authorize(request: Request, env: Env): boolean {
  const auth = request.headers.get("Authorization");
  const expected = `Bearer ${env.MODULE_HUB_API_KEY}`;
  return auth === expected;
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------
function json(data: any, status: number = 200): Response {
  return new Response(JSON.stringify(data, null, 2), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

// ---------------------------------------------------------------------------
// Worker entry
// ---------------------------------------------------------------------------
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const method = request.method;

    // Route: GET /modules
    if (method === "GET" && url.pathname === "/modules") {
      const modules = listModules();
      return json({ modules });
    }

    // Route: POST /modules/run
    if (method === "POST" && url.pathname === "/modules/run") {
      if (!authorize(request, env)) {
        return json({ error: "Unauthorized" }, 401);
      }

      const body = (await request.json()) as {
        modules?: string[];
      } || {};

      const ctx = buildContext(env);
      let results;

      if (body.modules && body.modules.length > 0) {
        results = await runModules(body.modules, ctx);
      } else {
        // Run all registered modules
        const allModules = listModules();
        results = await runModules(
          allModules.map((m) => m.key),
          ctx,
        );
      }

      return json({ results });
    }

    // Route: GET /modules/findings?module=gap-detector
    if (method === "GET" && url.pathname === "/modules/findings") {
      const moduleName = url.searchParams.get("module");
      if (!moduleName) {
        return json({ error: "Query parameter 'module' is required" }, 400);
      }

      // List all findings keys for this module from KV
      const listResult = await env.MODULE_HUB_KV.list({
        prefix: `findings:${moduleName}:`,
      });

      const findings: any[] = [];
      for (const key of listResult.keys) {
        const raw = await env.MODULE_HUB_KV.get(key.name);
        if (raw) {
          findings.push(JSON.parse(raw));
        }
      }

      return json({
        module: moduleName,
        runCount: findings.length,
        findings,
      });
    }

    // Fallback
    return json({ error: "Not Found" }, 404);
  },
};
