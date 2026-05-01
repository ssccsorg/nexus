# Codebase Analysis: Custom Entity Configuration

**Issue**: [#85](https://github.com/raphaelmansuy/edgequake/issues/85)

---

## Backend Entity Type Infrastructure

### 1. Default Entity Types (`edgequake-pipeline/src/prompts/mod.rs`)

```rust
pub fn default_entity_types() -> Vec<String> {
    vec![
        "PERSON", "ORGANIZATION", "LOCATION", "EVENT",
        "CONCEPT", "TECHNOLOGY", "PRODUCT", "DATE", "DOCUMENT"
    ]
}
```

9 types. Used by `SOTAExtractor::new()` and `LLMExtractor::new()`.

### 2. SOTA Extractor (`edgequake-pipeline/src/extractor/sota.rs`)

```rust
pub struct SOTAExtractor<L> {
    llm_provider: Arc<L>,
    entity_types: Vec<String>,  // ← Configurable at construction
    // ...
}

impl SOTAExtractor<L> {
    pub fn new(llm: Arc<L>) -> Self {
        Self {
            entity_types: crate::prompts::default_entity_types(),
            // ...
        }
    }

    pub fn with_entity_types(mut self, types: Vec<String>) -> Self {
        self.entity_types = types;
        self
    }
}
```

`entity_types` is passed to `EntityExtractionPrompts::system_prompt()` which
injects them into the LLM prompt:

```
Categorize the entity using one of the following types: `{entity_types}`.
If none of the provided entity types apply, classify it as `Other`.
```

### 3. LLM Extractor (`edgequake-pipeline/src/extractor/llm.rs`)

Same pattern:
```rust
pub struct LLMExtractor<L> {
    entity_types: Vec<String>,
    // ...
}

impl LLMExtractor<L> {
    pub fn new(llm: Arc<L>) -> Self {
        Self {
            entity_types: vec!["PERSON", "ORGANIZATION", ...],  // 7 types (not 9!)
        }
    }

    pub fn with_entity_types(mut self, types: Vec<String>) -> Self {
        self.entity_types = types;
        self
    }
}
```

**Note**: LLMExtractor defaults to 7 types (missing DATE, DOCUMENT).
SOTAExtractor defaults to 9 types. This inconsistency should be fixed.

### 4. Prompt System (`edgequake-pipeline/src/prompts/entity_extraction.rs`)

Two prompt methods accept entity types:

SOTA prompt:
```rust
pub fn system_prompt(&self, entity_types: &[impl AsRef<str>], language: &str) -> String
```

LLM prompt:
```rust
pub fn extraction_prompt(&self, entity_types: &[impl AsRef<str>], text: &str, ...) -> String
```

Both format types as comma-separated list in the prompt text.

---

## Workspace Configuration

### 5. `CreateWorkspaceRequest` (`edgequake-core/src/types/multitenancy/requests.rs`)

Current fields:
```
name, slug, description, max_documents,
llm_model, llm_provider,
embedding_model, embedding_provider, embedding_dimension,
vision_llm_model, vision_llm_provider
```

**No `entity_types` field.** This is the primary gap.

### 6. Workspace Metadata Storage (`workspace_service_impl.rs`)

Workspace metadata is stored as JSONB in PostgreSQL:
```sql
INSERT INTO workspaces (id, tenant_id, name, slug, metadata, ...)
VALUES ($1, $2, $3, $4, $5::jsonb, ...)
```

The `metadata` column already stores LLM config, embedding config, vision
config. Entity types can be added to the same JSONB.

### 7. Pipeline Creation in Upload Handler

When a document is uploaded (`file_upload.rs`), the pipeline is created with
workspace-level LLM/embedding providers but entity types are not configured:

```rust
// Current: no entity_types are read from workspace
let extractor = SOTAExtractor::new(llm_provider.clone());
// Missing: .with_entity_types(workspace_entity_types)
```

---

## Frontend

### 8. `CreateWorkspaceRequest` Type (`types/index.ts`)

```typescript
export interface CreateWorkspaceRequest {
    name: string;
    slug?: string;
    description?: string;
    max_documents?: number;
    llm_model?: string;
    llm_provider?: string;
    embedding_model?: string;
    embedding_provider?: string;
    embedding_dimension?: number;
    vision_llm_model?: string;
    vision_llm_provider?: string;
    // No entity_types field
}
```

### 9. Workspace Creation Dialog (`tenant-workspace-selector.tsx`)

The workspace creation form has:
- Name input
- LLM model/provider selection
- Embedding model/provider selection
- Vision LLM configuration

**No entity type selector.**

---

## Data Flow Gap

```
+--------------------------------------------------------------------+
|  CURRENT FLOW (entity types hardcoded)                             |
|                                                                    |
|  User creates workspace                                            |
|    → CreateWorkspaceRequest { name, llm_model, ... }               |
|                   No entity_types field                            |
|    → Workspace stored with metadata { llm_config, ... }            |
|                   No entity_types in metadata                      |
|                                                                    |
|  User uploads document                                             |
|    → Pipeline created with SOTAExtractor::new(llm)                 |
|                   default_entity_types() hardcoded                 |
|    → Entity extraction with default types only                     |
|    → Domain entities missed                                        |
+--------------------------------------------------------------------+

+--------------------------------------------------------------------+
|  DESIRED FLOW (entity types configurable)                          |
|                                                                    |
|  User creates workspace with entity types                          |
|    → CreateWorkspaceRequest { name, entity_types: [...], ... }     |
|    → Workspace stored with metadata { entity_types: [...], ... }   |
|                                                                    |
|  User uploads document                                             |
|    → Pipeline reads entity_types from workspace metadata           |
|    → SOTAExtractor::new(llm).with_entity_types(workspace_types)    |
|    → Entity extraction with domain-specific types                  |
|    → Domain entities captured correctly                             |
+--------------------------------------------------------------------+
```

---

## Implications

### Existing Workspaces

All existing workspaces have `metadata` without `entity_types`. The system
must fall back to `default_entity_types()` when the key is absent.

### Re-extraction

Changing entity types on an existing workspace does NOT retroactively re-extract
entities from already-processed documents. Only new uploads use the new types.
A future "rebuild workspace" feature could re-process all documents.

### Type Validation

Entity types should be:
- UPPERCASE with underscores (e.g., "MACHINE", "DEFECT_TYPE")
- Non-empty strings
- Maximum 20 types per workspace (to avoid prompt bloat)
- No duplicates

### LLM Prompt Impact

Adding domain-specific types increases prompt length. With 20 types vs 9, the
prompt grows by ~100 tokens. This is negligible compared to chunk content.
