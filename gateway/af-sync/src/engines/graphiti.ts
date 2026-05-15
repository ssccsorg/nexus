// src/engines/graphiti.ts
// Graphiti engine handler — temporal knowledge graph
//
// Graphiti reference: https://github.com/getzep/graphiti
//
// Strategy: "put" (cumulative)
//   Unlike LightRAG/EdgeQuake which overwrite on change, Graphiti ingests
//   each file version as a new episode in the temporal knowledge graph.
//   Deletes are rare and handled explicitly via remove_episode().
//
// Graphiti requires a thin HTTP wrapper server (graphiti-server.py) that
// exposes add_episode / retrieve_episodes / remove_episode as REST endpoints.
//
// Graphiti server API endpoints used:
//   GET  /episodes              – list recent episodes
//   POST /episodes              – add a new episode
//   DELETE /episodes/:uuid      – remove an episode by UUID

import type { Env } from "../index";

export class GraphitiHandler {
  readonly strategy = "put" as const;

  private base(env: Env): string {
    return env.GRAPHITI_API_HOST;
  }

  private headers(env: Env): HeadersInit {
    const h: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (env.GRAPHITI_API_KEY) {
      h["X-API-Key"] = env.GRAPHITI_API_KEY;
    }
    return h;
  }

  // -------------------------------------------------------------------------
  // listDocuments – list recent episodes
  // -------------------------------------------------------------------------
  async listDocuments(env: Env): Promise<Array<{ id: string; title: string }>> {
    const url = `${this.base(env)}/episodes`;

    const res = await fetch(url, {
      method: "GET",
      headers: this.headers(env),
    });

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(
        `Graphiti list failed: ${res.status} ${res.statusText} — ${body.slice(0, 300)}`,
      );
    }

    const data = (await res.json()) as {
      episodes: Array<{
        uuid: string;
        name: string;
        content: string;
        created_at: string;
      }>;
    };

    return data.episodes.map((ep) => ({
      id: ep.uuid,
      title: ep.name?.slice(0, 120) || ep.uuid,
    }));
  }

  // -------------------------------------------------------------------------
  // deleteDocument – remove an episode by UUID
  // -------------------------------------------------------------------------
  async deleteDocument(id: string, env: Env): Promise<void> {
    const url = `${this.base(env)}/episodes/${id}`;

    const res = await fetch(url, {
      method: "DELETE",
      headers: this.headers(env),
    });

    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(
        `Graphiti delete failed: ${res.status} ${res.statusText} — ${body.slice(0, 200)}`,
      );
    }
  }

  // -------------------------------------------------------------------------
  // uploadDocument – add a single episode
  // -------------------------------------------------------------------------
  async uploadDocument(
    key: string,
    buffer: ArrayBuffer,
    env: Env,
  ): Promise<string> {
    const url = `${this.base(env)}/episodes`;
    const text = new TextDecoder().decode(buffer);

    const body = JSON.stringify({
      name: key,
      episode_body: text,
      source_description: `R2 sync: ${key}`,
      reference_time: new Date().toISOString(),
    });

    const res = await fetch(url, {
      method: "POST",
      headers: this.headers(env),
      body,
    });

    if (!res.ok) {
      const bodyText = await res.text().catch(() => "");
      throw new Error(
        `Graphiti upload failed: ${res.status} ${res.statusText} — ${bodyText.slice(0, 300)}`,
      );
    }

    const data = (await res.json()) as { episode_uuid: string };

    console.log(
      `[graphiti] uploaded ${key} → episode_uuid=${data.episode_uuid}`,
    );

    // Return the episode UUID as the document_id for KV mapping
    return data.episode_uuid;
  }

  // -------------------------------------------------------------------------
  // uploadDocuments – batch add episodes
  // -------------------------------------------------------------------------
  async uploadDocuments(
    files: Array<{ key: string; buffer: ArrayBuffer }>,
    env: Env,
  ): Promise<Array<{ key: string; document_id: string }>> {
    const results: Array<{ key: string; document_id: string }> = [];

    // Graphiti server likely has a batch endpoint or we fall back to
    // sequential uploads with limited concurrency
    const CONCURRENCY = 3;
    let idx = 0;

    const worker = async (): Promise<void> => {
      while (idx < files.length) {
        const i = idx++;
        const f = files[i];
        if (!f) continue;
        try {
          const document_id = await this.uploadDocument(
            f.key,
            f.buffer,
            env,
          );
          results.push({ key: f.key, document_id });
        } catch (e) {
          console.error(`[graphiti] batch upload failed: ${f.key}`, e);
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
}
