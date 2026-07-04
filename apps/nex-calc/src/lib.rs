// nex-calc — FIH-based calculator.
//
// Computation is state space traversal. Every operation is recorded
// as an immutable Fact in the FIH (Fact-Intent-Hint) space.
//
//   F (Fact)  = number at a coordinate — both storage and operand
//   I (Intent) = operator with direction — the traversal itself
//   H (Hint)   = constraint or transform — dynamic boundaries
//
// The algebraic structure F × I × H → F' is the foundation.

pub mod engine;
pub mod hint;
pub mod ops;

pub use engine::{CalcEngine, CalcError, ResolvedIntent};
pub use hint::Constraint;
pub use ops::OpType;
