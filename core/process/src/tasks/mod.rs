// nexus-process — Task handlers implementing model detection traits.
//
// Each module implements one or more detection capability traits from
// nexus-model::detection. The scheduler composes them via Vec<Box<dyn DetectionCapable>>.
//
// Default implementations:
//   - GapDetector          → GapDetection
//   - ContradictionDetector → ContradictionDetection
//   - StateChangeDetector  → StateChangeDetection
//   - NewDocumentAnalyzer  → DetectionCapable (standalone)
//
// Custom detectors implement the appropriate trait and can be mixed in
// with the defaults — same pattern as swappable storage backends.

pub(crate) mod common;
pub mod contradiction_detector;
pub mod gap_detector;
pub mod new_document_analyzer;
pub mod state_change_detector;
