// src/engines/lightrag.ts
// LightRAG engine handler
//
// LightRAG API reference: https://github.com/HKUDS/LightRAG
//
// Key difference from EdgeQuake: LightRAG processes uploads asynchronously and
// returns a track_id (not a doc_id) on upload. The actual doc_id is assigned
// after background processing completes.
//
// Strategy: Use file_path (R2 key) as the stable identifier in KV mapping,
// since LightRAG documents are keyed by file_path. On each sync cycle,
// listDocuments() resolves file_path → doc_id for deletion operations.
//
// LightRAG API endpoints used:
//   GET  /documents              – list all documents grouped by status
//   POST /documents/upload       – upload a file (multipart/form-data)
//   POST /documents/delete       – delete documents by doc_ids
//   POST /documents/text         – insert text (used when raw text is available)
//   POST /documents/reprocess_failed – retry failed/pending documents

import type { Env } from "../index";

export class LightRagHandler {
  private base(env: Env): string {
    return env.LIGHTRAG_API_HOST;
  }

  private headers(env: Env): HeadersInit {
    const h: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (env.LIGHTRAG_API_KEY) {
      h["X-API-Key"] = env.LIGHTRAG_API_KEY;
    }
    return h;
  }

  // -------------------------------------------------------------------------
  // listDocuments – flatten all status groups into a unified list
  // -------------------------------------------------------------------------
  async listDocuments(env: Env): Promise<Array<{ id: string; title: string }>> {
    const url = `${this.base(env)}/documents`;

    const res = await fetch(url, {
      method: "GET",
      headers: this.headers(env),
    });

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(
        `LightRAG list failed: ${res.status} ${res.statusText} — ${body.slice(0, 300)}`,
      );
    }

    // LightRAG returns: { statuses: { "PROCESSED": [...], "PENDING": [...], ... } }
    const data = (await res.json()) as {
      statuses: Record<
        string,
        Array<{
          id: string;
          content_summary: string;
          content_length: number;
          status: string;
          created_at: string;
          updated_at: string;
          track_id: string;
          file_path: string;
          error?: string;
        }>
      >;
    };

    const results: Array<{ id: string; title: string }> = [];
    if (!data.statuses) return results;

    // Build a dedup map by file_path (the canonical key)
    const seen = new Set<string>();
    for (const statusDocs of Object.values(data.statuses)) {
      for (const doc of statusDocs) {
        // Use file_path as the primary key; fall back to id for text inserts
        const key = doc.file_path || doc.id;
        if (!seen.has(key)) {
          seen.add(key);
          results.push({
            id: key, // file_path is the stable identifier matching R2 keys
            title: doc.content_summary?.slice(0, 120) || key,
          });
        }
      }
    }

