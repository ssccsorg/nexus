# Mission 03 — Configurable PDF → Markdown Parser

**Version**: 1.0.1 | **Date**: 2026-04-10 | **Branch**: `feat/edgequake-v0.10.0`
**Status**: IMPLEMENTED

## Implementation Status

Mission 03 is now implemented in code.

Code-is-law verification completed against the current tree:
- `EdgeParse` and `vision` are both runtime-selectable PDF parser backends.
- Resolution order implemented in code is `per-upload override -> workspace default -> env -> vision`.
- Workspace-level configuration is exposed on the workspace page.
- Per-upload override is exposed in the upload UI with a `Workspace Default` option.
- The processor records `pdf_extraction_method` and low-content warnings for EdgeParse outputs.
- EdgeParse output is sanitized before persistence to remove NUL bytes that PostgreSQL rejects.
- Failed/non-Vision document lineage no longer infers a fake Vision extraction step from stale
  `vision_model` data alone.

---

## 1. Context & Motivation

### Current State (Code is Law)

EdgeQuake today converts PDF → Markdown using **Vision LLM** exclusively:

```
Upload PDF
    │
    ▼
edgequake-pdf2md (LLM vision)
    │   Pages rendered as images → sent to Vision LLM
    │   (gpt-4.1-nano / gemma3:latest etc.)
    │
    ▼
Markdown string
    │
    ▼
Pipeline → Entity extraction → Knowledge Graph
```

Key files governing this today:
- `crates/edgequake-api/src/processor/pdf_processing.rs` — main processor
- `crates/edgequake-api/src/handlers/pdf_upload/types.rs` — upload options
- `crates/edgequake-api/src/handlers/pdf_upload/helpers.rs` — task construction
- `crates/edgequake-tasks/src/types/data.rs` — `PdfProcessingData` payload
- `crates/edgequake-storage/src/pdf_storage.rs` — `ExtractionMethod` enum
- `crates/edgequake-core/src/types/multitenancy/workspace.rs` — `Workspace` struct
- `crates/edgequake-api/Cargo.toml` — `edgequake-pdf2md = "=0.8.0"`

### Problem Statement

Vision LLM has real strengths (handles scanned PDFs, images-in-text, complex layouts),
but it is **slow** (minutes per large document), **costly** (API tokens per page), and
**non-deterministic** (LLM output varies). Many digital-native PDFs with embedded text need
none of this.

