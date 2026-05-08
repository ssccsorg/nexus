// src/index.ts
// Nexus Sync Worker – /sync/:engine pattern
// Supports: LightRAG (/sync/lr) – primary, EdgeQuake (/sync/eq) – deprecated
//
// Recursively scans ALL objects in the R2 bucket (paginated, no prefix filter)
// and syncs them to the configured engine. Each sync run diffs the current R2
// state against the previous KV mapping, then enqueues upload/delete tasks.

import { EdgeQuakeHandler } from "./engines/edgequake";
import { LightRagHandler } from "./engines/lightrag";
import { GraphitiHandler } from "./engines/graphiti";

// ---------------------------------------------------------------------------
// Shared environment type (used by all engine handlers)
// ---------------------------------------------------------------------------
export interface Env {
  ARTIFACT_BUCKET: R2Bucket;
  SYNC_KV: KVNamespace;
  SYNC_QUEUE: Queue;
  SYNC_API_KEY: string;
  // EdgeQuake (deprecated – kept for backward compat)
  EDGEQUAKE_API_HOST: string;
  EDGEQUAKE_TENANT_ID: string;
  EDGEQUAKE_API_KEY: string;
  WORKSPACE_ID: string;
  // LightRAG (primary engine)
  LIGHTRAG_API_HOST: string;
  LIGHTRAG_API_KEY: string;
  // Graphiti (temporal knowledge graph)
  GRAPHITI_API_HOST: string;
  GRAPHITI_API_KEY: string;
}

// ---------------------------------------------------------------------------
// Engine Handler Interface (extend for new engines)
// ---------------------------------------------------------------------------
export interface EngineHandler {
  /** Sync strategy:
   *  "sync" — overwrite: diff R2 vs KV → delete old + upload new + reprocess failed
   *  "put"  — cumulative: scan R2 → upload new/changed only (no delete, no reprocess) */
  readonly strategy: "sync" | "put";
  /** List all indexed documents in the engine */
  listDocuments(env: Env): Promise<Array<{ id: string; title: string }>>;
  /** Delete a document by engine-native ID */
  deleteDocument(id: string, env: Env): Promise<void>;
  /** Upload a document; returns the engine-native ID assigned */
  uploadDocument(key: string, buffer: ArrayBuffer, env: Env): Promise<string>;
  /** Upload multiple documents in a single batch request (optional).
   *  Returns only successfully uploaded files with their document IDs. */
  uploadDocuments?(
    files: Array<{ key: string; buffer: ArrayBuffer }>,
    env: Env,
  ): Promise<Array<{ key: string; document_id: string }>>;
  /** Reprocess failed/pending documents in the engine (optional, "sync" only).
   *  Called after sync completes to retry stuck documents. */
  reprocessFailedDocuments?(env: Env): Promise<void>;
}

// ---------------------------------------------------------------------------
// Engine Registry – register new engines here
// ---------------------------------------------------------------------------
const engines: Record<string, EngineHandler> = {
  lr: new LightRagHandler(), // LightRAG (primary)
  eq: new EdgeQuakeHandler(), // EdgeQuake (deprecated – stability issues)
  gt: new GraphitiHandler(), // Graphiti (temporal knowledge graph)
  // auto: new AutoRAGHandler(),       // Cloudflare AI Search (future)
  // local: new LocalHandler(),        // Local / custom engine (future)
};

// ---------------------------------------------------------------------------
// Auth guard
// ---------------------------------------------------------------------------
function authorize(request: Request, env: Env): boolean {
  const auth = request.headers.get("Authorization");
  const expected = `Bearer ${env.SYNC_API_KEY}`;
  return auth === expected;
}

