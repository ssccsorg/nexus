# UX Spec — SPEC-0003: Query Explainability

**Issue**: #128 | **ADR**: [004_adr.md](004_adr.md) | **Status**: Proposed

---

## Placement

Three integration points — all inside existing components, no new routes:

1. **Explain toggle** — added to `query-settings-sheet.tsx` as one more `Switch`
2. **Explain panel** — rendered below each assistant message in `chat-message.tsx`
3. **Entity provenance** — click handler on entity badges in `source-citations.tsx`

---

## 1. Explain Toggle

Added as a row in the query settings sheet, same pattern as existing toggles:

```
  Query Settings (sheet / popover)
  ─────────────────────────────────
  Mode          [hybrid ▾]
  Top K         [10    ▾]
  Stream        ──●            ← Switch scale-75
  Rerank        ●──
  Explain       ──●            ← NEW, same row pattern
```

Default: OFF. Stored in `use-settings-store.ts → querySettings.explain`.

---

## 2. Explain Panel (below assistant message)

Follows the `ThinkingSection` pattern from `chat-message.tsx` — collapsible disclosure with a chevron icon. Collapsed by default.

### Collapsed

```
  ┌──────────────────────────────────────────────┐
  │  AI: The OEE drop was caused by...           │
  │                                              │
  │  ● ● ● ○ ○  High confidence       text-xs   │
  │                                              │
  │  ▸ Retrieval reasoning              text-xs  │
  │                                              │
  │  ┌ Sources ────────────────────────────────┐ │
  │  │ Entities | Documents | Chunks | Rels    │ │
  │  └────────────────────────────────────────-┘ │
  └──────────────────────────────────────────────┘

  Confidence uses existing ConfidenceDots pattern (5 dots):
    ●●●●● Very high  |  ●●●●○ High  |  ●●●○○ Medium
    ●●○○○ Low        |  ●○○○○ Very low
  Colors: filled = slate-600/400, empty = slate-200/700
```

### Expanded

```
  ┌──────────────────────────────────────────────┐
  │  AI: The OEE drop was caused by...           │
  │                                              │
  │  ● ● ● ○ ○  High confidence                 │
  │                                              │
  │  ▾ Retrieval reasoning                       │
  │                                              │
  │    1 · Query analysis              text-xs   │
  │      Keywords: OEE, drop, cause              │
  │      Mode: hybrid                            │
  │                                              │
  │    2 · Vector search                 12ms    │
  │      47 candidates → 20 selected             │
  │      min_score: 0.1                          │
  │                                              │
  │    3 · Graph traversal                5ms    │
  │      12 entities · 28 edges                  │
  │      Path: OEE → MACHINE_A → DEFECT_X       │
  │                                              │
  │    4 · Context assembly                      │
  │      4,200 / 30,000 tokens                   │
  │      15 chunks · 5 truncated                 │
  │                                              │
  │    5 · Rerank                        45ms    │
  │      20 → 10 results                         │
  │                                              │
  │  ┌ Sources ────────────────────────────────┐ │
  │  │ Entities | Documents | Chunks | Rels    │ │
  │  └────────────────────────────────────────-┘ │
  └──────────────────────────────────────────────┘
```

Each step is a flat row — no nested cards. Duration in `text-muted-foreground` aligned right. Step number in `text-[10px]` circle.

---

## 3. Entity Provenance Dialog

Triggered by clicking an entity badge in `source-citations.tsx`. Uses existing `Dialog` component.

```
  ┌─ Entity: OEE ────────────────────────┐
  │                                      │
  │  Type: CONCEPT · Seen 5×    text-xs  │
  │                                      │
  │  Sources                             │
  │  ─────                               │
  │  manufacturing_report.pdf            │
  │    chunk-3, chunk-7                  │
  │    Method: LLM · 2025-01-15         │
  │                                      │
  │  oee_definitions.pdf                 │
  │    chunk-1                           │
  │    Method: rule-based · 2025-01-18   │
  │                                      │
  │  Description                         │
  │  ─────                               │
  │  Overall Equipment Effectiveness,    │
  │  a key manufacturing metric...       │
  │                                      │
  │  [View in Graph]          [Close]    │
  └──────────────────────────────────────┘
```

---

## Components

| Component                | File                                            | Pattern                   |
| ------------------------ | ----------------------------------------------- | ------------------------- |
| `ExplainPanel`           | `components/query/explain-panel.tsx`            | Collapsible, ChevronRight |
| `ConfidenceDots`         | (reuse existing in source-citations.tsx)        | 5-dot display             |
| `EntityProvenanceDialog` | `components/query/entity-provenance-dialog.tsx` | Dialog                    |

Note: `ConfidenceBadge` is NOT a new component. Reuse the existing `ConfidenceDots` from `source-citations.tsx` — just export it.

---

## Type Extensions

```typescript
// types/index.ts additions
interface QueryRequest {
  // ... existing
  explain?: boolean;
}

interface ExplainTrace {
  query_analysis: { keywords: string[]; mode: string; reason: string };
  steps: RetrievalStep[];
  context: {
    tokens_used: number;
    budget: number;
    chunks: number;
    truncated: number;
  };
  confidence: {
    overall: number;
    coverage: number;
    similarity: number;
    graph: number;
  };
}

interface RetrievalStep {
  step: "vector_search" | "graph_traversal" | "rerank";
  candidates?: number;
  selected?: number;
  duration_ms: number;
}

interface QueryResponse {
  // ... existing
  explain?: ExplainTrace;
}
```

---

## Store Changes

```typescript
// use-settings-store.ts
defaultQuerySettings.explain = false;
```

---

## Validation Rules

| #   | Rule                                                          |
| --- | ------------------------------------------------------------- |
| 1   | Explain toggle OFF by default (no perf penalty)               |
| 2   | When ON, confidence dots appear on every response             |
| 3   | Explain panel collapsed by default — click chevron to expand  |
| 4   | Confidence uses existing ConfidenceDots — no red/yellow/green |
| 5   | Entity click in citations opens provenance dialog             |
| 6   | Duration shown in `text-muted-foreground` per step            |

---

## Design Alignment

- Follows `ThinkingSection` pattern exactly: chevron toggle, `text-xs`, collapsed default
- Reuses `ConfidenceDots` (already in source-citations.tsx) — no progress bars
- Entity provenance: compact `Dialog` with `text-xs` rows, no nested cards
- Colors: all neutral slate/blue — no confidence-mapped reds/yellows/greens
- Step layout: flat numbered rows, not accordion — keeps density high
- Path display: entity names as inline `text-[11px]` with `→` separator
