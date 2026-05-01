# SPEC-0002: Knowledge Injection — Candidate Solutions

**Issue**: [#131](https://github.com/raphaelmansuy/edgequake/issues/131)

---

## Candidate A: Tagged Ingestion (Recommended)

Process injection text through the **same pipeline** but tag all artifacts
with `source_type = "injection"`:

```
+-----------------------------------------------------------+
|                INJECTION FLOW                             |
|                                                           |
|  Admin submits injection text                             |
|    |                                                      |
|    v                                                      |
|  Generate stable doc_id:                                  |
|    "injection::{workspace_id}"                            |
|    |                                                      |
|    v                                                      |
|  Delete previous injection artifacts                      |
|  (cascade delete by source_document_id)                   |
|    |                                                      |
|    v                                                      |
|  Run pipeline with injection flag:                        |
|    - Chunks tagged: source_type = "injection"             |
|    - Entities tagged: source_type = "injection"           |
|    - Vector metadata: source = "injection"                |
|    |                                                      |
|    v                                                      |
|  Store in same KV/Vector/Graph stores                     |
|  (entities merge with existing if names match)            |
+-----------------------------------------------------------+
```

### Citation Filtering

At query time, filter injection sources:

```rust
// In query result builder
let cited_sources: Vec<_> = raw_sources
    .iter()
    .filter(|s| s.source_type != "injection")
    .collect();
```

### Pros

- Reuses 100% of existing pipeline
- Entities extracted from injection merge with doc-extracted entities
- Single cascade-delete removes all injection artifacts
- No separate storage engine

### Cons

- Injection entities can merge with document entities (intended)
- Query filter adds one condition per query

---

## Candidate B: System Prompt Augmentation

Instead of processing injection text through the pipeline, prepend it
to every LLM query as a system prompt augmentation:

```
System prompt = base_prompt + "\n\nDomain context:\n" + injection_text
```

### Pros

- Zero pipeline changes
- Immediate effect without re-processing
- Truly invisible to source tracking

### Cons

- Eats into LLM context window (can reduce answer quality for long glossaries)
- No semantic search benefit (vector search unchanged)
- No graph enrichment (no new entities)
- Scales poorly: 50KB glossary = ~12K tokens per query
- Does NOT improve retrieval, only generation

---

## Candidate C: Virtual Vector Collection

Create a separate vector collection for injection content. At query time,
search both collections and merge results but only cite from the main one.

### Pros

- Clean separation of injection vs. document vectors
- No metadata tagging needed

### Cons

- Doubles vector search cost
- Per-workspace vector tables already complex (SPEC-032)
- Merge logic for two result sets is non-trivial
- Graph entities still need tagging to avoid citations

---

## Decision Matrix

| Criterion             | Weight | A: Tagged | B: Sys Prompt | C: Virtual Vec |
| --------------------- | ------ | --------- | ------------- | -------------- |
| Retrieval improvement | 30%    | 5         | 1             | 5              |
| Implementation effort | 25%    | 4         | 5             | 2              |
| Graph enrichment      | 20%    | 5         | 0             | 3              |
| Simplicity/DRY        | 15%    | 4         | 5             | 2              |
| Scalability           | 10%    | 5         | 1             | 4              |
| **Weighted Score**    |        | **4.45**  | **2.25**      | **3.20**       |

## Recommendation

**Candidate A** — tagged ingestion through existing pipeline. Maximizes
retrieval improvement with minimal new code.