// ---------------------------------------------------------------------------
// HTTP fetch handler – route /sync/:engine
// ---------------------------------------------------------------------------
export default {
  async fetch(
    request: Request,
    env: Env,
    ctx: ExecutionContext,
  ): Promise<Response> {
    const url = new URL(request.url);

    // URLPattern matching for /sync/:engine
    const syncPattern = new URLPattern({ pathname: "/sync/:engine" });
    const match = syncPattern.exec(url);

    if (!match) {
      return new Response("Not Found", { status: 404 });
    }

    // Auth check
    if (!authorize(request, env)) {
      return new Response("Unauthorized", { status: 401 });
    }

    const engineName = match.pathname.groups["engine"]!;

    // Deprecation notice for EdgeQuake
    if (engineName === "eq") {
      console.warn(
        "[deprecated] /sync/eq is deprecated due to EdgeQuake stability issues. " +
          "Use /sync/lr for LightRAG instead.",
      );
    }

    const handler = engines[engineName];

    if (!handler) {
      return new Response(
        JSON.stringify({
          error: "Unknown engine",
          supported: Object.keys(engines),
          deprecated: ["eq"],
          recommended: "lr",
        }),
        { status: 400, headers: { "Content-Type": "application/json" } },
      );
    }

    // Dispatch to the sync orchestrator
    ctx.waitUntil(runSync(engineName, env));

    return new Response(
      JSON.stringify({ status: "accepted", engine: engineName }),
      { status: 202, headers: { "Content-Type": "application/json" } },
    );
  },

  async queue(
    batch: MessageBatch<{ chunk: SyncTask[]; engine: string }>,
    env: Env,
  ): Promise<void> {
    // Collect all updates from this batch to apply atomically at the end
    const updates: Record<string, { doc_id: string; etag: string }> = {};
    const deletedKeys: string[] = [];
    let engineName = "";

    for (const msg of batch.messages) {
      const { chunk, engine } = msg.body;
      engineName = engine;
      const handler = engines[engineName];
      if (!handler) {
        msg.ack();
        continue;
      }

      // Separate deletes and uploads
      const deleteTasks = chunk.filter((t) => t.type === "delete" && t.id);
      const uploadTasks = chunk.filter((t) => t.type === "upload" && t.key);

      // Process deletes individually
      for (const task of deleteTasks) {
        try {
          console.log(
            `[${engineName}] deleting doc ${task.id} (R2 key: ${task.key || "unknown"})`,
          );
          await handler.deleteDocument(task.id!, env);
          if (task.key) deletedKeys.push(task.key);
        } catch (e) {
          console.error(
            `[${engineName}] delete failed: id=${task.id} key=${task.key}`,
            e,
          );
        }
      }

      // Process uploads — use batch endpoint if the engine supports it
      if (uploadTasks.length > 0) {
        try {
          if (handler.uploadDocuments) {
            // Batch mode: fetch all buffers, upload in one request
            const files: Array<{ key: string; buffer: ArrayBuffer }> = [];
            for (const task of uploadTasks) {
              const obj = await env.ARTIFACT_BUCKET.get(task.key!);
              if (!obj) {
                console.warn(
                  `[${engineName}] R2 object not found: ${task.key}`,
                );
                continue;
              }
              const buffer = await obj.arrayBuffer();
              files.push({ key: task.key!, buffer });
            }

            if (files.length > 0) {
              console.log(
                `[${engineName}] batch uploading ${files.length} files...`,
              );
              const uploaded = await handler.uploadDocuments(files, env);
              for (const { key, document_id } of uploaded) {
                const head = await env.ARTIFACT_BUCKET.head(key);
                updates[key] = { doc_id: document_id, etag: head?.etag ?? "" };
                console.log(
                  `[${engineName}] uploaded ${key} → doc_id=${document_id}`,
                );
              }
            }
          } else {
            // Fallback: upload one by one
            for (const task of uploadTasks) {
              console.log(`[${engineName}] uploading ${task.key}...`);
              const obj = await env.ARTIFACT_BUCKET.get(task.key!);
              if (!obj) {
                console.warn(
                  `[${engineName}] R2 object not found: ${task.key}`,
                );
                continue;
              }
              const buffer = await obj.arrayBuffer();
              const newId = await handler.uploadDocument(
                task.key!,
                buffer,
                env,
              );
              const head = await env.ARTIFACT_BUCKET.head(task.key!);
              updates[task.key!] = { doc_id: newId, etag: head?.etag ?? "" };
              console.log(
                `[${engineName}] uploaded ${task.key} → doc_id=${newId}`,
              );
            }
          }
        } catch (e) {
          console.error(`[${engineName}] upload batch/single failed`, e);
        }
      }
      msg.ack();
    }

    // Apply mapping updates in one write (still eventual consistent but reduces within-batch races)
    if (Object.keys(updates).length > 0 || deletedKeys.length > 0) {
      const MAPPING_KEY = `mapping:${engineName}`;
      const current =
        ((await env.SYNC_KV.get(MAPPING_KEY, "json")) as Record<
          string,
          { doc_id: string; etag: string }
        >) || {};
      for (const [key, val] of Object.entries(updates)) {
        current[key] = val;
      }
      // Remove keys that were deleted from R2
      for (const key of deletedKeys) {
        delete current[key];
      }
      await env.SYNC_KV.put(`mapping:${engineName}`, JSON.stringify(current));
    }
  },
};

