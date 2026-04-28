# SPEC-0002: Knowledge Injection — Problem Statement

**Issue**: [#131](https://github.com/raphaelmansuy/edgequake/issues/131)
**Author**: EdgeQuake Team
**Status**: Draft
**Created**: 2026-04-02

---

## WHY — The Problem

In highly technical domains, search quality degrades because:

1. **Acronyms**: Users search "NLP" but documents say "Natural Language
   Processing". The vector space may not connect these.
2. **Synonyms**: "ML" = "Machine Learning" = "Statistical Learning". Without
   explicit mapping, retrieval misses relevant documents.
3. **Domain jargon**: Manufacturing terms like "OEE" (Overall Equipment
   Effectiveness) have no meaning without definition.

These definitions **should enrich the search context** but **must not appear as
source documents** in query results. If they appeared as chunks, every query
would return the glossary instead of actual domain content.

### First-Principle Analysis

```
                     QUERY FLOW TODAY
                     ================

  User query: "What is the OEE of Line 3?"
       |
       v
  +---------------------+
  |  Embed query         |
  +---------------------+
       |
       v
  +---------------------+     No "OEE" definition in vectors
  |  Vector search       | --> Misses documents that mention
  +---------------------+     "Overall Equipment Effectiveness"
       |
       v
  +---------------------+
  |  Graph traversal     | --> No "OEE" entity in graph
  +---------------------+
       |
       v
  Poor results: "No relevant documents found"


                     QUERY FLOW WITH INJECTION
                     =========================

  User query: "What is the OEE of Line 3?"
       |
       v
  +---------------------+
  |  Query expansion     | --> "OEE" ≡ "Overall Equipment
  |  (from injection)    |     Effectiveness"
  +---------------------+
       |
       v
  +---------------------+
  |  Vector search       | --> Finds docs mentioning "Overall
  |  (expanded terms)    |     Equipment Effectiveness"
  +---------------------+
       |
       v
  +---------------------+
  |  Graph traversal     | --> Finds OEE entity from injection
  +---------------------+
       |
       v
  Rich results: 5 documents about Line 3 OEE
  Sources: [doc1, doc2, ...] — NOT the glossary
```

### Impact

| Stakeholder     | Pain Point                                        |
| --------------- | ------------------------------------------------- |
| Domain expert   | Spends time re-explaining acronyms in every doc   |
| End user        | Gets poor results for jargon-heavy queries        |
| Workspace admin | No way to inject domain knowledge without docs    |
| System          | Retrieval quality degrades in specialized domains |

---

## Scope Boundaries

### In Scope

- CRUD API for a single "knowledge injection" text document per workspace.
- Injection content processed into graph entities + vector embeddings.
- Injection entities/embeddings **excluded** from source citations.
- Replace semantics: new version cancels and replaces previous.
- Delete endpoint to remove injection and its artifacts.

### Out of Scope

- Multiple injection documents per workspace.
- Structured formats (CSV, JSON) — plain text only (v1).
- UI for injection management (API-first).
- Automatic acronym detection (user provides definitions).