**[EdgeParse](https://edgeparse.com/)** (`edgeparse-core` crate, v0.2.3) is a CPU-only,
zero-ML Rust PDF parser that delivers:
- 83× faster than Docling (0.007 s/doc on M4 Max)
- Best-in-class benchmark score (78.1%, NID 0.885, TEDS 0.559)
- Zero GPU / OCR / JVM dependencies — single 15 MB binary
- Native Rust API: `edgeparse_core::convert(path, &config)` → `Document`
- Markdown output: `edgeparse_core::output::markdown::to_markdown(&doc)`
- Works from `&[u8]` bytes (no temp file needed if we use path-based API)

### Goal

Add `EdgeParse` as a selectable PDF parser backend alongside the existing Vision LLM.
Configurable at three levels (lowest wins):

```
ENV  >  Workspace setting  >  Per-upload override  >  Default (Vision)
```

**Default remains Vision LLM** — zero breaking change for existing deployments.

---

## 2. Architecture Decision Record (ADR)

### ADR-03-001 — Strategy Pattern for PDF Backends

**Status**: Accepted

**Decision**: Introduce a `PdfConverter` trait in a new `edgequake-pdf` crate.  
Two implementations: `VisionPdfConverter` (wraps `edgequake-pdf2md`) and
`EdgeParsePdfConverter` (wraps `edgeparse-core`). The processor selects the
concrete implementation at task-dispatch time.

**Alternatives considered**:
- Feature flags (`#[cfg(feature = "edgeparse")]`) — rejected: both backends must
  coexist at runtime, selectable without recompile.
- Direct enum match in `pdf_processing.rs` — rejected: violates OCP; adding a
  third backend later would require touching the processor again.

**Consequences**:
- Each backend is independently testable.
- Adding a new backend (e.g., Docling HTTP sidecar) requires only a new `impl PdfConverter`.
- `pdf_processing.rs` becomes backend-agnostic (just calls `converter.convert()`).

### ADR-03-002 — `edgequake-pdf` as the Abstraction Crate

**Status**: Accepted

The existing `crates/edgequake-pdf/` directory is empty (only `lib/libpdfium.dylib`).
We promote it to a proper Rust crate with:
- `PdfConverter` trait
- `PdfParserBackend` enum (serializable, env-parseable)
- `PdfConversionConfig` struct
- Backend factory: `create_pdf_converter(backend, options)`

This satisfies DRY: vision provider resolution logic currently scattered across
`types.rs`, `helpers.rs`, and `pdf_processing.rs` is consolidated here.

### ADR-03-003 — DB Migration for `extraction_method`

**Status**: Accepted

The `ExtractionMethod` enum gains a new variant `EdgeParse`. Since this is stored
in PostgreSQL as a `TEXT` column (not a PG ENUM), no DDL migration is needed.
The application-level enum simply adds a new variant.
Add a migration to update the column CHECK constraint if one exists.

### ADR-03-004 — No Vision Provider Required for EdgeParse

**Status**: Accepted

When `pdf_parser_backend = edgeparse`, the fields `vision_provider` and
`vision_model` in `PdfProcessingData` are ignored. No LLM API key is needed.
The processor MUST NOT call `create_safe_llm_provider()` in the EdgeParse path.
This is validated at task-creation time (warn if vision creds absent but
`vision` backend was explicitly requested).

### ADR-03-005 — Scanned PDF Fallback Policy

**Status**: Accepted

EdgeParse is CPU-only heuristic: scanned (image-only) PDFs produce near-empty
markdown. The system will:
1. Detect empty/near-empty markdown output from EdgeParse (< 50 chars per page on average).
2. Record `extraction_errors` with `"low_content_warning"`.
3. Store the (possibly poor) markdown — do not silently fallback to Vision.
4. Surface a UI warning: "Low text content — consider using Vision extraction."

Auto-fallback to Vision is out of scope (v0.10.0) — it introduces hidden cost
and non-determinism. Users must explicitly switch backend.

---

## 3. System Design

### 3.1 Component Diagram

```
  ┌──────────────────────────────────────────────────────────────────┐
  │                        edgequake-api                             │
  │                                                                  │
  │  ┌──────────────────────┐    ┌─────────────────────────────────┐ │
  │  │  PDF Upload Handler  │    │  Document Task Processor        │ │
  │  │  (handlers/pdf_upload│    │  (processor/pdf_processing.rs)  │ │
  │  │   /upload.rs)        │    │                                 │ │
  │  │                      │    │  backend = resolve_backend(data) │ │
  │  │  PdfUploadOptions    │    │  converter = create_converter() │ │
  │  │  + pdf_parser_backend│───▶│  markdown = converter.convert() │ │
  │  └──────────────────────┘    └────────────┬────────────────────┘ │
  │                                           │                      │
  └───────────────────────────────────────────┼──────────────────────┘
                                              │
              ┌───────────────────────────────┼────────────────────┐
              │           edgequake-pdf        │                    │
              │                               │                    │
              │   ┌───────────────────────────▼─────────────────┐  │
              │   │          trait PdfConverter                  │  │
              │   │  + convert(bytes, config) -> Result<String>  │  │
              │   └──────────────┬──────────────────────────────┘  │
              │                  │                                  │
              │      ┌───────────┴────────────┐                    │
              │      │                        │                    │
              │  ┌───▼───────────────┐  ┌────▼──────────────────┐  │
              │  │ VisionPdfConverter│  │EdgeParsePdfConverter   │  │
              │  │                   │  │                        │  │
              │  │ edgequake-pdf2md  │  │ edgeparse-core         │  │
              │  │  (LLM vision)     │  │  (CPU heuristic)       │  │
              │  └───────────────────┘  └────────────────────────┘  │
              └────────────────────────────────────────────────────┘
```

### 3.2 Configuration Resolution Order

```
  ┌────────────────────────────────────────────────────┐
  │         Backend Resolution (highest wins)          │
  ├────────────────────────────────────────────────────┤
  │  1. Per-upload form field: pdf_parser_backend      │
  │  2. Workspace setting: workspace.pdf_parser_backend│
  │  3. ENV: EDGEQUAKE_PDF_PARSER_BACKEND              │
  │  4. Default: "vision"                              │
  └────────────────────────────────────────────────────┘
```

### 3.3 Data Flow (Full Pipeline)

```
  Browser / CLI
       │
       │ POST /workspaces/{id}/pdfs  (multipart form)
       │  + pdf_parser_backend = "edgeparse" | "vision"
       ▼
  [pdf_upload/upload.rs]
       │ parse options → PdfUploadOptions { pdf_parser_backend }
       │ resolve_backend() → PdfParserBackend
       │ create_pdf_processing_task()
       ▼
  [Task Queue]
       │ PdfProcessingData { pdf_parser_backend, ... }
       ▼
  [pdf_processing.rs]
       │ create_pdf_converter(backend, &options)
       │    match backend {
       │       Vision     → VisionPdfConverter { llm_provider }
       │       EdgeParse  → EdgeParsePdfConverter { config }
       │    }
       │ markdown = converter.convert(&pdf_bytes, &conv_config).await?
       │ extraction_method = backend.to_extraction_method()
       ▼
  [Storage] save markdown + extraction_method
       ▼
  [Pipeline] entity extraction → Knowledge Graph
```

### 3.4 Workspace Setting Storage

`workspace.pdf_parser_backend` stored in the `metadata` JSON column (existing pattern),
serialized as `"vision"` or `"edgeparse"`. No schema migration required.

```rust
// In workspace.rs
pub struct Workspace {
    // ...existing fields...
    /// PDF parser backend for this workspace.
    /// None = use ENV / server default ("vision").
    pub pdf_parser_backend: Option<PdfParserBackend>,
}
```

---

## 4. Implementation Plan

### Phase 1 — New `edgequake-pdf` Crate (Abstraction Layer)

**Goal**: Define the `PdfConverter` trait and both implementations.

#### 4.1.1 Create `Cargo.toml`

File: `edgequake/crates/edgequake-pdf/Cargo.toml`

```toml
[package]
name = "edgequake-pdf"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
repository.workspace = true
description = "PDF → Markdown conversion backends for EdgeQuake"

[dependencies]
# Vision backend (always available)
edgequake-pdf2md = "=0.8.0"
edgequake-llm = { workspace = true }

# EdgeParse backend (CPU-only, zero ML)
edgeparse-core = "0.2"

async-trait.workspace = true
thiserror.workspace = true
tracing.workspace = true
serde = { workspace = true, features = ["derive"] }
tokio = { workspace = true, features = ["full"] }
```

#### 4.1.2 Create `src/lib.rs`

```
edgequake/crates/edgequake-pdf/src/
├── lib.rs          ← pub re-exports
├── backend/
│   ├── mod.rs      ← PdfParserBackend enum + factory
│   ├── trait.rs    ← PdfConverter trait + PdfConversionConfig
│   ├── vision.rs   ← VisionPdfConverter
│   └── edgeparse.rs ← EdgeParsePdfConverter
└── error.rs        ← PdfConversionError
```

**`src/backend/trait.rs`** — Core contract:

```rust
use async_trait::async_trait;
use crate::error::PdfConversionError;

/// Configuration for a single PDF conversion job.
#[derive(Debug, Clone, Default)]
pub struct PdfConversionConfig {
    /// Hint about document page count (used for adaptive concurrency).
    pub page_count_hint: Option<usize>,
    /// Table detection method for EdgeParse: "auto", "border", "cluster".
    pub table_method: Option<String>,
}

/// Unified PDF → Markdown conversion contract.
///
/// Each backend (Vision LLM, EdgeParse, …) implements this trait.
/// The trait is object-safe: all methods take &self or immutable refs.
///
/// # SOLID
/// - SRP: one method, one responsibility — produce Markdown from bytes.
/// - OCP: new backends add impl without changing call sites.
/// - DIP: callers depend on this trait, not concrete types.
#[async_trait]
pub trait PdfConverter: Send + Sync {
    /// Convert PDF bytes to a Markdown string.
    ///
    /// # Errors
    /// Returns `PdfConversionError` on any failure.
    async fn convert(
        &self,
        pdf_bytes: &[u8],
        config: &PdfConversionConfig,
    ) -> Result<String, PdfConversionError>;

    /// Human-readable backend name for logging and UI.
    fn backend_name(&self) -> &'static str;
}
```

**`src/backend/mod.rs`** — Backend enum & factory:

```rust
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::backend::trait_def::PdfConverter;

/// Selectable PDF parser backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PdfParserBackend {
    /// Vision LLM extraction (default). Handles scanned PDFs, images.
    #[default]
    Vision,
    /// EdgeParse CPU-only heuristic. Fast, deterministic, zero-cost per call.
    EdgeParse,
}

impl PdfParserBackend {
    /// Parse from environment variable string.
    pub fn from_env_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "vision" | "llm" => Some(Self::Vision),
            "edgeparse" | "edge_parse" | "edge-parse" => Some(Self::EdgeParse),
            _ => None,
        }
    }

    /// Read from EDGEQUAKE_PDF_PARSER_BACKEND, return None if unset/invalid.
    pub fn from_env() -> Option<Self> {
        std::env::var("EDGEQUAKE_PDF_PARSER_BACKEND")
            .ok()
            .filter(|s| !s.is_empty())
            .and_then(|s| Self::from_env_str(&s))
    }

    /// String form stored in workspace metadata JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vision    => "vision",
            Self::EdgeParse => "edgeparse",
        }
    }

    /// Map to the ExtractionMethod string used in PDF storage.
    pub fn extraction_method_str(self) -> &'static str {
        match self {
            Self::Vision    => "vision",
            Self::EdgeParse => "edgeparse",
        }
    }
}

/// Create a boxed `PdfConverter` for the requested backend.
///
/// Callers receive a trait object; they do not know which backend is active.
/// WHY Arc<dyn PdfConverter>: the converter may be shared across concurrent
/// chunk tasks; Arc avoids cloning the (potentially heavy) LLM provider.
pub fn create_pdf_converter(
    backend: PdfParserBackend,
    // Only used for Vision backend; ignored for EdgeParse.
    llm_provider: Option<Arc<dyn edgequake_llm::LlmProvider>>,
) -> Arc<dyn PdfConverter> {
    match backend {
        PdfParserBackend::Vision    => Arc::new(
            crate::backend::vision::VisionPdfConverter::new(llm_provider)
        ),
        PdfParserBackend::EdgeParse => Arc::new(
            crate::backend::edgeparse::EdgeParsePdfConverter::default()
        ),
    }
}
```

**`src/backend/edgeparse.rs`** — EdgeParse implementation:

```rust
use async_trait::async_trait;
use edgeparse_core::{convert as ep_convert, api::config::ProcessingConfig, output};
use std::io::Write;
use tempfile::NamedTempFile;
use tracing::{info, warn};

use super::trait_def::{PdfConversionConfig, PdfConverter};
use crate::error::PdfConversionError;

/// PDF converter backed by `edgeparse-core` (CPU-only, zero ML).
///
/// # Performance
/// Under Rayon parallelism: ~0.007 s/doc on M4 Max (83× faster than Docling).
/// Safe to call from async context via `spawn_blocking`.
///
/// # Scanned PDFs
/// EdgeParse cannot OCR; scanned-only PDFs will produce near-empty markdown.
/// The caller MUST check markdown length and surface a warning.
#[derive(Debug, Default)]
pub struct EdgeParsePdfConverter;

#[async_trait]
impl PdfConverter for EdgeParsePdfConverter {
    async fn convert(
        &self,
        pdf_bytes: &[u8],
        config: &PdfConversionConfig,
    ) -> Result<String, PdfConversionError> {
        let bytes = pdf_bytes.to_vec(); // move into blocking task
        let table_method = config.table_method.clone()
            .unwrap_or_else(|| "auto".to_string());

        // WHY spawn_blocking: edgeparse_core::convert is CPU-bound (Rayon).
        // Running it on the async executor would starve other tasks.
        let markdown = tokio::task::spawn_blocking(move || {
            // WHY temp file: edgeparse-core v0.2.3 accepts Path, not bytes.
            // We write to a named temp file, parse, then the file is deleted when
            // NamedTempFile is dropped. No persistent disk usage.
            let mut tmp = NamedTempFile::new()
                .map_err(|e| PdfConversionError::Io(e.to_string()))?;
            tmp.write_all(&bytes)
                .map_err(|e| PdfConversionError::Io(e.to_string()))?;

            let ep_config = ProcessingConfig {
                table_method: table_method.as_str().into(),
                ..ProcessingConfig::default()
            };

            let doc = ep_convert(tmp.path(), &ep_config)
                .map_err(|e| PdfConversionError::Backend(e.to_string()))?;

            let md = output::markdown::to_markdown(&doc)
                .map_err(|e| PdfConversionError::Backend(e.to_string()))?;

            info!(
                pages = doc.number_of_pages,
                elements = doc.kids.len(),
                markdown_bytes = md.len(),
                "EdgeParse conversion complete"
            );

            Ok::<String, PdfConversionError>(md)
        })
        .await
        .map_err(|e| PdfConversionError::Internal(e.to_string()))??;

        Ok(markdown)
    }

    fn backend_name(&self) -> &'static str { "edgeparse" }
}
```

**`src/error.rs`**:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PdfConversionError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("Backend error: {0}")]
    Backend(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Empty output: {0}")]
    EmptyOutput(String),
}
```

---

### Phase 2 — Storage Layer Changes

**File**: `crates/edgequake-storage/src/pdf_storage.rs`

#### 2.1 Extend `ExtractionMethod`

```rust
pub enum ExtractionMethod {
    Text,
    Vision,
    Hybrid,
    EdgeParse,   // ← NEW: CPU-only edgeparse-core extraction
}

