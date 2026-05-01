# Candidate Solutions: Custom Entity Configuration

**Issue**: [#85](https://github.com/raphaelmansuy/edgequake/issues/85)

---

## Candidate A: Workspace Metadata + Preset System (Recommended)

### Concept

Add `entity_types` to `CreateWorkspaceRequest`. Store in workspace JSONB
metadata. Read at pipeline creation time. Frontend provides preset templates
with a custom type editor.

### Backend Changes

**`CreateWorkspaceRequest`** — add field:
```rust
pub entity_types: Option<Vec<String>>,
```

**`workspace_service_impl.rs`** — persist to metadata:
```rust
if let Some(ref entity_types) = request.entity_types {
    metadata.insert("entity_types", json!(entity_types));
}
```

**Pipeline creation** (upload handler) — read from workspace:
```rust
let entity_types = workspace.metadata
    .get("entity_types")
    .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
    .unwrap_or_else(default_entity_types);

let extractor = SOTAExtractor::new(llm.clone())
    .with_entity_types(entity_types);
```

### Frontend Changes

**`CreateWorkspaceRequest`** type — add field:
```typescript
entity_types?: string[];
```

**Workspace creation dialog** — add entity type section:

```
+--------------------------------------------------------------------+
|  CREATE WORKSPACE                                                  |
|                                                                    |
|  Name: [Manufacturing QA          ]                                |
|                                                                    |
|  Entity Type Preset:                                               |
|  [ General   ] [ Manufacturing ] [ Healthcare ] [ Custom... ]      |
|        ↓                                                           |
|  Selected Entity Types:                                            |
|  ┌──────────────────────────────────────────────────────┐         |
|  │  ☑ PERSON    ☑ ORGANIZATION   ☑ LOCATION            │         |
|  │  ☑ EVENT     ☑ CONCEPT        ☑ TECHNOLOGY          │         |
|  │  ☑ PRODUCT   ☑ DATE           ☑ DOCUMENT            │         |
|  │  ☑ MACHINE   ☑ COMPONENT      ☑ DEFECT              │         |
|  │  ☑ MEASUREMENT  ☑ PROCESS                           │         |
|  │                                                      │         |
|  │  + Add custom type: [______________] [Add]           │         |
|  └──────────────────────────────────────────────────────┘         |
|                                                                    |
|  LLM Model: [gemma3:12b        ▼]                                |
|  ...                                                               |
|                                                                    |
|  [Cancel]                                [Create Workspace]        |
+--------------------------------------------------------------------+
```

### Presets

| Preset | Entity Types |
|--------|-------------|
| General | PERSON, ORGANIZATION, LOCATION, EVENT, CONCEPT, TECHNOLOGY, PRODUCT, DATE, DOCUMENT |
| Manufacturing | General + MACHINE, COMPONENT, DEFECT, MEASUREMENT, PROCESS, MATERIAL |
| Healthcare | General - TECHNOLOGY + SYMPTOM, DRUG, DIAGNOSIS, PROCEDURE, PATIENT, CONDITION |
| Legal | General - TECHNOLOGY + CONTRACT, CLAUSE, PARTY, REGULATION, JURISDICTION, CASE |
| Research | General - PRODUCT + PAPER, METHOD, DATASET, HYPOTHESIS, FINDING, METRIC |
| Custom | User starts from General and adds/removes |

### Pros

- Minimal backend change (1 new field, metadata storage)
- Presets cover common domains
- Custom types enable unlimited domains
- Pipeline builder method already exists
- Backward compatible (falls back to default)

### Cons

- Entity types are part of creation only — no update API (v1)
- Presets are hardcoded in frontend (need update for new domains)
- No validation that types are effective for extraction

---

## Candidate B: Workspace Configuration Table

### Concept

Create a separate `workspace_config` table with typed columns for all pipeline
settings including entity types.

```sql
CREATE TABLE workspace_config (
    workspace_id UUID PRIMARY KEY REFERENCES workspaces(id),
    entity_types TEXT[] DEFAULT ARRAY['PERSON','ORGANIZATION',...],
    chunk_size INT DEFAULT 1200,
    chunk_overlap INT DEFAULT 100,
    gleaning_iterations INT DEFAULT 2,
    extraction_method TEXT DEFAULT 'sota',
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
```

### Pros

- Typed columns with database-level validation
- Easy to query and filter workspaces by config
- Can add more config fields later without JSONB parsing
- SQL-level defaults

### Cons

- Requires migration (new table)
- Join overhead on every pipeline creation
- Over-engineered for current scope (only need entity_types)
- Duplicates some metadata already in workspace JSONB

---

## Candidate C: Tenant-Level Entity Type Templates

### Concept

Entity type presets are stored per-tenant as templates. Workspaces reference
a template ID or override with custom types.

```rust
// Tenant has templates
tenant.metadata.entity_type_templates = {
    "general": ["PERSON", "ORGANIZATION", ...],
    "manufacturing": ["MACHINE", "COMPONENT", ...],
    "custom_1": ["PAPER", "METHOD", ...]
};

// Workspace references template or overrides
workspace.metadata.entity_type_template = "manufacturing";  // OR
workspace.metadata.entity_types = ["CUSTOM_1", "CUSTOM_2"]; // Override
```

### Pros

- Templates shared across workspaces within tenant
- Consistent configuration for similar workspaces
- Admin can manage templates centrally

### Cons

- Complexity: template inheritance + override logic
- More API surface (CRUD for templates)
- Over-engineered for initial release
- Template sharing is a rare use case

---

## Recommendation

**Candidate A: Workspace Metadata + Preset System**

Rationale:
1. Simplest implementation — one new field, one metadata key, one UI component
2. Pipeline `with_entity_types()` builder already exists in both extractors
3. Presets provide immediate value for 5+ domains
4. Custom types handle unlimited domains
5. Backward compatible with zero migration needed (JSONB is schemaless)
6. Can evolve to Candidate B or C later if requirements grow
