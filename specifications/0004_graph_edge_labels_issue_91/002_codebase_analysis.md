# Codebase Analysis: Display Link Type in Graph View

**Issue**: [#91](https://github.com/raphaelmansuy/edgequake/issues/91)

---

## Backend: Edge Data Pipeline

### 1. Graph Storage (`edgequake-storage/src/traits/graph.rs`)

```rust
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub properties: HashMap<String, Value>,
}
```

The `relation_type` is stored inside `properties` as a key:
```
properties: { "relation_type": "WORKS_FOR", "weight": 1.0, "description": "..." }
```

### 2. API Response (`edgequake-api/src/handlers/graph_types.rs`)

```rust
pub struct GraphEdgeResponse {
    pub source: String,
    pub target: String,
    pub edge_type: String,       // Extracted from properties["relation_type"]
    pub weight: f32,
    pub properties: serde_json::Value,  // Full properties blob
}
```

**Note**: No `#[serde(rename)]` attribute is used. Field serializes as `edge_type`.

### 3. Edge Construction Sites

There are 4 places where `GraphEdgeResponse` is constructed from graph edges:

| File | Line | How `edge_type` is set |
|------|------|----------------------|
| `graph_stream.rs` | ~262 | `properties.get("relation_type")` |
| `graph_query/search.rs` | ~170 | `properties.get("relationship_type")` ← different key! |
| `graph_query/traversal.rs` | ~121 | `properties.get("relation_type")` |
| `graph_query/traversal.rs` | ~241 | `properties.get("relation_type")` |

**Inconsistency**: `search.rs` reads `relationship_type` while all others
read `relation_type`. The merger writes `relation_type`. This means the search
endpoint may also produce wrong edge types.

### 4. Relationship Helpers (`relationships/helpers.rs`)

```rust
pub fn extract_relation_type(keywords: &str) -> String { ... }
pub fn edge_to_relationship_response(edge: &GraphEdge) -> RelationshipResponse {
    // reads edge.properties.get("relation_type")
}
```

---

## Frontend: Edge Rendering Pipeline

### 1. Types (`types/index.ts`)

```typescript
interface GraphEdge {
    id: string;
    source: string;
    target: string;
    relationship_type: string;    // Expected field name
    weight: number;
    description?: string;
    source_ids: string[];
    created_at: string;
}
```

### 2. SSE Stream (`lib/api/edgequake.ts`)

```typescript
type GraphStreamEvent =
  | { type: "edges"; edges: GraphEdge[] }
  | ...
```

The raw JSON from the server has `edge_type`, but the type says
`relationship_type`. No transformation is applied — the JSON is used as-is.

### 3. Graph Renderer (`components/graph/graph-renderer.tsx`)

Two edge addition paths:

**Streaming path** (line ~168):
```typescript
graph.addEdgeWithKey(edgeId, edge.source, edge.target, {
    label: edge.relationship_type,  // undefined → no label
    ...
});
```

**Initial load path** (line ~362):
```typescript
graph.addEdge(edge.source, edge.target, {
    label: edge.relationship_type,  // undefined → no label
    ...
});
```

### 4. Sigma.js Configuration (line ~505):

```typescript
const sigma = new Sigma(graph, container, {
    renderEdgeLabels: showEdgeLabels && !isVeryLargeGraph,
    defaultEdgeType: 'curvedArrow',
    edgeProgramClasses: {
        curvedArrow: EdgeCurvedArrowProgram,
    },
    // No edgeLabelSize configured
    // No edgeLabelColor configured
});
```

### 5. Graph Controls (`components/graph/graph-controls.tsx`)

Toggle exists at line ~206:
```typescript
checked={graphSettings.showEdgeLabels ?? false}
onCheckedChange={(checked) => setGraphSettings({ showEdgeLabels: checked })}
```

### 6. Settings Store (`stores/use-settings-store.ts`)

```typescript
showEdgeLabels: false,  // Default: off
```

---

## The Complete Bug Chain

```
+--------------------------------------------------------------------+
|  BUG CHAIN: WHY EDGE LABELS DON'T DISPLAY                         |
|                                                                    |
|  1. PostgreSQL stores:                                             |
|     edge properties = { "relation_type": "WORKS_FOR" }             |
|                                                                    |
|  2. Backend extracts:                                              |
|     edge_type = properties.get("relation_type")  // "WORKS_FOR"    |
|                                                                    |
|  3. Backend serializes JSON:                                       |
|     { "edge_type": "WORKS_FOR", ... }                              |
|     ^^^^^^^^^^^^                                                   |
|     Field name: "edge_type"                                        |
|                                                                    |
|  4. Frontend receives JSON, types as GraphEdge:                    |
|     GraphEdge { relationship_type: ??? }                           |
|                   ^^^^^^^^^^^^^^^^^                                |
|     "relationship_type" not in JSON → undefined                    |
|                                                                    |
|  5. Graph renderer sets:                                           |
|     graph.addEdge(..., { label: undefined })                       |
|                                                                    |
|  6. Sigma.js receives no label → renders no text                   |
|                                                                    |
|  RESULT: No edge labels ever display                               |
+--------------------------------------------------------------------+
```

---

## Secondary Issues

### Issue A: Sigma.js `edgeLabelSize` not configured

Even with correct label values, Sigma.js may not render edge labels at
small zoom levels. The `edgeLabelSize` setting controls minimum label size.
Current config does not set it (uses default which may be too small).

### Issue B: Edge label color not configured

`edgeLabelColor` is not set. In dark mode, labels may render in a color
invisible against the background.

### Issue C: `GraphEdgeResponse` has 5 fields, `GraphEdge` has 8

The frontend type expects `id`, `description`, `source_ids`, `created_at`
which are not sent by the backend. These should either be:
- Added to `GraphEdgeResponse`, or
- Removed from `GraphEdge`, or
- Made optional in `GraphEdge`

### Issue D: Property key inconsistency in `search.rs`

`search.rs` reads `relationship_type` from properties while all other
endpoints read `relation_type`. The correct key (what the merger writes)
is `relation_type`.
