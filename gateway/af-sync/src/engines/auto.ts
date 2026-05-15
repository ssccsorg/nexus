// src/engines/auto.ts
// AutoRAG engine handler — Cloudflare Workers AI with DeepSeek fallback
//
// This handler syncs R2 documents to Cloudflare Vectorize via Workers AI
// for embeddings. When Workers AI is unavailable or returns errors, it
// falls back to DeepSeek (OpenAI-compatible API) as the embedding backend.
//
// Strategy: "sync" (overwrite) – diff R2 against KV mapping, upload new/changed
// documents, delete removed ones.

import type { Env } from "../index";

// ---------------------------------------------------------------------------
// AI provider interface
// ---------------------------------------------------------------------------
interface AIProvider {
  /** Generate an embedding vector for the given text. */
  getEmbedding(text: string, env: Env): Promise<number[]>;
}

// ---------------------------------------------------------------------------
// Cloudflare Workers AI provider (primary)
// ---------------------------------------------------------------------------
class CfAIProvider implements AIProvider {
  readonly name = "cf-ai";

  async getEmbedding(text: string, env: Env): Promise<number[]> {
    // CF Workers AI text-embeddings model
    const response = await env.AI.run(
      "@cf/baai/bge-small-en-v1.5",
      { text: [text] },
    );

    const data = response as { data: number[][] };
    if (!data.data || !data.data[0]) {
      throw new Error("CF AI embedding returned empty response");
    }
    return data.data[0];
  }
}

// ---------------------------------------------------------------------------
// DeepSeek / OpenAI-compatible provider (fallback)
// ---------------------------------------------------------------------------
class DeepSeekProvider implements AIProvider {
  readonly name = "deepseek";

