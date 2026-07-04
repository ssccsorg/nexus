// Hint definitions for nex-calc.
//
// Hints are dynamic constraints or transforms that modify computation
// without changing the Facts or Intents themselves. They represent the
// "H" in FIH: boundary conditions on state space traversal.

use std::fmt;

/// Constraints that gate or transform computation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    /// Result must be greater than N.
    GreaterThan(i64),
    /// Result must be less than N.
    LessThan(i64),
    /// Result must equal N.
    Equals(i64),
    /// Result must not equal N.
    NotEquals(i64),
    /// Result must be even.
    IsEven,
    /// Result must be positive (> 0).
    IsPositive,
    /// Transform: double both operands before applying the operator.
    MapDouble,
}

impl Constraint {
    /// Check whether a result satisfies this constraint.
    ///
    /// Returns `true` if the constraint is satisfied or if this
    /// constraint is a transform (transforms always pass the check,
    /// they modify operands instead).
    pub fn check(&self, result: i64) -> bool {
        match self {
            Constraint::GreaterThan(n) => result > *n,
            Constraint::LessThan(n) => result < *n,
            Constraint::Equals(n) => result == *n,
            Constraint::NotEquals(n) => result != *n,
            Constraint::IsEven => result % 2 == 0,
            Constraint::IsPositive => result > 0,
            Constraint::MapDouble => true,
        }
    }

    /// Apply operand-level transforms. Returns the transformed (lhs, rhs).
    pub fn transform_operands(&self, lhs: i64, rhs: i64) -> (i64, i64) {
        match self {
            Constraint::MapDouble => (lhs * 2, rhs * 2),
            _ => (lhs, rhs),
        }
    }

    /// Human-readable description of the constraint.
    pub fn description(&self) -> String {
        match self {
            Constraint::GreaterThan(n) => format!("result > {n}"),
            Constraint::LessThan(n) => format!("result < {n}"),
            Constraint::Equals(n) => format!("result = {n}"),
            Constraint::NotEquals(n) => format!("result != {n}"),
            Constraint::IsEven => "result is even".into(),
            Constraint::IsPositive => "result > 0".into(),
            Constraint::MapDouble => "double operands".into(),
        }
    }

    /// Parse from Display-formatted string.
    pub fn parse_str(s: &str) -> Option<Self> {
        if s.starts_with("result > ") {
            s.trim_start_matches("result > ")
                .parse()
                .ok()
                .map(Constraint::GreaterThan)
        } else if s.starts_with("result < ") {
            s.trim_start_matches("result < ")
                .parse()
                .ok()
                .map(Constraint::LessThan)
        } else if s.starts_with("result = ") {
            s.trim_start_matches("result = ")
                .parse()
                .ok()
                .map(Constraint::Equals)
        } else if s.starts_with("result != ") {
            s.trim_start_matches("result != ")
                .parse()
                .ok()
                .map(Constraint::NotEquals)
        } else if s == "result is even" {
            Some(Constraint::IsEven)
        } else if s == "result > 0" {
            Some(Constraint::IsPositive)
        } else if s == "double operands" {
            Some(Constraint::MapDouble)
        } else {
            None
        }
    }

    /// Parse from command arguments.
    pub fn parse(kind: &str, arg: Option<&str>) -> Option<Self> {
        match kind {
            "gt" => arg
                .and_then(|a| a.parse().ok())
                .map(Constraint::GreaterThan),
            "lt" => arg.and_then(|a| a.parse().ok()).map(Constraint::LessThan),
            "eq" => arg.and_then(|a| a.parse().ok()).map(Constraint::Equals),
            "ne" => arg.and_then(|a| a.parse().ok()).map(Constraint::NotEquals),
            "even" => Some(Constraint::IsEven),
            "pos" => Some(Constraint::IsPositive),
            "double" => Some(Constraint::MapDouble),
            _ => None,
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}
