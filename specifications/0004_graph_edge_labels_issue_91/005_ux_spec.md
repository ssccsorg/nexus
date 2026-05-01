# UX Spec — SPEC-0004: Graph Edge Labels

**Issue**: #91 | **ADR**: [004_adr.md](004_adr.md) | **Status**: Proposed

---

## Problem

The "Show Edge Labels" toggle in `graph-controls.tsx` exists but labels never render. Two root causes:

1. **Backend**: `GraphEdgeResponse` serializes `edge_type` but frontend expects `relationship_type`
2. **Frontend**: Sigma.js constructor missing `edgeLabelSize`, `edgeLabelColor`, `edgeLabelFont`

---

## Fix 1: Backend (serde rename)

```rust
// edgequake-api/src/handlers/graph_types.rs
#[serde(rename = "relationship_type")]
pub edge_type: String,
```

Zero UI change needed — JSON field name matches `GraphEdge.relationship_type`.

---

## Fix 2: Frontend (Sigma config)

```typescript
// components/graph/graph-renderer.tsx — Sigma constructor
new Sigma(graph, container, {
  renderEdgeLabels: showEdgeLabels,
  edgeLabelSize: 11, // ADD
  edgeLabelColor: {
    // ADD
    color: isDark ? "#9ca3af" : "#6b7280",
  },
  edgeLabelFont: "Inter, ui-sans-serif, sans-serif", // ADD
});
```

---

## Existing UI (no changes needed)

The toggle already exists in `graph-controls.tsx` at the correct position:

```
  ┌─ Graph Controls ─────── w-56 ───┐
  │                                  │
  │  Layout                          │
  │  ForceAtlas2              ▾      │
  │                                  │
  │  ─────────────────               │
  │                                  │
  │  Appearance                      │
  │  Show Labels          ──●        │
  │  Show Edge Labels     ●──   ← already wired
  │  Highlight Neighbors  ──●        │
  │                                  │
  │  ─────────────────               │
  │                                  │
  │  Display                         │
  │  Node Size    [S] [M] [L]       │
  └──────────────────────────────────┘

  Toggle pattern: text-[11px] label + Switch scale-75
```

---

## Components Changed

| File                                  | Change                        |
| ------------------------------------- | ----------------------------- |
| `components/graph/graph-renderer.tsx` | Add 3 Sigma config properties |
| Backend `graph_types.rs`              | Add `#[serde(rename)]`        |

No new components. No new files.

---

## Validation Rules

| #   | Rule                                                         |
| --- | ------------------------------------------------------------ |
| 1   | Edge labels hidden by default (`showEdgeLabels: false`)      |
| 2   | Labels show `relationship_type` (e.g. "WORKS_FOR")           |
| 3   | Light: gray-500 (`#6b7280`). Dark: gray-400 (`#9ca3af`)      |
| 4   | Labels auto-hide at low zoom (Sigma LOD behavior)            |
| 5   | Labels disabled for graphs > 500 nodes (existing perf guard) |

---

## Design Alignment

- No visual additions — pure bug fix + config
- Colors match existing node label color scheme (gray-500/400)
- Font matches UI font stack (Inter)
- Size 11px matches `text-[11px]` used across controls
