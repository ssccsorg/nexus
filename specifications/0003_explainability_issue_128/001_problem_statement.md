# Problem Statement: Explainability in EdgeQuake Information Retrieval

**Issue**: [#128](https://github.com/raphaelmansuy/edgequake/issues/128)

---

## WHY

RAG systems are increasingly used for high-stakes decisions — compliance audits,
medical literature reviews, manufacturing root-cause analysis. In these domains,
**an answer without provenance is worthless**. Users need to understand not just
*what* the system concluded, but *why* it reached that conclusion.

Today EdgeQuake returns answers with source references (chunk IDs, document IDs,
entity names), but provides NO insight into:

1. **Reasoning path**: Which entities and relationships contributed most to the
   answer? What graph traversal path connected user query to final context?
2. **Extraction provenance**: Was this entity extracted by the LLM or by the
   SOTA rule-based extractor? How many gleaning iterations refined it?
3. **Retrieval selection**: Why was this chunk chosen over other candidates?
   What was the embedding similarity score? Was reranking applied?
4. **Confidence signal**: How trustworthy is this answer based on source
   coverage, entity connectivity, and extraction quality?

Without explainability, EdgeQuake is a black box. Users cannot:

- Verify correctness when the answer seems surprising
- Debug poor answers when retrieval quality is low
- Audit the system for regulatory compliance
- Build trust incrementally through transparent reasoning

### Inspiration: TrustGraph

[TrustGraph](https://trustgraph.ai/) implements a 3-graph model:

| Graph | Purpose | Named Graph |
|-------|---------|-------------|
| **Knowledge Graph** | Primary entity-relationship data | `urn:graph:knowledge` |
| **Extraction Provenance** | Which document, chunk, and LLM call produced each entity | `urn:graph:source` |
| **Retrieval Explainability** | Query-time: which paths were traversed, scores, reasoning | `urn:graph:retrieval` |

TrustGraph stores all three as RDF named graphs in the same triplestore. This
allows SPARQL queries like: "Show me the source document for ENTITY_X" or
"What was the retrieval path for query Q?"

EdgeQuake does NOT need to adopt RDF or SPARQL. But the **conceptual model** of
separating provenance from knowledge is directly applicable.

---

## Problem Decomposition

### P1: Extraction Provenance (Build-time)

When entities and relationships are extracted, record:
- Which document and chunk produced them
- Which extractor (SOTA vs LLM) was used
- LLM model name, token counts, gleaning iteration
- Timestamp and processing duration
- Confidence score from extraction

**Current state**: `lineage.rs` tracks `ExtractionMetadata` with model, tokens,
and timing. `EntitySource` tracks document/chunk origins. But this data is NOT
stored in the graph node properties — it's only available during pipeline
processing.

### P2: Retrieval Explainability (Query-time)

When answering a query, record:
- Query embedding and mode selection reasoning
- Vector search candidates (scores, chunk IDs)
- Graph traversal path (entities visited, edges followed)
- Context window composition (what was included/excluded and why)
- Token budget allocation and truncation decisions
- Rerank scores (if applied)

**Current state**: `QueryStats` tracks timing metrics. `SourceReference` tracks
chunk/entity IDs and scores. But NO intermediate decisions are recorded.

### P3: Confidence Scoring

Compute an answer-level confidence score based on:
- Number and diversity of supporting sources
- Average embedding similarity scores
- Graph connectivity of cited entities
- Extraction quality metadata

**Current state**: No confidence scoring exists.

### P4: Explainability API Surface

Expose explainability data through:
- Optional `explain` flag on query requests
- Structured response field with provenance chain
- Standalone API endpoint for entity/relationship provenance

**Current state**: `include_references: bool` on `QueryRequest` adds document
IDs to sources. No deeper explainability.

---

## Success Criteria

1. User can trace any entity in the answer back to its source document and chunk
2. User can see which retrieval steps led to each source being included
3. User can see a confidence indicator for the overall answer
4. Explainability is opt-in (no performance penalty when disabled)
5. Provenance survives entity merges (multi-document entities track ALL sources)
6. Frontend can render an "Explain" panel showing the reasoning chain
