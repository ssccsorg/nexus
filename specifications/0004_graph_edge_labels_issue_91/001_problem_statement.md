# Problem Statement: Display Link Type (Relation) in the Graph View

**Issue**: [#91](https://github.com/raphaelmansuy/edgequake/issues/91)

---

## WHY

Knowledge graphs are fundamentally about **typed relationships**. An edge
connecting "Alice" to "Acme Corp" means nothing without knowing whether Alice
*works for*, *founded*, or *sued* Acme Corp. The relationship type IS the
knowledge.

EdgeQuake's graph view renders nodes and edges correctly. A toggle "Show Edge
Labels" exists in both the Settings page and Graph Controls panel. Users can
enable it, and the code sets `renderEdgeLabels: true` on the Sigma.js instance.

**But edge labels never display.** The user-reported issue includes a screenshot
confirming edges render without labels even when the toggle is active.

This is not a cosmetic issue — it fundamentally undermines the graph
visualization's purpose. Without relationship labels, users cannot:

- Distinguish between different types of connections
- Validate extraction quality (is this edge type correct?)
- Navigate the graph meaningfully (which edges to follow?)
- Trust the knowledge graph representation

---

## Root Cause Analysis

### Finding: Field Name Mismatch (Backend → Frontend)

The backend API response type `GraphEdgeResponse` serializes the relationship
type as `edge_type`:

```rust
// edgequake-api/src/handlers/graph_types.rs
pub struct GraphEdgeResponse {
    pub source: String,
    pub target: String,
    pub edge_type: String,      // <-- Rust field, serializes as "edge_type"
    pub weight: f32,
    pub properties: serde_json::Value,
}
```

The frontend TypeScript type `GraphEdge` expects `relationship_type`:

```typescript
// edgequake_webui/src/types/index.ts
export interface GraphEdge {
    id: string;
    source: string;
    target: string;
    relationship_type: string;   // <-- Expected by frontend
    weight: number;
    description?: string;
    source_ids: string[];
    properties?: Record<string, unknown>;
    created_at: string;
}
```

The SSE stream event is typed as `{ type: "edges"; edges: GraphEdge[] }`.
When the JSON arrives with `edge_type`, TypeScript silently ignores it (no
runtime type checking). The `relationship_type` field is `undefined`.

In the graph renderer:
```typescript
graph.addEdge(edge.source, edge.target, {
    label: edge.relationship_type,  // === undefined → no label
    ...
});
```

### Additional Finding: Missing Fields

`GraphEdgeResponse` only sends 5 fields: `source`, `target`, `edge_type`,
`weight`, `properties`. But `GraphEdge` expects 8 fields including `id`,
`description`, `source_ids`, `created_at`. These are all `undefined`.

### Additional Finding: Sigma.js Edge Label Rendering

Even if the field name were correct, Sigma.js edge labels have additional
requirements:

1. The `renderEdgeLabels: true` setting (already set)
2. The `EdgeCurvedArrowProgram` renders the edge curve/arrow but labels are
   rendered separately by Sigma's built-in canvas label renderer
3. Edge labels require `edgeLabelSize` to be configured
4. The edge label rendered size threshold must be met at current zoom level

---

## Success Criteria

1. Edge labels display the relationship type when "Show Edge Labels" is enabled
2. Labels are readable (appropriate size and color)
3. Labels work with curved arrow edges
4. Labels adapt to dark/light mode
5. Performance: labels are hidden for very large graphs (>500 nodes)
6. The field name mismatch between backend and frontend is resolved