impl ExtractionMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text      => "text",
            Self::Vision    => "vision",
            Self::Hybrid    => "hybrid",
            Self::EdgeParse => "edgeparse",
        }
    }
}

impl std::str::FromStr for ExtractionMethod {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "text"       => Ok(Self::Text),
            "vision"     => Ok(Self::Vision),
            "hybrid"     => Ok(Self::Hybrid),
            "edgeparse"  => Ok(Self::EdgeParse),
            _ => Err(StorageError::InvalidData(format!(
                "Invalid extraction method: {}", s
            ))),
        }
    }
}
```

#### 2.2 DB Migration (if CHECK constraint exists)

File: `edgequake/migrations/XXXX_add_edgeparse_extraction_method.sql`

```sql
-- Migration: allow 'edgeparse' as extraction_method value
-- WHY: TEXT column with no CHECK constraint — this migration is a no-op for safety.
-- If a CHECK constraint exists, add 'edgeparse' to it.
-- The migration is idempotent.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.table_constraints
        WHERE constraint_name = 'pdf_records_extraction_method_check'
    ) THEN
        ALTER TABLE pdf_records
            DROP CONSTRAINT pdf_records_extraction_method_check;
        ALTER TABLE pdf_records
            ADD CONSTRAINT pdf_records_extraction_method_check
            CHECK (extraction_method IN ('text','vision','hybrid','edgeparse'));
    END IF;