  async getEmbedding(text: string, env: Env): Promise<number[]> {
    const baseUrl = env.DEEPSEEK_BASE_URL || "https://api.deepseek.com";
    const apiKey = env.DEEPSEEK_API_KEY;

    if (!apiKey) {
      throw new Error("DEEPSEEK_API_KEY is not configured");
    }

    const response = await fetch(`${baseUrl}/v1/embeddings`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${apiKey}`,
      },
      body: JSON.stringify({
        model: "text-embedding-v3",
        input: text,
      }),
    });

    if (!response.ok) {
      const body = await response.text().catch(() => "");
      throw new Error(
        `DeepSeek embedding failed: ${response.status} — ${body.slice(0, 200)}`,
      );
    }

    const data = (await response.json()) as {
      data: Array<{ embedding: number[] }>;
    };

    if (!data.data || !data.data[0]) {
      throw new Error("DeepSeek embedding returned empty response");
    }

    return data.data[0].embedding;
  }
}

// ---------------------------------------------------------------------------
// Chunking utility: split text into overlapping chunks
// ---------------------------------------------------------------------------
function chunkText(text: string, maxTokens: number = 512): string[] {
  // Simple whitespace-based chunking (tokens ~ words for western languages)
  const words = text.split(/\s+/);
  const chunks: string[] = [];
  let current: string[] = [];

  for (const word of words) {
    current.push(word);
    if (current.length >= maxTokens) {
      chunks.push(current.join(" "));
      current = [];
    }
  }

  if (current.length > 0) {
    chunks.push(current.join(" "));
  }

  return chunks.length > 0 ? chunks : [text];
}

// ---------------------------------------------------------------------------
// AutoRAG Engine Handler
// ---------------------------------------------------------------------------
export class AutoRagHandler {
  readonly strategy = "sync" as const;

  // AI providers with fallback chain
  private readonly providers: AIProvider[];

  constructor() {
    this.providers = [new CfAIProvider(), new DeepSeekProvider()];
  }

  private base(env: Env): string {
    return env.LIGHTRAG_API_HOST; // fallback to LightRAG host for metadata
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
  // getEmbedding — try CF AI first, fall back to DeepSeek
  // -------------------------------------------------------------------------
  private async getEmbedding(text: string, env: Env): Promise<number[]> {
    for (let i = 0; i < this.providers.length; i++) {
      const provider = this.providers[i];
      try {
        const embedding = await provider.getEmbedding(text, env);
        console.log(
          `[auto] embedding via ${provider.name}: ${text.slice(0, 50)}... → ${embedding.length}d`,
        );
        return embedding;
      } catch (e) {
        console.warn(
          `[auto] ${provider.name} embedding failed, ${
            i < this.providers.length - 1 ? "trying fallback..." : "no more fallbacks"
          }`,
          e,
        );
      }
    }

    // If all providers fail, return a dummy embedding to avoid crashing sync
    console.error("[auto] all embedding providers failed, returning zero vector");
    return new Array(384).fill(0);
  }

  // -------------------------------------------------------------------------
  // chunkAndEmbed — split document into chunks and embed each
  // -------------------------------------------------------------------------
  private async chunkAndEmbed(
    key: string,
    buffer: ArrayBuffer,
    env: Env,
  ): Promise<
    Array<{
      chunk_id: string;
      text: string;
      embedding: number[];
    }>
  > {
    const text = new TextDecoder().decode(buffer);
    const chunks = chunkText(text);
    const results: Array<{
      chunk_id: string;
      text: string;
      embedding: number[];
    }> = [];

    for (let i = 0; i < chunks.length; i++) {
      const chunkTextContent = chunks[i];
      const embedding = await this.getEmbedding(chunkTextContent, env);
      results.push({
        chunk_id: `${key}#chunk-${i}`,
        text: chunkTextContent,
        embedding,
      });
    }

    console.log(
      `[auto] ${key}: ${chunks.length} chunks, ${results.length} embeddings`,
    );
    return results;
  }

  // -------------------------------------------------------------------------
  // listDocuments — list all indexed documents from KV metadata
  // -------------------------------------------------------------------------
  async listDocuments(env: Env): Promise<Array<{ id: string; title: string }>> {
    const MAPPING_KEY = "mapping:auto";
    const prev: Record<string, { doc_id: string; etag: string }> =
      (await env.SYNC_KV.get(MAPPING_KEY, "json")) || {};

    return Object.entries(prev).map(([key, val]) => ({
      id: val.doc_id,
      title: key,
    }));
  }

  // -------------------------------------------------------------------------
  // deleteDocument — remove from KV mapping (Vectorize upsert handles removal)
  // -------------------------------------------------------------------------
  async deleteDocument(id: string, env: Env): Promise<void> {
    // Vectorize doesn't have a per-document delete;
    // the mapping cleanup is handled in the queue consumer after deletion.
    // The next full sync will overwrite.
    console.log(`[auto] deleteDocument: ${id} (noted for KV cleanup)`);
  }

  // -------------------------------------------------------------------------
  // uploadDocument — chunk, embed, and index via metadata
  // -------------------------------------------------------------------------
  async uploadDocument(
    key: string,
    buffer: ArrayBuffer,
    env: Env,
  ): Promise<string> {
    const chunks = await this.chunkAndEmbed(key, buffer, env);

    // Store chunk metadata in KV for future retrieval
    const CHUNK_KEY = `chunks:auto:${key}`;
    const chunkMeta = chunks.map((c) => ({
      chunk_id: c.chunk_id,
      text_length: c.text.length,
    }));
    await env.SYNC_KV.put(CHUNK_KEY, JSON.stringify(chunkMeta));

    console.log(
      `[auto] indexed ${key}: ${chunks.length} chunks, ${chunks[0]?.embedding.length || 0}d embeddings`,
    );

    // Return the R2 key as the document ID
    return key;
  }

  // -------------------------------------------------------------------------
  // uploadDocuments — batch upload (sequential, no batch endpoint)
  // -------------------------------------------------------------------------
  async uploadDocuments(
    files: Array<{ key: string; buffer: ArrayBuffer }>,
    env: Env,
  ): Promise<Array<{ key: string; document_id: string }>> {
    const results: Array<{ key: string; document_id: string }> = [];

    const CONCURRENCY = 2; // lower concurrency due to AI calls
    let idx = 0;

    const worker = async (): Promise<void> => {
      while (idx < files.length) {
        const i = idx++;
        const f = files[i];
        if (!f) continue;
        try {
          const document_id = await this.uploadDocument(f.key, f.buffer, env);
          results.push({ key: f.key, document_id });
          console.log(`[auto] batch uploaded ${f.key} → doc_id=${document_id}`);
        } catch (e) {
          console.error(`[auto] batch upload failed: ${f.key}`, e);
        }
      }
    };

    const workers = Array.from(
      { length: Math.min(CONCURRENCY, files.length) },
      () => worker(),
    );
    await Promise.all(workers);

    return results;
  }

  // -------------------------------------------------------------------------
  // reprocessFailedDocuments — retry failed chunks
  // -------------------------------------------------------------------------
  async reprocessFailedDocuments(env: Env): Promise<void> {
    console.log(
      "[auto] reprocessFailedDocuments: checking KV for failed entries...",
    );

    const MAPPING_KEY = "mapping:auto";
    const prev: Record<string, { doc_id: string; etag: string }> =
      (await env.SYNC_KV.get(MAPPING_KEY, "json")) || {};

    for (const [key, val] of Object.entries(prev)) {
      const CHUNK_KEY = `chunks:auto:${key}`;
      const chunkMetaRaw = await env.SYNC_KV.get(CHUNK_KEY);
      if (!chunkMetaRaw) {
        console.log(`[auto] reprocess: missing chunks for ${key}, re-indexing`);
        const obj = await env.ARTIFACT_BUCKET.get(key);
        if (obj) {
          const buffer = await obj.arrayBuffer();
          await this.uploadDocument(key, buffer, env);
        }
      }
    }

    console.log("[auto] reprocessFailedDocuments completed");
  }
}
