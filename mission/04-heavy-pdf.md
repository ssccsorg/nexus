# Mission 04 — Reliable Large-PDF Ingestion with Local Models

**Linked issue**: https://github.com/raphaelmansuy/edgequake/issues/90  
**Branch**: `feat/edgequake-v0.10.0`  
**Status**: 🔴 Active — Implementation in progress

---

## Problem Statement

A 120-page document uploaded to EdgeQuake with a local Ollama model (GPU-accelerated) hits the
600 s timeout **three times** and then trips the circuit-breaker, leaving the document permanently
stuck in `Failed` state. Small documents (≤ 5 pages) work fine.

---

## First-Principle Analysis

### The Physics of Local Vision Inference

| Variable                                          | Value                              |
| ------------------------------------------------- | ---------------------------------- |
| Pages                                             | 120                                |
| Local throughput (Ollama / single GPU, realistic) | 15–30 s / page                     |
| Required end-to-end conversion time               | 1 800–3 600 s (30–60 min)          |
| Current hard timeout per-page LLM call            | 600 s (`MAXIMUM_TIMEOUT_SECS`)     |
| Current outer vision-conversion timeout           | `max(60 + pages × 5, 600)` = 660 s |

**Conclusion**: 660 s outer timeout vs. 30–60 min requirement = guaranteed failure.

### Root Causes (Code Is Law)

#### RC-1 — Per-page LLM call capped at 600 s (`safety_limits.rs`)

```rust
// safety_limits.rs
pub const MAXIMUM_TIMEOUT_SECS: u64 = 600; // ← hard cap for EVERY provider

pub fn create_safe_llm_provider(...) -> Result<Arc<dyn LLMProvider>> {
    let config = SafetyLimitsConfig::from_env(); // clamps to MAXIMUM_TIMEOUT_SECS
    Ok(Arc::new(SafetyLimitedProviderWrapper::new(inner, config)))
}
```

`SafetyLimitsConfig::from_env()` clamps `EDGEQUAKE_LLM_TIMEOUT_SECS` to
`[MINIMUM_TIMEOUT_SECS, MAXIMUM_TIMEOUT_SECS]` = `[10, 600]`. Even setting
`EDGEQUAKE_LLM_TIMEOUT_SECS=3600` has no effect. Individual slow pages fail silently.

#### RC-2 — Outer vision-conversion timeout formula is wrong (`pdf_processing.rs`)

```rust
// pdf_processing.rs
let adaptive = 60 + (page_count as u64 * 5); // 5 s/page assumes cloud API speed
let vision_timeout_secs = adaptive.max(600);   // 660 s for 120 pages
```

The formula assumes ~5 s/page (OpenAI/Gemini cloud speed). Local Ollama models need
15–30 s/page minimum. For 120 pages: needed = 3 600 s, actual = 660 s → **5× too short**.

#### RC-3 — Concurrency default is too high for local VRAM (`pdf_processing.rs`)

```rust
let concurrency = match page_count {
    50..=199 => 8, // ← 8 concurrent LLM calls on a single GPU = VRAM contention
    _ => ...
};
```

With Ollama pinning one GPU, 8 concurrent vision requests compete for the same VRAM pool.
Each request takes longer, compounding the timeout problem.

#### RC-4 — Circuit-breaker has no visibility into cause

The task worker's circuit-breaker fires after 3 consecutive `TaskError::Timeout`. It doesn't
distinguish "PDF vision timeout (expected for large docs)" from "LLM server completely down".
This conflation means a legitimately long document permanently gets blocked.

---

## Brainstorm — Solutions Inventory

| ID  | Solution                                                            | Impact | Effort | Risk   |
| --- | ------------------------------------------------------------------- | ------ | ------ | ------ |
| S1  | Provider-aware timeout: separate cap for local providers            | High   | Low    | Low    |
| S2  | Fix outer timeout formula with realistic per-page estimate          | High   | Low    | Low    |
| S3  | Reduce default concurrency for Ollama/local providers               | High   | Low    | Low    |
| S4  | Add `EDGEQUAKE_PDF_SECS_PER_PAGE` env var                           | Medium | Low    | None   |
| S5  | Use OCR-specialized model (glm-ocr 0.9B) for faster local inference | High   | Medium | Low    |
| S6  | Page-batch streaming (convert 10 pages → save → continue)           | High   | High   | Medium |
| S7  | Circuit-breaker exemption for PDF vision tasks                      | Medium | Medium | Medium |
| S8  | DPI auto-downscale on timeout retry                                 | Low    | Medium | Low    |

**Selected for this iteration**: S1, S2, S3, S4 (safe, non-breaking, immediately effective).

---

## ADR — Architecture Decision Record

### ADR-04-001: Provider-Aware Vision Timeout

**Status**: Accepted  
**Date**: 2026-04-11

**Context**:  
The `SafetyLimitsConfig` applies a single `MAXIMUM_TIMEOUT_SECS = 600` ceiling to all providers.
This is correct for cloud LLM APIs (OpenAI, Anthropic, Gemini) where 600 s is already generous.
It is catastrophically wrong for local providers (Ollama, LM Studio) where a single page can
take 30 s and 120 pages requires 60+ minutes.

**Decision**:  
Introduce `create_safe_vision_provider(provider_name, model)` — a new factory function that
uses a **per-provider timeout** derived from an env var `EDGEQUAKE_VISION_PAGE_TIMEOUT_SECS`
(default: 300 s for cloud, 600 s for local), **without a hard ceiling** for local providers.
The existing `create_safe_llm_provider` remains unchanged (used for entity extraction, chat).