END $$;
```

---

### Phase 3 — Workspace Type Extension

**File**: `crates/edgequake-core/src/types/multitenancy/workspace.rs`

#### 3.1 Add field

```rust
use edgequake_pdf::backend::PdfParserBackend;

pub struct Workspace {
    // --- existing fields ---
    pub vision_llm_provider: Option<String>,
    pub vision_llm_model: Option<String>,

    // NEW ↓
    /// PDF parser backend for this workspace.
    ///
    /// None → fall back to EDGEQUAKE_PDF_PARSER_BACKEND env var → "vision".
    ///
    /// Stored in `metadata` JSON under key "pdf_parser_backend".
    #[serde(skip)]  // resolved from metadata at load time
    pub pdf_parser_backend: Option<PdfParserBackend>,
}
```

#### 3.2 Backend resolution helper

```rust
impl Workspace {
    /// Resolve PDF parser backend with full fallback chain.
    ///
    /// Priority: workspace field → ENV → default (Vision).
    pub fn resolved_pdf_parser_backend(&self) -> PdfParserBackend {
        self.pdf_parser_backend
            .or_else(PdfParserBackend::from_env)
            .unwrap_or_default() // Vision
    }
}
```

#### 3.3 Workspace service: load/save `pdf_parser_backend`

In `workspace_service_impl.rs`, when serializing workspace metadata:

```rust
if let Some(backend) = workspace.pdf_parser_backend {
    map["pdf_parser_backend"] = serde_json::json!(backend.as_str());
}
```

When deserializing (loading from KV):

```rust
if let Some(v) = map.get("pdf_parser_backend").and_then(|v| v.as_str()) {
    workspace.pdf_parser_backend = PdfParserBackend::from_env_str(v);
}
```

---

### Phase 4 — Task Data Extension

**File**: `crates/edgequake-tasks/src/types/data.rs`

```rust
use edgequake_pdf::backend::PdfParserBackend;

