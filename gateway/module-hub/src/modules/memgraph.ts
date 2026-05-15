// src/modules/memgraph.ts
// Memgraph client — speaks to Memgraph via the HTTP-to-bolt proxy.
// Implements the KgClient interface defined in types.ts.

import type { KgClient } from "./types";

export class MemgraphClient implements KgClient {
  private baseUrl: string;
  private apiKey: string;

  constructor(baseUrl: string, apiKey: string) {
    // Remove trailing slash
    this.baseUrl = baseUrl.replace(/\/+$/, "");
    this.apiKey = apiKey;
  }

  async query(cypher: string, params?: Record<string, any>): Promise<any[]> {
    const url = `${this.baseUrl}/cypher`;
    const res = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(this.apiKey ? { Authorization: `Bearer ${this.apiKey}` } : {}),
      },
      body: JSON.stringify({
        query: cypher,
        params: params ?? {},
      }),
    });

    if (!res.ok) {
      const text = await res.text();
      throw new Error(`Memgraph query failed (${res.status}): ${text}`);
    }

    const body = (await res.json()) as { results: any[] };
    return body.results;
  }

  async ping(): Promise<boolean> {
    try {
      const res = await fetch(`${this.baseUrl}/health`, {
        signal: AbortSignal.timeout(5000),
      });
      return res.ok;
    } catch {
      return false;
    }
  }
}