    return results;
  }

  // -------------------------------------------------------------------------
  // deleteDocument – resolve file_path/key → doc_id, then delete
  // -------------------------------------------------------------------------
  async deleteDocument(id: string, env: Env): Promise<void> {
    // LightRAG uses doc_ids (UUIDs) for deletion, but our KV stores file_path.
    // We first look up the actual doc_id by file_path from the current listing,
    // then issue the delete. If the id is already a UUID (from a previous
    // resolution), we try it directly first.

    // Fast path: try direct delete if `id` looks like a doc_id (UUID)
    const uuidPattern =
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
    if (uuidPattern.test(id)) {
      try {
        await this.deleteByIds([id], env);
        return;
      } catch {
        // Fall through to file_path lookup
      }
    }

    // Slow path: resolve file_path → doc_id
    const filePath = id;
    const url = `${this.base(env)}/documents`;
    const res = await fetch(url, {
      method: "GET",
      headers: this.headers(env),
    });

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(
        `LightRAG list (for delete lookup) failed: ${res.status} — ${body.slice(0, 200)}`,
      );
    }

    const data = (await res.json()) as {
      statuses: Record<string, Array<{ id: string; file_path: string }>>;
    };

    // Find the doc_id matching this file_path
    const docId = this.findDocIdByFilePath(data.statuses, filePath);
    if (!docId) {
      console.warn(
        `[lightrag] deleteDocument: no document found for file_path="${filePath}", skipping`,
      );
      return;
    }

    await this.deleteByIds([docId], env);
  }

  private findDocIdByFilePath(
    statuses: Record<string, Array<{ id: string; file_path: string }>>,
    filePath: string,
  ): string | null {
    for (const docs of Object.values(statuses)) {
      for (const doc of docs) {
        if (doc.file_path === filePath) {
          return doc.id;
        }
      }
    }
    return null;
  }

  // -------------------------------------------------------------------------
  // deleteByIds – raw DELETE call to LightRAG
  // -------------------------------------------------------------------------
  private async deleteByIds(docIds: string[], env: Env): Promise<void> {
    const url = `${this.base(env)}/documents`;
    const body = JSON.stringify({
      doc_ids: docIds,
      delete_file: false,
      delete_llm_cache: true,
    });

    const res = await fetch(url, {
      method: "DELETE",
      headers: {
        ...this.headers(env),
        "Content-Type": "application/json",
      },
      body,
    });

    if (!res.ok) {
      const text = await res.text().catch(() => "");
      throw new Error(
        `LightRAG delete failed: ${res.status} ${res.statusText} — ${text.slice(0, 200)}`,
      );
    }
  }

  // -------------------------------------------------------------------------
  // uploadDocument – upload via /documents/text with explicit file_path
  //
  // We use /documents/text instead of /documents/upload because
  // FormData strips directory separators from filenames (R2 key
  // "foo/bar.txt" → file_path "foobar.txt"), which breaks KV mapping.
  // /documents/text accepts an explicit file_path alongside the text.
  // -------------------------------------------------------------------------
  async uploadDocument(
    key: string,
    buffer: ArrayBuffer,
    env: Env,
  ): Promise<string> {
    const url = `${this.base(env)}/documents/text`;
    const text = new TextDecoder().decode(buffer);

    const body = JSON.stringify({
      text,
      file_source: key, // preserve full R2 key including directory separators
    });

    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (env.LIGHTRAG_API_KEY) {
      headers["X-API-Key"] = env.LIGHTRAG_API_KEY;
    }

    const res = await fetch(url, {
      method: "POST",
      headers,
      body,
    });

    if (!res.ok) {
      const bodyText = await res.text().catch(() => "");
      throw new Error(
        `LightRAG upload failed: ${res.status} ${res.statusText} — ${bodyText.slice(0, 300)}`,
      );
    }

    const data = (await res.json()) as {
      status: string;
      message: string;
      track_id: string;
    };

    if (data.status === "duplicated") {
      console.warn(
        `[lightrag] upload: ${key} → duplicate (skipped), message: ${data.message}`,
      );
    } else if (data.status !== "success") {
      console.warn(
        `[lightrag] upload: ${key} → status=${data.status}, message: ${data.message}`,
      );
    }

    // Return the R2 key as the document_id for KV mapping.
    // LightRAG returns a track_id, but the actual doc_id is assigned
    // asynchronously. Using the file_path (key) as the stable identifier
    // allows listDocuments() to resolve it on the next sync cycle.
    return key;
  }

  // -------------------------------------------------------------------------
  // uploadDocuments – batch upload (sequential, LightRAG has no batch endpoint)
  // -------------------------------------------------------------------------
  async uploadDocuments(
    files: Array<{ key: string; buffer: ArrayBuffer }>,
    env: Env,
  ): Promise<Array<{ key: string; document_id: string }>> {
    const results: Array<{ key: string; document_id: string }> = [];

    // LightRAG does not have a batch upload endpoint, so we upload
    // sequentially with minimal concurrency to avoid overwhelming the server.
    const CONCURRENCY = 3;
    let idx = 0;

    const worker = async (): Promise<void> => {
      while (idx < files.length) {
        const i = idx++;
        const f = files[i];
        if (!f) continue;
        try {
          const document_id = await this.uploadDocument(f.key, f.buffer, env);
          results.push({ key: f.key, document_id });
          console.log(
            `[lightrag] batch uploaded ${f.key} → doc_id=${document_id}`,
          );
        } catch (e) {
          console.error(`[lightrag] batch upload failed: ${f.key}`, e);
        }
      }
    };

    // Launch concurrent workers
    const workers = Array.from(
      { length: Math.min(CONCURRENCY, files.length) },
      () => worker(),
    );
    await Promise.all(workers);

    return results;
  }

  // -------------------------------------------------------------------------
  // reprocessFailedDocuments – retry failed/pending documents
  // -------------------------------------------------------------------------
  async reprocessFailedDocuments(env: Env): Promise<void> {
    const url = `${this.base(env)}/documents/reprocess_failed`;

    const res = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(env.LIGHTRAG_API_KEY
          ? { "X-API-Key": env.LIGHTRAG_API_KEY }
          : {}),
      },
    });

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(
        `LightRAG reprocess failed: ${res.status} ${res.statusText} — ${body.slice(0, 300)}`,
      );
    }

    const data = (await res.json()) as {
      status: string;
      message: string;
    };

    console.log(
      `[lightrag] reprocessFailedDocuments: status=${data.status}, message="${data.message}"`,
    );
  }
}