pub struct PdfProcessingData {
    // --- existing fields ---
    pub pdf_id: Uuid,
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub enable_vision: bool,
    pub vision_provider: String,
    pub vision_model: Option<String>,
    pub existing_document_id: Option<String>,

    // NEW ↓
    /// Which PDF parser backend to use.
    /// Serialized as "vision" | "edgeparse".
    #[serde(default)]
    pub pdf_parser_backend: PdfParserBackend,
}
```

**WHY `#[serde(default)]`**: existing tasks in the DB have no `pdf_parser_backend` field.
The `Default` impl returns `Vision`, so old tasks seamlessly use the Vision path.

---

### Phase 5 — API Layer Changes

#### 5.1 Upload Options

**File**: `crates/edgequake-api/src/handlers/pdf_upload/types.rs`

```rust
pub struct PdfUploadOptions {
    // --- existing fields ---
    pub enable_vision: bool,
    pub vision_provider: Option<String>,
    pub vision_model: Option<String>,
    pub title: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub track_id: Option<String>,
    pub force_reindex: bool,

    // NEW ↓
    /// Explicit backend override from upload form.
    /// None = use workspace/ENV/default resolution.
    pub pdf_parser_backend: Option<PdfParserBackend>,
}
```

Backend resolution:

```rust
impl PdfUploadOptions {
    pub fn resolved_backend(&self, workspace: &Workspace) -> PdfParserBackend {
        self.pdf_parser_backend          // 1. per-upload override
            .or_else(|| workspace.pdf_parser_backend) // 2. workspace setting
            .or_else(PdfParserBackend::from_env)       // 3. ENV
            .unwrap_or_default()                       // 4. Vision
    }
}
```

#### 5.2 Upload Handler

**File**: `crates/edgequake-api/src/handlers/pdf_upload/upload.rs`

Parse `pdf_parser_backend` from multipart form field:

```rust
"pdf_parser_backend" => {
    let val = field.text().await?;
    options.pdf_parser_backend = PdfParserBackend::from_env_str(&val);
}
```

#### 5.3 Task Construction Helper

**File**: `crates/edgequake-api/src/handlers/pdf_upload/helpers.rs`

```rust
let backend = options.resolved_backend(&workspace);

let task_data = PdfProcessingData {
    // existing fields ...
    pdf_parser_backend: backend,
    // If EdgeParse: vision fields are still populated for potential future use,
    // but the processor will ignore them.
};
```

#### 5.4 PDF Processor

**File**: `crates/edgequake-api/src/processor/pdf_processing.rs`

Replace the large `if data.enable_vision { ... }` block with:

```rust
use edgequake_pdf::{
    backend::{create_pdf_converter, PdfParserBackend},
    backend::trait_def::PdfConversionConfig,
};

// Step: resolve backend
let backend = data.pdf_parser_backend;

// Step: build converter (only creates LLM provider when needed)
let converter: Arc<dyn edgequake_pdf::backend::trait_def::PdfConverter> = match backend {
    PdfParserBackend::Vision => {
        let provider = create_safe_llm_provider(&data.vision_provider, &model)?;
        edgequake_pdf::backend::create_pdf_converter(backend, Some(Arc::new(provider)))
    }
    PdfParserBackend::EdgeParse => {
        edgequake_pdf::backend::create_pdf_converter(backend, None)
    }
};

// Step: convert
let conv_config = PdfConversionConfig {
    page_count_hint: pdf.page_count.map(|n| n as usize),
    table_method: None, // "auto" default
};

let markdown = converter
    .convert(&pdf.pdf_data, &conv_config)
    .await
    .map_err(|e| TaskError::Processing(format!("PDF conversion failed: {e}")))?;

// Step: low-content warning for EdgeParse scanned PDFs
let page_count = pdf.page_count.unwrap_or(1) as usize;
let avg_chars_per_page = markdown.len() / page_count.max(1);
let extraction_method = backend.extraction_method_str();
if backend == PdfParserBackend::EdgeParse && avg_chars_per_page < 50 {
    warn!(
        pdf_id = %data.pdf_id,
        avg_chars_per_page,
        "Low text content from EdgeParse — PDF may be scanned/image-only. \
         Consider switching to Vision backend."
    );
    // Store warning in extraction_errors
}
```

