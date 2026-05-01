# Mission 02 — Delete Document: Analysis & ADR

**Status**: Implemented (2026-04-10)  
**Branch**: `feat/edgequake-v0.9.9`

---

## Requirements

1. When a document is deleted, its associated vectors (chunk embeddings, entity embeddings) MUST be deleted.
2. The delete operation MUST respect Multi-Tenant and Workspace isolation — no cross-workspace contamination.
3. The operation MUST delete the document reference from each Entity or Relation `source_ids`.
4. An Entity or Relation MUST be deleted when all its source documents are removed (unless it is a "Knowledge Document" / injected entity).

---

## How deletion works today (before fixes)

### Single document deletion (`handlers/documents/delete/single.rs`)

1. **KV key resolution**: Handles historic key/id mismatch (KV key prefix vs JSON `id` field).
2. **Task cancellation**: Cancels in-flight tasks for `pending`/`processing` docs before proceeding.
3. **Chunk embedding deletion**: Deletes chunk embeddings from workspace-scoped vector storage.
4. **Graph cleanup (inline)**: Scans ALL nodes across ALL workspaces (`get_all_nodes()`), filters by `source_ids`, removes document from sources or deletes node entirely. Same for edges. Third `get_all_nodes()` call used for orphaned edge detection.
5. **KV deletion**: Deletes metadata, content, chunk, and additional prefix keys.
6. **PDF table cleanup**: Deletes `pdf_documents` and `documents` rows (postgres feature).
7. **Metrics snapshot**: Records post-deletion metrics.

### Bulk deletion (`handlers/documents/delete/bulk.rs`)

1. **KV scan**: Lists all `-metadata` keys to find documents.
2. **Status check**: Skips `pending`/`processing` unless "stuck" (>1h at 100% progress). **No task cancellation**.
3. **KV deletion**: Deletes chunks, metadata, content from KV.
4. **Vector deletion**: Uses `state.vector_storage.delete()` — **global** (not workspace-scoped).
5. **Graph cleanup**: Scans all nodes, deletes those with empty `source_ids`. Scans all edges, deletes orphaned ones. **No source-filtering** (only checks empty sources).
6. **PDF table cleanup**: Deletes all PDF rows.

### Graph cleanup helper (`handlers/documents/storage_helpers.rs::cleanup_document_graph_data`)

Reusable helper used by reprocess/recovery flows. Takes a single `document_id` and filters nodes/edges. Also handles orphaned edge cleanup.

---

## What doesn't work well (Gaps Found)

### GAP-1: DRY Violation — Duplicated Graph Cleanup Logic  
**Severity**: Medium  
`single.rs` has an inline copy of graph cleanup logic (300+ lines) that diverges from `cleanup_document_graph_data()` in `storage_helpers.rs`:
- `single.rs` uses `source_prefixes` (handles mismatch case) while helper only accepts one `document_id`
- The helper is used by reprocess/recovery but NOT by delete_document itself
- **Result**: Two divergent implementations that can drift; bugs fixed in one won't be in the other

### GAP-2: Multi-Tenancy / Workspace Isolation Breach  
**Severity**: High  
`get_all_nodes()` and `get_all_edges()` return **all nodes from all workspaces**.
- When deleting a document from workspace A, entities from workspace B are scanned unnecessarily
- No workspace filtering in the graph scan — uses only `source_ids` to protect isolation
- Risk: if two workspaces have a document with an identical id prefix (unlikely with UUIDs, but possible with legacy data), cross-workspace entity modification can occur
- `state.graph_storage` is a single shared instance; there is no per-workspace graph like there is per-workspace vector storage

### GAP-3: Performance — Three Separate Graph Round-Trips  
**Severity**: Medium  
In `single.rs::delete_document()`:
```
call 1: get_all_nodes() → process nodes, collect deleted_ids
call 2: get_all_edges() → process edges  
call 3: get_all_nodes() → rebuild existing_node_ids for orphan detection
```
Call 3 is redundant — the set of remaining nodes can be computed from call 1 minus the nodes we just deleted.

### GAP-4: Bulk Delete — Missing Entity Embedding Cleanup  
**Severity**: High  
When an entity node is deleted during bulk delete, its entity embedding in the vector store is NOT deleted:
- Entity embeddings are stored separately from chunk embeddings
- After bulk delete, stale entity embeddings remain in the vector index
- Future queries can return phantom results from deleted entities

### GAP-5: Bulk Delete — Missing Content Hash Key Cleanup  
**Severity**: Medium  
`bulk.rs` does not delete the `content_hash` duplicate-detection key.
After bulk delete, users cannot re-upload the same file (still blocked by hash check).

### GAP-6: Bulk Delete — No Task Cancellation  
**Severity**: Medium  
`bulk.rs` skips `pending`/`processing` documents without cancelling their tasks.
- The processor continues writing data for skipped documents
- After bulk delete, these docs finish processing and create data in otherwise-empty system

