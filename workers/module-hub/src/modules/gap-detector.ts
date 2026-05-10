// src/modules/gap-detector.ts
// Gap Detector — structural analysis of the document corpus.
//
// CURRENT MODE: Embedding-based similarity analysis using CF Workers AI.
//   Detects potential conceptual gaps by comparing document embeddings.
//
// FUTURE MODE (when Memgraph is deployed):
//   Cypher-based KG structural analysis — orphaned nodes, disconnected subgraphs,
//   contradictory edges, etc. (Cypher queries are preserved at the bottom
//   of this file, commented out, for when Memgraph comes online.)

import type { ReasoningModule, ModuleContext, Finding } from "./types";
import { register } from "./registry";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Split text into chunks of roughly `maxWords` words. */
function chunkText(text: string, maxWords: number = 256): string[] {
  const words = text.split(/\s+/);
  const chunks: string[] = [];
  let current: string[] = [];
  for (const w of words) {
    current.push(w);
    if (current.length >= maxWords) {
      chunks.push(current.join(" "));
      current = [];
    }
  }
  if (current.length > 0) chunks.push(current.join(" "));
  return chunks.length > 0 ? chunks : [text];
}

/** Compute cosine similarity between two equal-length vectors. */
function cosineSimilarity(a: number[], b: number[]): number {
  if (a.length !== b.length || a.length === 0) return 0;
  let dot = 0, normA = 0, normB = 0;
  for (let i = 0; i < a.length; i++) {
    const ai = a[i]!;
    const bi = b[i]!;
    dot += ai * bi;
    normA += ai * ai;
    normB += bi * bi;
  }
  const denom = Math.sqrt(normA) * Math.sqrt(normB);
  return denom === 0 ? 0 : dot / denom;
}

// ---------------------------------------------------------------------------
// Document chunk result
// ---------------------------------------------------------------------------
interface DocumentChunks {
  key: string;
  chunks: string[];
  embeddings: number[][];
}

// ---------------------------------------------------------------------------
// Gap Detector Module
// ---------------------------------------------------------------------------
export class GapDetectorModule implements ReasoningModule {
  manifest = {
    name: "gap-detector",
    description:
      "Structural analysis of the document corpus using embedding similarity. " +
      "Detects potential conceptual gaps, orphaned documents, and near-duplicate " +
      "content. When Memgraph is available, switches to Cypher-based KG analysis.",
    version: "0.2.0",
    runMode: "periodic" as const,
    schedule: "0 */6 * * *",
    requiresKgs: [],
    contract: {
      level: "normal" as const,
      minEvidence: 1,
      requireHumanReview: false,
    },
  };

  async init(_ctx: ModuleContext): Promise<void> {
    // No initialization needed
  }

