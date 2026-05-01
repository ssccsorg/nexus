# Candidate Solutions: Display Link Type in Graph View

**Issue**: [#91](https://github.com/raphaelmansuy/edgequake/issues/91)

---

## Candidate A: Backend Rename (Recommended)

### Concept

Rename `GraphEdgeResponse.edge_type` to `relationship_type` in the backend
using `#[serde(rename)]`. Add missing fields. Fix `search.rs` property key.

### Changes

**Backend**:
```rust
// graph_types.rs
pub struct GraphEdgeResponse {
    pub source: String,
    pub target: String,
    #[serde(rename = "relationship_type")]
    pub edge_type: String,
    pub weight: f32,
    pub description: Option<String>,    // NEW
    #[serde(default)]
    pub source_ids: Vec<String>,        // NEW
    pub properties: serde_json::Value,
}
```

All 4 construction sites updated to populate `description` from
`properties.get("description")` and `source_ids` from `properties.get("source_id")`.

Fix `search.rs` to use `relation_type` instead of `relationship_type`.

**Frontend**: No changes needed — `GraphEdge.relationship_type` already matches.

**Sigma.js config** — add edge label styling:
```typescript
edgeLabelSize: 12,
edgeLabelColor: { color: isDark ? '#9ca3af' : '#6b7280' },
```

### Pros

- Minimal change: 1 struct + 4 construction sites + 2 Sigma settings
- No frontend type changes
- Fixes root cause (field name mismatch)
- `serde(rename)` keeps Rust field idiomatic while JSON matches frontend
- Fix search.rs property key inconsistency

### Cons

- API breaking change if external consumers use `edge_type` field name
- Requires coordinated deploy (backend before frontend or vice versa)

---

## Candidate B: Frontend Mapping

### Concept

Add a transformation in the frontend SSE handler that maps `edge_type` to
`relationship_type`. Keep backend unchanged.

### Changes

```typescript
// use-graph-stream.ts or lib/api/edgequake.ts
case "edges":
    const mappedEdges = event.edges.map(e => ({
        ...e,
        relationship_type: (e as any).edge_type || e.relationship_type || 'RELATED_TO',
        id: `${e.source}-${e.target}`,
        source_ids: [],
        created_at: new Date().toISOString(),
    }));
    setEdges(mappedEdges);
```

### Pros

- No backend changes
- No API breaking change
- Can handle both old and new field names

### Cons

- `as any` type cast is fragile
- Masks the real issue (mismatched contract)
- Every consumer of edge data must apply the mapping
- Doesn't fix search.rs property key bug
- Technical debt: frontend patching a backend issue

---

## Candidate C: Both Sides + Full Contract

### Concept

Fix backend to send `relationship_type`. Fix frontend type to make optional
fields truly optional. Add comprehensive edge label styling.

### Backend changes (same as A)

### Frontend changes:

```typescript
export interface GraphEdge {
    id?: string;             // Made optional (generated client-side if absent)
    source: string;
    target: string;
    relationship_type: string;
    weight: number;
    description?: string;
    source_ids?: string[];   // Made optional
    properties?: Record<string, unknown>;
    created_at?: string;     // Made optional
}
```

### Sigma.js changes:

```typescript
const sigma = new Sigma(graph, container, {
    renderEdgeLabels: showEdgeLabels && !isVeryLargeGraph,
    edgeLabelSize: 11,
    edgeLabelFont: 'Inter, ui-sans-serif, system-ui, sans-serif',
    edgeLabelColor: {
        color: isDark ? '#9ca3af' : '#6b7280',
    },
    edgeLabelWeight: '400',
    // ...existing settings
});
```

### Pros

- Fixes root cause + secondary issues
- Clean contract between backend and frontend
- Robust edge label rendering

### Cons

- More changes to coordinate
- Frontend type change may require updating other consumers

---

## Recommendation

**Candidate A: Backend Rename** for minimal fix scope, combined with the
Sigma.js styling additions from Candidate C.

Rationale:
1. Root cause is the field name mismatch — fixing it at the source is correct
2. `#[serde(rename)]` is the idiomatic Rust way to handle JSON naming differences
3. Sigma.js label styling is essential for labels to be visible
4. Frontend `GraphEdge` optional fields can be addressed in a follow-up PR
5. `search.rs` property key fix prevents silent bugs in graph search results
