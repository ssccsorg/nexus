// ── FIH-specific extension: adds origin and creator metadata ──────────
//
// Wraps the pure semantic RecordLoad trait with FIH-specific accessors
// for origin and creator metadata. Consumers that need FIH-specific
// fields can bound on `FihRecordLoad` instead of bare `RecordLoad`.

use super::record::RecordLoad;

/// FIH-specific extension: adds origin and creator metadata.
///
/// Extends the pure semantic `RecordLoad` with FIH-specific accessors
/// for origin and creator strings. This is a trait-lego pattern:
/// consumers that need only generic record loading use `RecordLoad`,
/// while FIH-aware consumers use `FihRecordLoad`.
pub trait FihRecordLoad: RecordLoad {
    /// Load the origin string for a record.
    fn origin(&self, id: u32) -> Option<String>;

    /// Load the creator string for a record.
    fn creator(&self, id: u32) -> Option<String>;
}