#### 5.5 Workspace Settings API

**File**: `crates/edgequake-api/src/handlers/workspaces/` (update route)

Add `pdf_parser_backend` to workspace update request and response types:

```rust
// In workspaces_types/mod.rs (or similar)
pub struct UpdateWorkspaceRequest {
    // existing fields ...
    pub pdf_parser_backend: Option<String>, // "vision" | "edgeparse" | "none"
}
```

In workspace service update handler:

```rust
if let Some(ref backend_str) = request.pdf_parser_backend {
    if backend_str == "none" || backend_str.is_empty() {
        workspace.pdf_parser_backend = None;
    } else {
        workspace.pdf_parser_backend =
            PdfParserBackend::from_env_str(backend_str);
    }
}
```

---

### Phase 6 — Cargo Workspace

**File**: `edgequake/Cargo.toml` (workspace)

Add:
```toml
[workspace.dependencies]
edgeparse-core = "0.2"
```

Add `edgequake-pdf` to workspace members if not already present:
```toml
members = [
    # ...existing...
    "crates/edgequake-pdf",
]
```

**File**: `crates/edgequake-api/Cargo.toml`

Replace direct `edgequake-pdf2md` dependency with:
```toml
edgequake-pdf    = { path = "../edgequake-pdf" }   # abstraction layer
edgequake-pdf2md = "=0.8.0"                         # kept in edgequake-pdf Cargo.toml
```

---

### Phase 7 — Frontend (NextJS / React)

**Upload Dialog** — Add backend selector:

```
  ┌─────────────────────────────────────────────────────────┐
  │  Upload PDF Document                             [×]    │
  ├─────────────────────────────────────────────────────────┤
  │                                                         │
  │  File:  [ report.pdf                         ←browse ]  │
  │                                                         │
  │  Parser Backend:  ┌─────────────────────────────────┐   │
  │                   │  ○ Vision (LLM)     ← default   │   │
  │                   │  ○ EdgeParse (fast)              │   │
  │                   └─────────────────────────────────┘   │
  │                                                         │
  │  [Vision selected]                                      │
  │  Provider: [Ollama ▼]   Model: [gemma3:latest     ]    │
  │                                                         │
  │                        [ Cancel ]  [ Upload & Parse ]   │
  └─────────────────────────────────────────────────────────┘
```

**Workspace Settings Page** — Backend section:

```
  ┌─────────────────────────────────────────────────────────┐
  │  PDF Parsing                                            │
  ├─────────────────────────────────────────────────────────┤
  │  Default Parser Backend                                 │
  │  ┌───────────────────────────────────────────────────┐  │
  │  │  ● Vision LLM  (default)                         │  │
  │  │  ○ EdgeParse   (fast, CPU-only, no API key)       │  │
  │  └───────────────────────────────────────────────────┘  │
  │                                                         │
  │  ℹ Vision: Best for scanned PDFs and image-heavy docs.  │
  │    EdgeParse: 83× faster, deterministic, no token cost. │
  │                                                         │
  │  Vision Provider: [OpenAI ▼]                           │
  │  Vision Model:    [gpt-4.1-nano          ]             │
  │                                                         │
  │                                    [ Save Settings ]   │
  └─────────────────────────────────────────────────────────┘
```

**Document Detail — Extraction Badge**:

```
  ┌────────────────────────────────────────────┐
  │  Extraction Method:  [⚡ EdgeParse]         │
  │                  or  [👁 Vision LLM]        │
  └────────────────────────────────────────────┘
```

Show `ExtractionMethod` value returned from API in the document detail view.
Surface low-content warning inline:

```
  ┌─────────────────────────────────────────────────────────┐
  │  ⚠ Low text content detected (avg 12 chars/page).      │
  │  This PDF may be image-only. Consider switching to     │
  │  Vision extraction for better results.    [Switch →]   │
  └─────────────────────────────────────────────────────────┘
```

---

## 5. Edge Cases & Mitigations