// ---------------------------------------------------------------------------
// Sync orchestrator (Producer → Queue → Consumer)
// ---------------------------------------------------------------------------
async function runSync(engineName: string, env: Env): Promise<void> {
  const CHUNK_SIZE = 50; // ≤50 tasks per message to stay under subrequest limit

  const handler = engines[engineName];
  if (!handler) {
    console.error(`[${engineName}] unknown engine, aborting sync`);
    return;
  }

  const strategy = handler.strategy;

  if (strategy === "sync") {
    await runSyncStrategy(engineName, env, handler, CHUNK_SIZE);
  } else if (strategy === "put") {
    await runPutStrategy(engineName, env, handler, CHUNK_SIZE);
  }
}

// ---------------------------------------------------------------------------
// "sync" strategy — overwrite: diff → delete old + upload new + reprocess
// Used by: LightRAG, EdgeQuake
// ---------------------------------------------------------------------------
async function runSyncStrategy(
  engineName: string,
  env: Env,
  handler: EngineHandler,
  CHUNK_SIZE: number,
): Promise<void> {
  // 1. R2 inventory – recursively list ALL objects in the bucket (paginated)
  const r2Map = new Map<string, string>();
  let cursor: string | undefined;
  do {
    const opts: R2ListOptions = { limit: 1000 };
    if (cursor) {
      opts.cursor = cursor;
    }
    const result = await env.ARTIFACT_BUCKET.list(opts);
    for (const obj of result.objects) {
      r2Map.set(obj.key, obj.etag);
    }
    cursor = result.truncated ? result.cursor : undefined;
  } while (cursor);
  console.log(
    `[${engineName}] found ${r2Map.size} objects in R2 bucket (full recursive scan)`,
  );

  // 2. Previous mapping (KV) — per-engine namespace to avoid cross-contamination
  const MAPPING_KEY = `mapping:${engineName}`;
  const prev: Record<string, { doc_id: string; etag: string }> =
    (await env.SYNC_KV.get(MAPPING_KEY, "json")) || {};
  console.log(
    `[${engineName}] previous mapping has ${Object.keys(prev).length} entries`,
  );

  // 3. Engine inventory — list actual documents to detect drift
  //    (e.g., engine data wiped while KV mapping still looks healthy)
  let engineListOk = false;
  const engineDocIds = new Set<string>();
  try {
    const engineDocs = await handler.listDocuments(env);
    for (const doc of engineDocs) {
      engineDocIds.add(doc.id);
    }
    engineListOk = true;
    console.log(`[${engineName}] engine has ${engineDocIds.size} documents`);
  } catch (e) {
    console.warn(
      `[${engineName}] could not list engine documents, skipping drift check:`,
      e,
    );
  }

  // 4. Diff
  //    - Keys in prev but not in R2 → delete from engine
  //    - Keys in R2 but not in prev → upload
  //    - Keys in both but etag differs → delete old + upload new
  //    - Keys in both with same etag but NOT in engine → upload (drift)
  const tasks: Array<{ type: "delete" | "upload"; id?: string; key?: string }> =
    [];
  for (const [key, p] of Object.entries(prev)) {
    if (!r2Map.has(key)) {
      tasks.push({ type: "delete", id: p.doc_id, key });
    }
  }
  for (const [key, etag] of r2Map) {
    const p = prev[key];
    if (!p || p.etag !== etag) {
      if (p) {
        tasks.push({ type: "delete", id: p.doc_id, key });
      }
      tasks.push({ type: "upload", key });
    }
  }
  // Drift check: KV says synced but engine doesn't have it
  if (engineListOk) {
    for (const [key, p] of Object.entries(prev)) {
      if (
        r2Map.has(key) &&
        r2Map.get(key) === p.etag &&
        !engineDocIds.has(p.doc_id)
      ) {
        tasks.push({ type: "upload", key });
      }
    }
  }
  console.log(
    `[${engineName}] sync plan: ${tasks.length} tasks (${tasks.filter((t) => t.type === "delete").length} deletes, ${tasks.filter((t) => t.type === "upload").length} uploads)`,
  );

  // 5. Enqueue chunks
  for (let i = 0; i < tasks.length; i += CHUNK_SIZE) {
    const chunk = tasks.slice(i, i + CHUNK_SIZE);
    await env.SYNC_QUEUE.send({ chunk, engine: engineName });
  }

  // 6. Reprocess failed/pending documents in the engine
  //    (e.g. documents stuck in FAILED state after transient errors)
  if (handler.reprocessFailedDocuments) {
    try {
      console.log(`[${engineName}] reprocessing failed/pending documents...`);
      await handler.reprocessFailedDocuments(env);
      console.log(`[${engineName}] reprocess initiated successfully`);
    } catch (e) {
      console.warn(
        `[${engineName}] reprocess failed documents returned an error:`,
        e,
      );
    }
  }
}