### GAP-7: Graph Isolation Not Workspace-Scoped  
**Severity**: Medium (by design)  
The graph storage is a single global Apache AGE graph. All workspaces share the same graph, with workspace isolation achieved only via `workspace_id` property on nodes. There is no per-workspace graph table (unlike vector storage). This is a design trade-off:
- **Pro**: Single graph allows cross-workspace knowledge queries
- **Con**: Graph cleanup operations scan all workspaces; no hard isolation guarantee

### GAP-8: No Atomicity  
**Severity**: Low (existing design constraint)  
The delete cascade (KV → vector → graph → PDF table) is non-atomic. A partial failure at any step leaves inconsistent state. Currently no compensation/rollback logic. Best-effort logging exists but no retry or saga pattern.

### GAP-9: "Knowledge Document" / Injected Entity Not Exempted  
**Severity**: Low  
Business rule: entities created via the Injection API (permanent knowledge base entries) should NOT be deleted even if all regular source documents are removed. Currently any entity with empty `source_ids` after document removal is deleted unconditionally.

---

## ADR: Approach to Bullet-Proof Deletion

### Decision 1: Fix DRY — Single Source of Truth for Graph Cleanup

**Decision**: Extend `cleanup_document_graph_data()` to accept `source_prefixes: &[String]` instead of a single `document_id`. Update `single.rs` to call this shared function.

**Rationale**:
- Reduces code surface from ~300 lines in single.rs to ~10 lines delegation
- All future bug fixes apply to all callers uniformly
- The mismatch case (key/id divergence) is handled by passing both prefixes

### Decision 2: Fix Performance — Eliminate Redundant Graph Call

**Decision**: In graph cleanup, track which nodes were deleted during node processing to compute `remaining_node_ids` locally, avoiding the second `get_all_nodes()` call.

**Rationale**: O(2N) to O(N) reduction in graph operations with no semantic change.

### Decision 3: Add Workspace_id Filtering to Graph Cleanup

**Decision**: When `workspace_id` is known, filter nodes by `workspace_id` property during cleanup to avoid scanning foreign-workspace data.

**Rationale**: Reduces the blast radius of bugs and improves performance for large multi-tenant deployments.

### Decision 4: Fix Bulk Delete — Entity Embeddings and Hash Keys

**Decision**: During bulk delete, when entity nodes are deleted, also delete their entity embeddings from vector storage. Also add content hash key cleanup.

### Decision 5: Preserve "Injected" Entities

**Decision**: Add `is_injected: bool` check — skip deletion of nodes that have `injected: true` property (set by Injection API).

---

## Edge Cases to Handle

| #   | Edge Case                                         | Current Behavior                           | Required Behavior                 |
| --- | ------------------------------------------------- | ------------------------------------------ | --------------------------------- |
| E1  | KV key prefix ≠ JSON `id` field (legacy mismatch) | Handled via `source_prefixes`              | Preserved ✓                       |
| E2  | Document being processed when deleted             | Task cancelled before delete               | ✓ (single only; bulk skips)       |
| E3  | Workspace record deleted before document          | Vector cleanup degrades to default storage | ✓ (lenient helper)                |
| E4  | Entity shared between two documents               | Source removed, entity updated             | ✓                                 |
| E5  | Entity with no sources after deletion             | Entity deleted                             | ✓                                 |
| E6  | Injected entity (Knowledge Document)              | Deleted with empty sources                 | **GAP-9: WIP**                    |
| E7  | Orphaned edges (endpoint deleted)                 | Detected and deleted                       | ✓                                 |
| E8  | Bulk delete while documents processing            | Skipped (no cancellation)                  | Partial — GAP-6                   |
| E9  | Re-upload after delete blocked by hash            | Hash key deleted in single delete          | Bulk: **GAP-5 fixed**             |
| E10 | Entity embedding orphan after bulk delete         | Not cleaned up                             | **GAP-4 fixed**                   |
| E11 | Graph scan across workspaces                      | All workspaces scanned                     | **GAP-2: workspace filter added** |

---

## Implementation / Improvement Plan

- [x] **Step 1**: Update mission file with analysis (this document)
- [x] **Step 2**: Extend `cleanup_document_graph_data()` to accept `source_prefixes: &[String]` and `workspace_id: Option<&str>` for isolation
- [x] **Step 3**: Refactor `single.rs` to delegate graph cleanup to shared function (eliminate 200+ LoC duplication)
- [x] **Step 4**: Fix `bulk.rs` — add entity embedding cleanup + content hash key cleanup + task cancellation
- [x] **Step 5**: Run full test suite to verify no regressions
- [x] **Step 6**: Clippy + format pass


