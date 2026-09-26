// nex-derive: a goal derives a fact.
//
// The FIH primitives read as a directed relation. A goal is a point in the
// product space of work, intent, and hint (the contract). Resolving a goal
// derives a fact, and the derivation is the recorded pair (goal, fact). The
// direction is the point: the relation is not symmetric, so the record carries
// goal to fact rather than a set of co-equal items.
//
// The intersection that resolves a goal is symmetric. That is fine, because
// the intersection is the inner step, not the identity: an intersection of
// conditions does not know which side is the premise. The direction lives in
// the record, and the record is what this app is named for.
//
// nex-calc executes the relation once, as a state transition on an Intent
// (submit, claim, conclude) that writes the resulting Fact. nex-derive keeps
// the derivation instead: the record of which goal yielded which fact, so an
// accumulated board can be read back. Neither app adds a rule engine; the
// resolution is a meet of posting bitsets.
//
// The fast fragment is stated exactly, because the boundary matters more than
// the win. The intersection is the whole resolution when the query is a
// conjunction (no negation, no disjunction), the derivations are materialized,
// the axes have fixed radix, and each goal holds one derivation. Recursion,
// aggregation, ranking, temporal and non-monotone rules leave that fragment and
// need machinery on top. nex-derive states the fragment and nothing more.

pub mod axis;
pub mod bits;
pub mod board;
pub mod layout;
pub mod query;

pub use axis::{Axis, NameError, Names, pack, unpack};
pub use bits::{BitIter, Bits};
pub use board::{BenchLine, Board, Derivation, DeriveOutcome};
pub use layout::{AXES, AXIS_BITS, AXIS_CARD, COORD_BITS, COORD_SPACE, WORDS};
pub use query::{ALL, Condition, Query};