**Consequences**:
- Cloud providers: unchanged behaviour
- Local providers: individual page calls can run up to `EDGEQUAKE_VISION_PAGE_TIMEOUT_SECS`
  (default 600 s per page), uncapped
- No risk to entity extraction pipeline (uses different code path)

---

### ADR-04-002: Realistic Outer Vision-Conversion Timeout

**Status**: Accepted  
**Date**: 2026-04-11

**Context**:  
The outer `tokio::time::timeout(vision_timeout_secs, converter.convert(...))` formula
`max(60 + pages × 5, 600)` uses 5 s/page which matches cloud API speeds, not local inference.

**Decision**:  
Use a provider-aware formula:

```
secs_per_page = EDGEQUAKE_PDF_SECS_PER_PAGE env var
             OR (30 for local providers, 8 for cloud providers)

vision_timeout_secs = 120 + (pages × secs_per_page)
                    clamped to [60, u64::MAX] for local, [60, 7200] for cloud
```

**Consequences**:
- 120-page doc with Ollama: `120 + (120 × 30)` = 3 720 s ≈ 62 min — matches hardware reality
- 120-page doc with OpenAI: `120 + (120 × 8)` = 1 080 s ≈ 18 min — safe ceiling maintained
- Environment-variable override allows expert users to tune for their GPU

---

### ADR-04-003: Provider-Aware Concurrency Defaults

**Status**: Accepted  
**Date**: 2026-04-11

**Context**:  
Default concurrency of 8 for 50–199 page documents assumes independent parallel resources
(cloud API rate-limited, not memory-limited). Local GPU inference is memory-bound and
sequential: concurrency > 2 causes VRAM thrashing and *increases* total time.

**Decision**:  
Detect local providers (Ollama, LM Studio) and apply reduced concurrency defaults:

```
Local provider (ollama, lmstudio): concurrency = min(2, EDGEQUAKE_PDF_CONCURRENCY)
Cloud provider (openai, etc.):     existing formula unchanged
```

**Consequences**:
- Local: 2 concurrent pages maximally utilises single GPU without thrashing
- Cloud: unchanged
- `EDGEQUAKE_PDF_CONCURRENCY` still overrides for expert users

---

### ADR-04-004: OCR-Optimised Model Recommendation

**Status**: Accepted (documentation only, no code change)  
**Date**: 2026-04-11

**Context**:  
`gemma4:latest` is a general-purpose multimodal model. For PDF page OCR it is larger than needed.
Two purpose-built alternatives exist on Ollama:

| Model                               | Params | Size   | Context | Best for                   |
| ----------------------------------- | ------ | ------ | ------- | -------------------------- |
| `glm-ocr:latest`                    | 0.9B   | 2.2 GB | 128K    | Tables, code, seals, forms |
| `fredrezones55/chandra-ocr-2:patch` | 4B     | 5.8 GB | 256K    | Multilingual (90+ langs)   |
| `gemma4:latest`                     | 4B+    | 8 GB+  | long    | General multimodal         |

**Decision**:  
- Recommend `glm-ocr:latest` as the default local OCR vision model for most use cases
- Recommend `chandra-ocr-2:patch` for multilingual documents
- Document in README and UI tooltips
- No code change required; user selects model via `EDGEQUAKE_VISION_MODEL` or UI

---

## Edge Cases & Mitigations

| Edge Case                                           | Mitigation                                                                              |
| --------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Ollama OOM mid-conversion                           | FileCheckpointStore resumes from last completed page on retry                           |
| Single corrupt page causes full failure             | Process-with-resilience skips failed pages, logs which pages failed                     |
| User sets `EDGEQUAKE_PDF_SECS_PER_PAGE=1` (too low) | Floor of 5 s/page enforced in code                                                      |
| Provider name typo ("olama") classified as cloud    | Local provider detection is additive: unknown providers treated as cloud (conservative) |
| 1 000-page document: timeout = 30 000 s             | Cap at 86 400 s (24 h) with warning log                                                 |
| VRAM fragmentation after many pages                 | Concurrency capped at 2 for local; sequential processing is the worst-case fallback     |

---

## Implementation Checklist

- [ ] `safety_limits.rs` — add `create_safe_vision_provider()` with provider-aware timeout
- [ ] `pdf_processing.rs` — fix outer timeout formula (ADR-04-002)
- [ ] `pdf_processing.rs` — fix concurrency defaults for local providers (ADR-04-003)
- [ ] `pdf_processing.rs` — read `EDGEQUAKE_PDF_SECS_PER_PAGE` env var (ADR-04-001)
- [ ] `AGENTS.md` / README — document new env vars
- [ ] Build + test: `cargo build -p edgequake-api` passes

---

## New Environment Variables

| Variable                             | Default                                 | Description                                     |
| ------------------------------------ | --------------------------------------- | ----------------------------------------------- |
| `EDGEQUAKE_PDF_SECS_PER_PAGE`        | `30` (local) / `8` (cloud)              | Estimated seconds per page for outer timeout    |
| `EDGEQUAKE_VISION_PAGE_TIMEOUT_SECS` | `600` (local, uncapped) / `120` (cloud) | Per-page LLM call timeout for vision extraction |
| `EDGEQUAKE_PDF_CONCURRENCY`          | `2` (local) / formula (cloud)           | Parallel page extraction workers                |

