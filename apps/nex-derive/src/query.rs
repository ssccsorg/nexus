// Conjunctive queries.
//
// A query is a conjunction of per-axis conditions. Each condition names the
// values it allows on one axis; the intersection of the conditions is the
// answer. The form is deliberately restricted to conjunction: negation,
// disjunction, and recursion each step outside the monotone fragment where the
// intersection is the whole evaluation.

use crate::axis::Axis;
use crate::layout::{AXES, AXIS_CARD};

/// All values on an axis are allowed. Requires `AXIS_CARD == 64` so the mask
/// fills a `u64` exactly.
pub const ALL: u64 = u64::MAX;

const _: () = assert!(
    AXIS_CARD == 64,
    "query::ALL assumes an axis of exactly 64 values"
);

/// The allowed value set for one axis, as a bitmask over value indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Condition {
    pub axis: Axis,
    pub allow: u64,
}

impl Condition {
    pub fn all(axis: Axis) -> Self {
        Self { axis, allow: ALL }
    }

    pub fn single(axis: Axis, value: u8) -> Self {
        debug_assert!((value as usize) < AXIS_CARD);
        Self {
            axis,
            allow: 1u64 << value,
        }
    }

    #[inline]
    pub fn allows(&self, value: u8) -> bool {
        self.allow & (1u64 << value) != 0
    }

    /// True when this condition excludes no value, so it adds no constraint.
    #[inline]
    pub fn is_unconstrained(&self) -> bool {
        self.allow == ALL
    }

    pub fn count(&self) -> u32 {
        self.allow.count_ones()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub conditions: Vec<Condition>,
}

impl Query {
    pub fn new(conditions: Vec<Condition>) -> Self {
        Self { conditions }
    }

    /// The naive predicate: every condition must allow the record's value.
    /// This is the reference the intersection is checked against.
    pub fn matches(&self, values: [u8; AXES]) -> bool {
        self.conditions
            .iter()
            .all(|condition| condition.allows(values[condition.axis.index()]))
    }

    /// How many conditions actually narrow the result.
    pub fn constrained_axes(&self) -> usize {
        self.conditions
            .iter()
            .filter(|condition| !condition.is_unconstrained())
            .count()
    }
}