// ---------------------------------------------------------------------------
// "put" strategy — cumulative: upload new/changed only, never delete
// Used by: Graphiti (temporal knowledge graph)
// ---------------------------------------------------------------------------
async function runPutStrategy(
  engineName: string,
  env: Env,
  _handler: EngineHandler, // reserved for future use
  CHUNK_SIZE: number,
): Promise<void> {
  // 1. R2 inventory
  const r2Map = new Map<string, string>();
  let cursor: string | undefined;
  do {
    const opts: R2ListOptions = { limit: 1000 };
    if (cursor) {
      opts.cursor = cursor;
    }
    const result = await env.ARTIFACT_BUCKET.list(opts);
    for (const obj of result.objects) {
      r2Map.set(obj.key, obj.etag);
    }
    cursor = result.truncated ? result.cursor : undefined;
  } while (cursor);
  console.log(
    `[${engineName}] found ${r2Map.size} objects in R2 bucket`,
  );

  // 2. Previous mapping (KV) — only used to skip already-ingested files
  const MAPPING_KEY = `mapping:${engineName}`;
  const prev: Record<string, { doc_id: string; etag: string }> =
    (await env.SYNC_KV.get(MAPPING_KEY, "json")) || {};
  console.log(
    `[${engineName}] previous mapping has ${Object.keys(prev).length} entries`,
  );

  // 3. Plan — upload new or changed files only (no deletes, no drift check)
  const tasks: Array<{ type: "upload"; key: string }> = [];
  for (const [key, etag] of r2Map) {
    const p = prev[key];
    if (!p || p.etag !== etag) {
      tasks.push({ type: "upload", key });
    }
  }
  console.log(
    `[${engineName}] put plan: ${tasks.length} uploads`,
  );

  // 4. Enqueue chunks
  for (let i = 0; i < tasks.length; i += CHUNK_SIZE) {
    const chunk = tasks.slice(i, i + CHUNK_SIZE);
    await env.SYNC_QUEUE.send({ chunk, engine: engineName });
  }
}

// Types
interface SyncTask {
  type: "delete" | "upload";
  id?: string;
  key?: string;
}