| #     | Edge Case                                                                             | Risk                                                   | Mitigation                                                                                                                                                                                         |
| ----- | ------------------------------------------------------------------------------------- | ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| EC-01 | Scanned/image-only PDF with EdgeParse                                                 | Near-empty markdown, poor knowledge graph              | Detect avg_chars/page < 50; store warning; surface UI alert to switch to Vision                                                                                                                    |
| EC-02 | Password-protected PDF                                                                | edgeparse-core panics or returns error                 | Catch `PdfConversionError::Backend`; mark PDF as `Failed`; surface "Password-protected PDF" message                                                                                                |
| EC-03 | Corrupt/truncated PDF bytes                                                           | Parse failure at any stage                             | `map_err(PdfConversionError::Backend)`; task retries (max 3, existing logic)                                                                                                                       |
| EC-04 | EdgeParse returns empty string                                                        | Empty markdown → entity extraction produces nothing    | Check `markdown.is_empty()` before storing; return `PdfConversionError::EmptyOutput`                                                                                                               |
| EC-05 | Very large PDF (1000+ pages) with EdgeParse                                           | Memory spike from Rayon threads                        | EdgeParse is CPU-bound; `spawn_blocking` isolates it; no page concurrency limit needed (engine handles internally)                                                                                 |
| EC-06 | Old tasks in queue (no `pdf_parser_backend` field)                                    | Deserialization failure                                | `#[serde(default)]` on field → defaults to `Vision` seamlessly                                                                                                                                     |
| EC-07 | Worker retry with EdgeParse after crash                                               | Duplicate document creation                            | Existing `existing_document_id` propagation handles this — no EdgeParse-specific change                                                                                                            |
| EC-08 | Vision credentials missing when Vision selected                                       | `create_safe_llm_provider` error                       | Existing error handling; surface "Vision provider not configured"                                                                                                                                  |
| EC-09 | `edgeparse-core` temp file creation fails                                             | Disk full / permissions                                | `NamedTempFile::new()` returns `Err`; mapped to `PdfConversionError::Io`                                                                                                                           |
| EC-10 | Cancellation during EdgeParse conversion                                              | `spawn_blocking` cannot be cancelled mid-execution     | EdgeParse is fast (< 1s typical); accept that cancellation check fires before/after; do not add tokio cancel inside sync block                                                                     |
| EC-11 | Workspace backend set to EdgeParse, but PDF is scanned → user expects Vision fallback | Silent poor quality                                    | Explicit policy (ADR-03-005): no auto-fallback. Warning surface + manual switch. Document this in UI tooltip                                                                                       |
| EC-12 | `edgeparse-core` returns non-UTF8 bytes                                               | `to_markdown` produces valid UTF-8 (it's a Rust crate) | No issue; Rust strings are UTF-8 by construction                                                                                                                                                   |
| EC-13 | Concurrent `spawn_blocking` calls saturate blocking pool                              | Worker starvation                                      | Tokio's blocking pool auto-scales; Rayon within each task is CPU-bound and parallel. Max concurrency controlled by `EDGEQUAKE_PDF_CONCURRENCY` env (Vision only; EdgeParse has its own Rayon pool) |
| EC-14 | `PdfParserBackend` unknown value in ENV or workspace metadata                         | Invalid backend silently ignored                       | `from_env_str` returns `None`; falls back to next in resolution chain; logs `warn!("Unknown pdf_parser_backend: {val}, using Vision")`                                                             |
| EC-15 | Frontend sends `pdf_parser_backend=""` (empty string)                                 | Treated as unset                                       | `filter(                                                                                                                                                                                           | s | !s.is_empty())` before `from_env_str` — existing pattern throughout codebase |
| EC-16 | DB CHECK constraint on `extraction_method` rejects "edgeparse"                        | Insert failures                                        | Migration (Phase 2.2) updates constraint before first deploy                                                                                                                                       |

---

## 6. Testing Plan

### Unit Tests (in `edgequake-pdf`)

```
tests/
├── edgeparse_converter_test.rs
│   ├── test_convert_digital_native_pdf()   ← fixture: small text PDF
│   ├── test_empty_output_detection()       ← fixture: blank PDF
│   ├── test_scanned_pdf_warning()          ← avg chars < 50 detection
│   └── test_backend_name()
├── backend_resolution_test.rs
│   ├── test_env_override()
│   ├── test_workspace_override()
│   ├── test_per_upload_override()
│   └── test_default_is_vision()
└── extraction_method_test.rs
    └── test_edgeparse_roundtrip()          ← as_str / from_str
```

### Integration Tests (in `edgequake-api`)

```
tests/
└── pdf_processing_edgeparse.rs
    ├── test_full_pipeline_edgeparse_backend()
    └── test_task_deserialization_backward_compat()  ← old task, no backend field
```

### Test Fixtures

Use `edgequake/crates/edgequake-pdf/test-data/`:
- `sample_digital.pdf` — text-based PDF (expected: > 200 chars/page)
- `sample_scanned_sim.pdf` — near-blank PDF (expected: < 50 chars/page, warning)

---

## 7. Environment Variables Reference

| Variable                        | Values                | Default                | Description                 |
| ------------------------------- | --------------------- | ---------------------- | --------------------------- |
| `EDGEQUAKE_PDF_PARSER_BACKEND`  | `vision`, `edgeparse` | `vision`               | Server-wide default backend |
| `EDGEQUAKE_VISION_MODEL`        | any model name        | (provider default)     | Override vision model       |
| `EDGEQUAKE_PDF_CONCURRENCY`     | integer               | adaptive by page count | Vision-only concurrency     |
| `EDGEQUAKE_PDF_DPI`             | integer               | adaptive               | Vision-only DPI             |
| `EDGEQUAKE_VISION_TIMEOUT_SECS` | integer               | adaptive               | Vision-only timeout         |

---

## 8. Rollout & Migration

### Backward Compatibility Checklist

- [ ] Existing workspaces: `pdf_parser_backend` absent from metadata → `Vision` (default)
- [ ] Existing tasks in queue: no `pdf_parser_backend` field → `Vision` (serde default)  
- [ ] Existing DB rows: `extraction_method` column accepts "edgeparse" after migration
- [ ] Cargo.lock: `edgeparse-core` added without breaking existing deps
- [ ] Docker image: `edgeparse-core` is statically linked (no system deps) — add nothing to Dockerfile

### Feature Flag

No feature flag needed — `edgeparse-core` is always compiled in. The backend
selection is runtime-only. This avoids conditional compilation complexity and
ensures the binary always supports both backends.

### Deployment Steps

```
1. Run DB migration (add_edgeparse_extraction_method.sql)
2. Deploy new binary
3. Optionally set: EDGEQUAKE_PDF_PARSER_BACKEND=edgeparse
4. Test with a known-good digital PDF
5. Verify ExtractionMethod stored as "edgeparse" in DB
6. Roll back: set EDGEQUAKE_PDF_PARSER_BACKEND=vision
```

---

## 9. Non-Goals (v0.10.0)

- Auto-fallback from EdgeParse → Vision for scanned PDFs
- Hybrid mode (EdgeParse text + Vision for image regions)  
- EdgeParse WASM in the browser
- Any changes to the entity extraction pipeline
- OCR integration
- Streaming/progress callbacks for EdgeParse (it completes in < 1s)

---

## 10. Open Questions

| #     | Question                                                           | Owner | Resolution                                                         |
| ----- | ------------------------------------------------------------------ | ----- | ------------------------------------------------------------------ |
| OQ-01 | Does `edgeparse-core` 0.2.x API break between minor versions?      | Dev   | Pin to `"0.2"` (compatible range); review changelog before upgrade |
| OQ-02 | Should the EdgeParse `table_method` be configurable per workspace? | PM    | Out of scope v0.10.0; `"auto"` is sensible default                 |
| OQ-03 | Should we expose `pdf_parser_backend` in the OpenAPI spec?         | Dev   | Yes — add `ToSchema` derive and document in utoipa                 |

---

## 11. Files Changed Summary

| File                                                            | Change Type | Phase |
| --------------------------------------------------------------- | ----------- | ----- |
| `crates/edgequake-pdf/Cargo.toml`                               | CREATE      | 1     |
| `crates/edgequake-pdf/src/lib.rs`                               | CREATE      | 1     |
| `crates/edgequake-pdf/src/backend/mod.rs`                       | CREATE      | 1     |
| `crates/edgequake-pdf/src/backend/trait.rs`                     | CREATE      | 1     |
| `crates/edgequake-pdf/src/backend/vision.rs`                    | CREATE      | 1     |
| `crates/edgequake-pdf/src/backend/edgeparse.rs`                 | CREATE      | 1     |
| `crates/edgequake-pdf/src/error.rs`                             | CREATE      | 1     |
| `crates/edgequake-storage/src/pdf_storage.rs`                   | MODIFY      | 2     |
| `edgequake/migrations/XXXX_add_edgeparse_extraction_method.sql` | CREATE      | 2     |
| `crates/edgequake-core/src/types/multitenancy/workspace.rs`     | MODIFY      | 3     |
| `crates/edgequake-core/src/workspace_service_impl.rs`           | MODIFY      | 3     |
| `crates/edgequake-tasks/src/types/data.rs`                      | MODIFY      | 4     |
| `crates/edgequake-api/src/handlers/pdf_upload/types.rs`         | MODIFY      | 5     |
| `crates/edgequake-api/src/handlers/pdf_upload/helpers.rs`       | MODIFY      | 5     |
| `crates/edgequake-api/src/handlers/pdf_upload/upload.rs`        | MODIFY      | 5     |
| `crates/edgequake-api/src/processor/pdf_processing.rs`          | MODIFY      | 5     |
| `crates/edgequake-api/src/handlers/workspaces/`                 | MODIFY      | 5     |
| `crates/edgequake-api/Cargo.toml`                               | MODIFY      | 6     |
| `edgequake/Cargo.toml`                                          | MODIFY      | 6     |
| `edgequake_webui/src/` (upload dialog, workspace settings)      | MODIFY      | 7     |

Total: ~20 files, 2 new crate files (edgequake-pdf), 1 DB migration.

---

## 12. Acceptance Criteria

- [ ] AC-01: Upload a digital PDF → select EdgeParse → markdown produced in < 5 seconds
- [ ] AC-02: Upload a scanned PDF → select EdgeParse → warning shown in UI; markdown stored (possibly thin)
- [ ] AC-03: Set `EDGEQUAKE_PDF_PARSER_BACKEND=edgeparse` → all uploads use EdgeParse by default
- [ ] AC-04: Workspace setting overrides ENV; per-upload override overrides workspace
- [ ] AC-05: Old task data (no `pdf_parser_backend` field) processed correctly → Vision path
- [ ] AC-06: `ExtractionMethod::EdgeParse` stored and retrieved correctly from DB
- [ ] AC-07: Vision path unchanged — existing tests pass without modification
- [ ] AC-08: `cargo test --workspace` passes
- [ ] AC-09: `cargo clippy --all-targets` passes with zero warnings
- [ ] AC-10: No Vision LLM API call made when EdgeParse backend is selected

---

*Mission end.*