  async run(ctx: ModuleContext): Promise<Finding[]> {
    const findings: Finding[] = [];
    const now = new Date().toISOString();

    // ---------------------------------------------------------------
    // Phase 1: Scan R2, chunk, embed
    // ---------------------------------------------------------------
    const documents: DocumentChunks[] = [];
    let scanned = 0;
    let cursor: string | undefined;

    do {
      const opts: R2ListOptions = { limit: 100 };
      if (cursor) {
        opts.cursor = cursor;
      }
      const result = await ctx.bucket.list(opts);
      for (const obj of result.objects) {
        // Skip non-text files
        if (!obj.key.endsWith(".md") && !obj.key.endsWith(".txt") && !obj.key.endsWith(".qmd")) {
          continue;
        }

        const r2Obj = await ctx.bucket.get(obj.key);
        if (!r2Obj) continue;

        const text = await r2Obj.text();
        const chunks = chunkText(text, 256);

        // Embed each chunk using CF Workers AI
        const embeddings: number[][] = [];
        for (const chunk of chunks) {
          try {
            const response = await ctx.ai.run(
              "@cf/baai/bge-small-en-v1.5",
              { text: [chunk] },
            ) as { data: number[][] };
            if (response.data?.[0]) {
              embeddings.push(response.data[0]);
            }
          } catch (e) {
            console.warn(`[gap-detector] embedding failed for ${obj.key}:`, e);
          }
        }

        if (embeddings.length > 0) {
          documents.push({ key: obj.key, chunks, embeddings });
        }

        scanned++;
        if (scanned >= 50) break;
      }
      cursor = result.truncated ? result.cursor : undefined;
    } while (cursor && scanned < 50);

    console.log(`[gap-detector] scanned ${scanned} files, embedded ${documents.length}`);

    // ---------------------------------------------------------------
    // Phase 2: Cross-document similarity analysis
    // ---------------------------------------------------------------

    for (let i = 0; i < documents.length; i++) {
      const docA = documents[i]!;

      for (let j = i + 1; j < documents.length; j++) {
        const docB = documents[j]!;

        for (let ci = 0; ci < docA.embeddings.length; ci++) {
          const embA = docA.embeddings[ci]!;

          for (let cj = 0; cj < docB.embeddings.length; cj++) {
            const embB = docB.embeddings[cj]!;
            const sim = cosineSimilarity(embA, embB);

            // Near-duplicate: similarity above 0.95
            if (sim > 0.95) {
              findings.push({
                id: `gap-duplicate-${docA.key}-${docB.key}-c${ci}-c${cj}`,
                moduleName: "gap-detector",
                severity: "warning",
                status: "open",
                title: `Near-duplicate content: ${docA.key} ↔ ${docB.key}`,
                description:
                  `Two chunks from different documents have near-identical content ` +
                  `(similarity: ${sim.toFixed(3)}). This may indicate redundant or ` +
                  `duplicate information across the corpus.`,
                evidence: [
                  `docA: ${docA.key}#chunk-${ci}`,
                  `docB: ${docB.key}#chunk-${cj}`,
                  `similarity: ${sim.toFixed(3)}`,
                ],
                createdAt: now,
                updatedAt: now,
              });
            }

            // Severely divergent first chunks
            if (ci === 0 && cj === 0 && sim < 0.1) {
              findings.push({
                id: `gap-divergent-${docA.key}-${docB.key}`,
                moduleName: "gap-detector",
                severity: "info",
                status: "open",
                title: `Potentially divergent concepts: ${docA.key} ↔ ${docB.key}`,
                description:
                  `Opening chunks of these two documents have very low similarity ` +
                  `(${sim.toFixed(3)}). If they belong to the same domain, this ` +
                  `may indicate conceptual drift or conflicting definitions.`,
                evidence: [
                  `docA: ${docA.key}`,
                  `docB: ${docB.key}`,
                  `similarity: ${sim.toFixed(3)}`,
                ],
                createdAt: now,
                updatedAt: now,
              });
            }
          }
        }
      }
    }

    // 2b. Orphaned documents
    if (documents.length >= 3) {
      for (const doc of documents) {
        let maxSim = 0;
        for (const other of documents) {
          if (other.key === doc.key) continue;
          for (const emb of doc.embeddings) {
            for (const oEmb of other.embeddings) {
              const sim = cosineSimilarity(emb, oEmb);
              if (sim > maxSim) maxSim = sim;
            }
          }
        }
        if (maxSim < 0.2) {
          findings.push({
            id: `gap-orphaned-${doc.key}`,
            moduleName: "gap-detector",
            severity: "warning",
            status: "open",
            title: `Orphaned document: ${doc.key}`,
            description:
              `"${doc.key}" has no significant semantic overlap with any other ` +
              `document in the corpus (max similarity: ${maxSim.toFixed(3)}). ` +
              `It may be topically isolated or an outlier.`,
            evidence: [`doc: ${doc.key}`, `max_similarity: ${maxSim.toFixed(3)}`],
            createdAt: now,
            updatedAt: now,
          });
        }
      }
    }

    // ---------------------------------------------------------------
    // Persist findings to KV
    // ---------------------------------------------------------------
    await ctx.kv.put(
      `findings:gap-detector:${now}`,
      JSON.stringify({
        generatedAt: now,
        count: findings.length,
        documentsScanned: scanned,
        documentsEmbedded: documents.length,
        findings,
      }),
    );
    console.log(`[gap-detector] total findings: ${findings.length}`);

    return findings;
  }

  async teardown(_ctx: ModuleContext): Promise<void> {
    // No resources to clean up
  }
}

// ---------------------------------------------------------------------------
// Register the module
// ---------------------------------------------------------------------------
register("gap-detector", GapDetectorModule);

// ===========================================================================
// FUTURE: Memgraph Cypher queries (uncomment when Memgraph is deployed)
// ===========================================================================
//
// const CYPHER_QUERIES = {
//   // Orphaned concepts — nodes with zero relationships
//   orphanedConcepts: `
//     MATCH (c:Concept)
//     WHERE NOT EXISTS { (c)--() }
//     RETURN c.name, c.id, labels(c) AS labels
//     ORDER BY c.name
//   `,
//
//   // Missing relationships — co-occurring but unconnected
//   missingRelationships: `
//     MATCH (a:Concept)-[:APPEARS_IN]->(d:Document)<-[:APPEARS_IN]-(b:Concept)
//     WHERE a <> b AND NOT EXISTS { (a)-[:RELATES_TO|DEPENDS_ON|DEFINES|CITES]-(b) }
//     WITH a.name AS source, b.name AS target, collect(DISTINCT d.title) AS shared_docs
//     WHERE size(shared_docs) >= 2
//     RETURN source, target, shared_docs, size(shared_docs) AS cooccurrence_count
//     ORDER BY cooccurrence_count DESC LIMIT 100
//   `,
//
//   // Contradictory edges
//   contradictoryEdges: `
//     MATCH (a:Concept)-[r1]->(b:Concept)
//     MATCH (a)-[r2]->(b)
//     WHERE r1 <> r2 AND type(r1) <> type(r2)
//     RETURN a.name AS source, b.name AS target,
//            collect(DISTINCT type(r1)) AS relationship_types
//     ORDER BY size(collect(DISTINCT type(r1))) DESC LIMIT 20
//   `,
// };
