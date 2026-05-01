# SPEC-0002: Knowledge Injection — Codebase Analysis

**Issue**: [#131](https://github.com/raphaelmansuy/edgequake/issues/131)

---

## Current Ingestion Architecture

```
+------------------------------------------------------------------+
|                   DOCUMENT INGESTION PIPELINE                    |
|                                                                  |
|  Content (text/PDF)                                              |
|    |                                                             |
|    v                                                             |
|  +-------------------+                                           |
|  | Chunker            |  Split into overlapping chunks           |
|  | (ChunkerConfig)    |  chunk_size=1200, overlap=100            |
|  +-------------------+                                           |
|    |                                                             |
|    v                                                             |
|  +-------------------+                                           |
|  | Entity Extractor   |  LLM extracts entities + relationships   |
|  | (SOTA/LLM/Glean)   |  Outputs: ExtractedEntity, Rel           |
|  +-------------------+                                           |
|    |                                                             |
|    v                                                             |
|  +-------------------+                                           |
|  | Embedding          |  Generate vectors for chunks + entities  |
|  | (per workspace)    |                                          |
|  +-------------------+                                           |
|    |                                                             |
|    v                                                             |
|  +-------------------+                                           |
|  | KG Merger           |  Deduplicate + merge into graph         |
|  | (entity + rel)      |  source_id tracked per entity          |
|  +-------------------+                                           |
|    |                                                             |
|    v                                                             |
|  +-------------------+  +-------------------+  +-------------+  |
|  | KV Storage         |  | Vector Storage    |  | Graph Store |  |
|  | (doc + chunks)     |  | (pgvector)        |  | (AGE)       |  |
|  +-------------------+  +-------------------+  +-------------+  |
+------------------------------------------------------------------+
```

## Key Observation: Source Tracking

Every entity and relationship carries `source_id` (pipe-separated chunk IDs)
and `source_document_id`. The query engine uses these to cite sources.

**Critical insight**: If injection content is processed as a regular document,
its chunk IDs will appear in `source_id` and be cited. We must either:

1. **Filter at query time**: Exclude injection sources from citations.
2. **Tag at ingestion time**: Mark injection entities with a special flag.

## Relevant Code Touchpoints

| Component           | File                                                 | Role                            |
| ------------------- | ---------------------------------------------------- | ------------------------------- |
| Pipeline entry      | `edgequake-core/src/orchestrator/ingestion.rs`       | `insert()` processes content    |
| Chunk embedding     | `edgequake-pipeline/src/pipeline/helpers.rs`         | Generates embeddings            |
| Entity merger       | `edgequake-pipeline/src/merger/entity.rs`            | Merges entities into graph      |
| Relationship merger | `edgequake-pipeline/src/merger/relationship.rs`      | Merges rels into graph          |
| Vector metadata     | `merger/entity.rs` L20-35                            | Sets `type`, `source_chunk_ids` |
| KV storage          | `edgequake-storage/src/traits/kv.rs`                 | Stores doc + chunk records      |
| Query citations     | `edgequake-query/src/`                               | Builds source lists             |
| Workspace model     | `edgequake-core/src/types/multitenancy/workspace.rs` | Workspace metadata              |

## Vector Metadata Structure (Current)

When entities are stored in vector storage, metadata includes:

```json
{
  "type": "entity",
  "entity_name": "OEE",
  "entity_type": "CONCEPT",
  "description": "Overall Equipment Effectiveness",
  "source_chunk_ids": ["chunk-123"],
  "source_document_id": "doc-456",
  "source_file_path": "report.pdf"
}
```

**Injection approach**: Add `"source": "injection"` to metadata, then
filter in query results.

## Workspace Metadata Extension Point

`Workspace.metadata` is a `HashMap<String, serde_json::Value>`. The injection
document reference can be stored here:

```json
{
  "injection_document_id": "uuid",
  "injection_version": 3,
  "injection_updated_at": "2026-04-01T00:00:00Z"
}
```
