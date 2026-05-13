// nexus-cypher — Cypher → petgraph translation
//
// Dual-path architecture:
//   Plan::External → cyrs_plan::ReadOp (production, default)
//   Plan::Internal → PlanIR (fallback, lightweight)
//
// Parser delegates to cyrs-syntax + cyrs-hir pipeline.
// Executor handles both plan variants through a unified interface.

mod plan;
mod parser;
mod translate;

pub use plan::*;
pub use parser::parse_query;
pub use translate::*;
