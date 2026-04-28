# Candidate Solutions: Explainability

**Issue**: [#128](https://github.com/raphaelmansuy/edgequake/issues/128)

---

## Candidate A: Property-Enriched Provenance (Recommended)

### Concept

Extend existing graph node/edge properties with extraction provenance at
build-time. Add an opt-in `ExplainTrace` collector at query-time that records
retrieval decisions. Surface via `explain` flag on query requests.

### Build-Time: Enriched Properties

Write additional properties during entity/relationship merge:

```
Node properties (entity):
  extraction_method:   "sota" | "llm" | "gleaning"
  extraction_model:    "gpt-5-nano" | "gemma3:latest"
  gleaning_iterations: 2
  extraction_count:    3      (how many times extracted across docs)
  source_documents:    ["doc-001", "doc-003"]  (ALL source doc IDs)
  first_extracted_at:  "2025-01-15T..."
  last_updated_at:     "2025-01-20T..."

Edge properties (relationship):
  extraction_method:   "sota" | "llm"
  extraction_model:    "gemma3:latest"
  extraction_weight:   0.85   (confidence from extraction)
```

### Query-Time: ExplainTrace

```rust
struct ExplainTrace {
    query_analysis: QueryAnalysis,
    retrieval_steps: Vec<RetrievalStep>,
    context_assembly: ContextAssembly,
    confidence: ConfidenceScore,
}

struct QueryAnalysis {
    original_query: String,
    extracted_keywords: ExtractedKeywords,
    selected_mode: QueryMode,
    mode_reason: String,  // "hybrid: query contains both entity names and themes"
}

struct RetrievalStep {
    step_type: StepType,  // VectorSearch | GraphTraversal | Rerank
    candidates_count: usize,
    selected_count: usize,
    threshold: f32,
    top_scores: Vec<(String, f32)>,  // (id, score) for top candidates
    duration_ms: u64,
}

struct ContextAssembly {
    total_tokens: usize,
    budget: usize,
    chunks_included: usize,
    chunks_truncated: usize,
    entities_included: usize,
    relationships_included: usize,
}

struct ConfidenceScore {
    overall: f32,           // 0.0 to 1.0
    source_coverage: f32,   // % of answer supported by multiple sources
    avg_similarity: f32,    // average embedding score
    graph_connectivity: f32, // average degree of cited entities
}
```

### API Surface

```
POST /api/v1/query        { "query": "...", "explain": true }
POST /api/v1/query/stream { "query": "...", "explain": true }

GET /api/v1/entities/:id/provenance

Response additions:
{
  "answer": "...",
  "sources": [...],
  "explain": {                      // Only present when explain=true
    "query_analysis": {...},
    "retrieval_steps": [...],
    "context_assembly": {...},
    "confidence": { "overall": 0.82, ... }
  }
}
```

**Pros**:
- Leverages existing property storage (no schema change)
- Opt-in: zero overhead when explain=false
- Confidence score gives actionable trust signal
- Entity provenance endpoint enables standalone exploration

**Cons**:
- Properties are untyped (HashMap<String, Value>) — no compile-time safety
- Explain trace is ephemeral (not persisted) — per-query only

---

## Candidate B: Separate Provenance Table

### Concept

Create a dedicated `extraction_provenance` PostgreSQL table that stores
structured provenance per entity/relationship.

```sql
CREATE TABLE extraction_provenance (
  id              UUID PRIMARY KEY,
  entity_id       TEXT NOT NULL,
  workspace_id    UUID NOT NULL,
  document_id     UUID NOT NULL,
  chunk_id        TEXT NOT NULL,
  extraction_method TEXT NOT NULL,  -- 'sota', 'llm', 'gleaning'
  model_name      TEXT,
  gleaning_iter   INT DEFAULT 0,
  input_tokens    INT DEFAULT 0,
  output_tokens   INT DEFAULT 0,
  extraction_ms   INT DEFAULT 0,
  confidence      FLOAT DEFAULT 0.0,
  extracted_at    TIMESTAMPTZ DEFAULT NOW(),

  CONSTRAINT fk_workspace FOREIGN KEY (workspace_id)
    REFERENCES workspaces(id) ON DELETE CASCADE
);

CREATE INDEX idx_prov_entity ON extraction_provenance(entity_id, workspace_id);
CREATE INDEX idx_prov_document ON extraction_provenance(document_id);
```

**Pros**:
- Structured, queryable provenance (SQL joins, aggregations)
- Clean separation of concerns (provenance vs knowledge)
- Can support audit log queries ("show all extractions from document X")
- Easy to export for compliance

**Cons**:
- Requires migration (new table)
- Additional write during merge (INSERT per entity-document pair)
- More complex delete cascade (must clean provenance on entity delete)
- Separate from graph — not accessible via graph traversal

---

## Candidate C: TrustGraph-Inspired Named Graphs (Apache AGE)

### Concept

Use Apache AGE's graph labeling to simulate named graphs:
- Primary KG label: `entity` / `relationship` (existing)
- Provenance label: `provenance_entity` / `provenance_edge`
- Retrieval label: `retrieval_trace`

Store provenance as graph nodes linked to entities via `EXTRACTED_FROM` edges.

```
(:Document {id: "doc-001"})
  -[:PRODUCED]->
    (:Chunk {id: "chunk-5"})
      -[:EXTRACTED {method: "llm", model: "gpt-5-nano"}]->
        (:Entity {name: "OEE"})
```

**Pros**:
- True graph provenance (traversable with Cypher)
- Closest to TrustGraph model
- Provenance is first-class citizen in the knowledge graph
- Enables queries like: "What documents support this entity?"

**Cons**:
- Significant complexity in Apache AGE graph schema
- Large graph bloat: 2-3x more nodes (every chunk becomes a node)
- Performance risk: graph traversal becomes slower
- Not all storage adapters support named graphs (memory adapter)

---

## Recommendation

**Candidate A: Property-Enriched Provenance**

Rationale:
1. Lowest implementation cost — extends existing properties, no migrations
2. Opt-in design — zero overhead for users who don't need explainability
3. Pragmatic: confidence score + retrieval trace cover 80% of use cases
4. Compatible with future upgrade to Candidate B if audit requirements grow
5. ExplainTrace can later be persisted to a table (Candidate B) without API changes

Candidate C is the ideal long-term architecture but requires Apache AGE expertise
and carries significant performance risk. It can be considered for v2.0.
