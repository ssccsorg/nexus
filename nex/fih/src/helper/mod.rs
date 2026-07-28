// ── Helper modules ──────────────────────────────────────────────────────
//
// Protocol extension helpers for Content and other core types.
// These are NOT part of nexus-model — they are consumer-side utilities
// that depend on specific formats (JSON, etc.).

pub mod content;

pub use content::ContentJsonExt;
