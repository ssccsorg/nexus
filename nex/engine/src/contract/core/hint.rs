// ── HintEngine: constraint evaluation for Intent resolution ────────────
//
// The HintEngine manages a registry of HintRules that gate whether an
// Intent may be concluded. Pattern from nex-calc's constrain → resolve
// pipeline: constraints are evaluated before a transition is permitted.
//
// Hint types mirror the nex-calc Constraint enum:
//   - Gt/Lt/Eq/Ne: numeric comparison
//   - Positive/Even: property checks
//   - FromSchema/ToSchema: schema constraints (stubbed in v1)
//   - Custom: arbitrary string constraint (stubbed in v1)

use std::sync::Mutex;

/// Constraint rule evaluated during Intent resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum HintRule {
    /// Value must be greater than N.
    Gt(i64),
    /// Value must be less than N.
    Lt(i64),
    /// Value must equal N.
    Eq(i64),
    /// Value must not equal N.
    Ne(i64),
    /// Value must be positive (> 0).
    Positive,
    /// Value must be even.
    Even,
    /// Result must conform to the given schema (stubbed, always true in v1).
    FromSchema(String),
    /// Result must be convertible to the given schema (stubbed, always true in v1).
    ToSchema(String),
    /// Arbitrary constraint string (evaluated by external resolver in v1).
    Custom(String),
}

impl HintRule {
    /// Parse a constraint string into a HintRule.
    ///
    /// Supported formats:
    ///   "gt 10", "lt 5", "eq 42", "ne -1"
    ///   "positive", "even"
    ///   "from schema:<id>", "to schema:<id>"
    ///   "custom:<text>" or any unrecognized string
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("positive") {
            return Some(Self::Positive);
        }
        if s.eq_ignore_ascii_case("even") {
            return Some(Self::Even);
        }
        if let Some(val) = s.strip_prefix("gt ") {
            return val.trim().parse::<i64>().ok().map(Self::Gt);
        }
        if let Some(val) = s.strip_prefix("lt ") {
            return val.trim().parse::<i64>().ok().map(Self::Lt);
        }
        if let Some(val) = s.strip_prefix("eq ") {
            return val.trim().parse::<i64>().ok().map(Self::Eq);
        }
        if let Some(val) = s.strip_prefix("ne ") {
            return val.trim().parse::<i64>().ok().map(Self::Ne);
        }
        if let Some(id) = s.strip_prefix("from schema:") {
            return Some(Self::FromSchema(id.trim().to_string()));
        }
        if let Some(id) = s.strip_prefix("to schema:") {
            return Some(Self::ToSchema(id.trim().to_string()));
        }
        if let Some(text) = s.strip_prefix("custom:") {
            return Some(Self::Custom(text.trim().to_string()));
        }
        None
    }

    /// Describe this rule as a human-readable string.
    pub fn describe(&self) -> String {
        match self {
            Self::Gt(v) => format!("value > {}", v),
            Self::Lt(v) => format!("value < {}", v),
            Self::Eq(v) => format!("value == {}", v),
            Self::Ne(v) => format!("value != {}", v),
            Self::Positive => "value > 0".into(),
            Self::Even => "value is even".into(),
            Self::FromSchema(id) => format!("conforms to schema '{}'", id),
            Self::ToSchema(id) => format!("convertible to schema '{}'", id),
            Self::Custom(t) => format!("custom: {}", t),
        }
    }

    /// Check whether `value` satisfies this numeric constraint.
    ///
    /// For schema-based rules (FromSchema, ToSchema) and Custom, returns
    /// `true` in v1 (stubbed, to be replaced by real resolution).
    pub fn check_numeric(&self, value: i64) -> bool {
        match self {
            Self::Gt(v) => value > *v,
            Self::Lt(v) => value < *v,
            Self::Eq(v) => value == *v,
            Self::Ne(v) => value != *v,
            Self::Positive => value > 0,
            Self::Even => value % 2 == 0,
            Self::FromSchema(_) | Self::ToSchema(_) | Self::Custom(_) => true,
        }
    }

    /// Return the operand transform for this rule, if any.
    ///
    /// Some constraints act as operand transforms (e.g., "map double")
    /// rather than result gates. Returns `None` for pure gate constraints.
    pub fn operand_transform(&self) -> Option<fn(i64) -> i64> {
        None // v1: no transforms; reserved for future "map" type constraints
    }
}

impl std::fmt::Display for HintRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.describe())
    }
}

// ── HintEntry ───────────────────────────────────────────────────────────

/// A registered hint with metadata.
#[derive(Debug, Clone)]
pub struct HintEntry {
    /// Unique hint identifier.
    pub id: String,
    /// The constraint rule.
    pub rule: HintRule,
    /// Human-readable description (cached from rule).
    pub description: String,
}

// ── HintEngine ──────────────────────────────────────────────────────────

/// Registry of active constraints evaluated during Intent resolution.
///
/// Analogy to mind-mem's pheromone decay and nex-calc's constraint list.
/// In v1, hints are simple rules evaluated in order. Future versions may
/// add time-based decay (pheromone weakening) for true stigmergy.
pub struct HintEngine {
    hints: Mutex<Vec<HintEntry>>,
}

impl HintEngine {
    /// Create a new empty hint engine.
    pub fn new() -> Self {
        Self {
            hints: Mutex::new(Vec::new()),
        }
    }

    /// Register a new hint rule.
    pub fn add(&self, id: &str, rule: HintRule) {
        let entry = HintEntry {
            id: id.to_string(),
            description: rule.describe(),
            rule,
        };
        self.hints.lock().expect("HintEngine lock").push(entry);
    }

    /// Remove a hint by ID.
    pub fn remove(&self, id: &str) {
        self.hints
            .lock()
            .expect("HintEngine lock")
            .retain(|h| h.id != id);
    }

    /// Remove all hints.
    pub fn clear(&self) {
        self.hints.lock().expect("HintEngine lock").clear();
    }

    /// Return the number of registered hints.
    pub fn len(&self) -> usize {
        self.hints.lock().expect("HintEngine lock").len()
    }

    /// Returns true if no hints are registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check whether a numeric result satisfies ALL active hints.
    ///
    /// Returns `Ok(())` if all constraints pass.
    /// Returns `Err` with the first failing rule description.
    pub fn check_numeric(&self, value: i64) -> Result<(), String> {
        let hints = self.hints.lock().expect("HintEngine lock");
        for hint in hints.iter() {
            if !hint.rule.check_numeric(value) {
                return Err(format!(
                    "hint '{}' violated: {} (value was {})",
                    hint.id, hint.description, value
                ));
            }
        }
        Ok(())
    }

    /// Return all active hints as (id, description) pairs.
    pub fn all(&self) -> Vec<(String, String)> {
        self.hints
            .lock()
            .expect("HintEngine lock")
            .iter()
            .map(|h| (h.id.clone(), h.description.clone()))
            .collect()
    }

    /// Return a single hint by ID, if present.
    pub fn get(&self, id: &str) -> Option<HintEntry> {
        self.hints
            .lock()
            .expect("HintEngine lock")
            .iter()
            .find(|h| h.id == id)
            .cloned()
    }
}

impl Default for HintEngine {
    fn default() -> Self {
        Self::new()
    }
}
