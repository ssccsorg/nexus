# Codebase Analysis: Explainability in EdgeQuake

**Issue**: [#128](https://github.com/raphaelmansuy/edgequake/issues/128)

---

## Current Provenance Infrastructure

### 1. Pipeline Lineage (`edgequake-pipeline/src/lineage.rs`)

The lineage module already defines rich provenance types:

```
SourceSpan          → line/offset location within a document
ExtractionMetadata  → LLM model, tokens, timing, cache hit, gleaning iterations
ChunkLineage        → chunk → [entity_ids, relationship_ids] mapping
EntitySource        → document → chunk → source_spans chain
DescriptionVersion  → append-only history of entity description evolution
EntityLineage       → entity → [sources, extraction_count, description_history]
RelationshipLineage → relationship → [source/target entities, source docs, chunks]
DocumentLineage     → document → [chunks, entity count, relationship count]
```

**Gap**: These types are constructed during pipeline processing but are NOT
persisted to graph storage. After extraction completes, lineage metadata is lost
except for what the merger writes to node/edge properties.

### 2. Merger Source Tracking (`edgequake-pipeline/src/merger/`)

The entity merger writes to graph node properties:
```
source_chunk_ids:     Vec<String>   (JSON array)
source_document_id:   String
source_file_path:     String
```

The relationship merger writes to edge properties:
```
source_chunk_id:      String
source_document_id:   String
source_file_path:     String
```

**Gap**: No extraction method, LLM model, gleaning iteration, or confidence is
stored. When entities merge from multiple documents, only the last
`source_document_id` survives — earlier sources are in `source_chunk_ids` but
not explicitly tracked per-document.

### 3. Query Source Tracking (`edgequake-query/src/helpers.rs`)

At query time, source tracking is extracted from node/edge properties:

```
EntitySourceTracking:
  source_chunk_ids:    Vec<String>
  source_document_id:  Option<String>
  source_file_path:    Option<String>

RelationshipSourceTracking:
  source_chunk_id:     Option<String>
  source_document_id:  Option<String>
  source_file_path:    Option<String>
```

These are mapped into `RetrievedEntity` and `RetrievedRelationship` objects,
which become `SourceReference` in the API response.

**Gap**: No intermediate query decisions are recorded. The query engine
discards all candidates that didn't make the final cut.

### 4. API Response Structure (`edgequake-api/src/handlers/query_types.rs`)

```
QueryResponse:
  answer:    String
  mode:      String
  sources:   Vec<SourceReference>
  stats:     QueryStats

SourceReference:
  source_type:      String (chunk | entity | relationship)
  id:               String
  score:            f32
  snippet:          Option<String>
  document_id:      Option<String>
  file_path:        Option<String>
  source_chunk_ids: Option<Vec<String>>  (entities only)

QueryStats:
  embedding_time_ms, retrieval_time_ms, generation_time_ms
  total_time_ms, sources_retrieved
```

**Gap**: No explainability field. No confidence score. No reasoning trace.

### 5. Streaming Events (`QueryStreamEvent`)

```
Context { sources, query_mode, retrieval_time_ms }
Token { content }
Thinking { content }
Done { stats }
Error { message, code }
```

**Gap**: No `Explain` event type for streaming explainability data.

---

## Data Flow Summary

```
+------------------------------------------------------------------+
|  DOCUMENT INGESTION (BUILD-TIME)                                 |
|                                                                  |
|  Document                                                        |
|    |                                                             |
|    v                                                             |
|  Chunker (lineage: ChunkLineage with line numbers)               |
|    |                                                             |
|    v                                                             |
|  Extractor (lineage: ExtractionMetadata with model/tokens)       |
|    |    LOST HERE: ExtractionMetadata not persisted               |
|    v                                                             |
|  Merger (writes: source_chunk_ids, source_document_id only)      |
|    |    LOST HERE: extraction method, gleaning, confidence        |
|    v                                                             |
|  Graph Storage (node/edge properties with partial provenance)    |
+------------------------------------------------------------------+

+------------------------------------------------------------------+
|  QUERY ANSWERING (QUERY-TIME)                                    |
|                                                                  |
|  Query Text                                                      |
|    |                                                             |
|    v                                                             |
|  Embedding + Mode Selection (no trace recorded)                  |
|    |    LOST: why hybrid mode was chosen                         |
|    v                                                             |
|  Vector Search (scores returned but candidates discarded)        |
|    |    LOST: rejected candidates, thresholds                    |
|    v                                                             |
|  Graph Traversal (entities/edges fetched but path not recorded)  |
|    |    LOST: traversal decisions, degree-based ranking           |
|    v                                                             |
|  Context Assembly (truncation applied but decisions not recorded) |
|    |    LOST: what was cut to fit token budget                    |
|    v                                                             |
|  LLM Call (prompt + answer, no intermediate trace)               |
|    |                                                             |
|    v                                                             |
|  Response: answer + sources + stats (minimal provenance)         |
+------------------------------------------------------------------+
```

---

## Implications

### What We Already Have (Strengths)

1. **Lineage types are well-designed** — `lineage.rs` has comprehensive types
   that just need to be persisted and surfaced
2. **Source tracking in properties** — Entity/relationship nodes already carry
   chunk and document IDs
3. **Query stats framework** — Timing metrics infrastructure can be extended
4. **Graph storage has properties** — `HashMap<String, Value>` on nodes/edges
   can store arbitrary metadata without schema changes

### What We Need (Gaps)

| Gap | Location | Impact |
|-----|----------|--------|
| Extraction metadata not persisted | Merger | Cannot trace extraction method |
| Single source_document_id per entity | Merger | Multi-doc entities lose provenance |
| No query trace recorder | Query Engine | Cannot explain retrieval decisions |
| No confidence scorer | Query Engine | No trust signal for users |
| No explain API field | API layer | No way to request explainability |
| No explain event | Streaming | Streaming queries lack explainability |
| No explain UI panel | Frontend | No visual explainability |

### Storage Impact

Adding extraction provenance to graph properties increases storage per node:

```
Current:  ~3 properties × ~50 bytes = ~150 bytes/entity
Proposed: ~8 properties × ~50 bytes = ~400 bytes/entity
Overhead: ~250 bytes/entity, ~170% increase

For 10K entities: ~2.5 MB additional storage
For 100K entities: ~25 MB additional storage
```

This is acceptable for PostgreSQL-backed storage.

### Performance Impact

Explainability should be opt-in. When disabled (default), zero overhead.
When enabled:

- **Build-time**: Writing additional properties during merge adds ~5% overhead
- **Query-time**: Collecting trace data during retrieval adds ~10% overhead
- **Response size**: Explain payload adds ~2-5KB per query response
