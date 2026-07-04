// Calculator engine — the core of nex-calc.
//
// CalcEngine implements a FIH-based calculator where:
//   F (Fact)  = number stored as an immutable, content-addressed Fact
//   I (Intent) = operator that defines a traversal through the FIH space
//   H (Hint)   = constraint or transform on computation
//
// Computation is NOT a CPU instruction. It is a traversal of the
// FIH coordinate space: from operand Facts, through an operator Intent,
// constrained by Hints, producing a result Fact at a new coordinate.
//
// The inefficiency is intentional. It reveals the algebraic structure
// F × I × H → F' that underpins the SSCCS/neXus philosophy.

use std::collections::HashMap;
use std::fmt;

use nexus_model::{Content, Fact, FihHash, Hint, Intent};

use crate::hint::Constraint;
use crate::ops::OpType;

/// MIME type used for number Facts.
const NUMBER_MIME: &str = "application/x-nex-calc-number";

/// Errors that can occur during engine operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalcError {
    /// Referenced Fact not found.
    FactNotFound(String),
    /// Referenced Intent not found.
    IntentNotFound(String),
    /// An operator-level error (e.g., division by zero).
    OpError(String),
    /// A Hint constraint was violated.
    ConstraintViolated {
        hint_id: String,
        constraint: String,
        result: i64,
    },
    /// Intent was already resolved.
    AlreadyResolved(String),
    /// Invalid number encoding in Fact content.
    InvalidNumberEncoding(String),
}

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalcError::FactNotFound(id) => write!(f, "fact not found: {id}"),
            CalcError::IntentNotFound(id) => write!(f, "intent not found: {id}"),
            CalcError::OpError(msg) => write!(f, "operator error: {msg}"),
            CalcError::ConstraintViolated {
                hint_id,
                constraint,
                result,
            } => write!(
                f,
                "constraint violated [{hint_id}]: {constraint} (got {result})"
            ),
            CalcError::AlreadyResolved(id) => write!(f, "intent already resolved: {id}"),
            CalcError::InvalidNumberEncoding(id) => {
                write!(f, "invalid number encoding in fact: {id}")
            }
        }
    }
}

/// A resolved Intent: the original Intent plus its result Fact.
#[derive(Debug, Clone)]
pub struct ResolvedIntent {
    pub intent_id: FihHash,
    pub op: OpType,
    pub lhs: i64,
    pub rhs: i64,
    pub result_id: FihHash,
    pub result_value: i64,
}

/// The FIH-based calculator engine.
///
/// All state is stored as Facts, Intents, and Hints. Computation
/// happens through Intent resolution, which traverses the FIH space.
pub struct CalcEngine {
    facts: HashMap<FihHash, Fact>,
    intents: HashMap<FihHash, Intent>,
    hints: Vec<(FihHash, Hint, Constraint)>,
    /// Monotonically increasing coordinate counter. Each new Fact
    /// receives a coordinate that reflects its position in the
    /// state space traversal history.
    coord_counter: u64,
}

impl CalcEngine {
    /// Create a new, empty calculator engine.
    pub fn new() -> Self {
        CalcEngine {
            facts: HashMap::new(),
            intents: HashMap::new(),
            hints: Vec::new(),
            coord_counter: 0,
        }
    }

    // ── Fact operations ───────────────────────────────────────────

    /// Store a number in the FIH space as a Fact.
    ///
    /// The number is encoded as little-endian bytes in Content.
    /// The Fact's id is content-addressed via SHA256 of the value.
    /// If a Fact with the same value already exists, returns its id
    /// without creating a duplicate (immutability guarantee).
    pub fn put(&mut self, value: i64) -> FihHash {
        let id = make_number_fact_id(value);
        if self.facts.contains_key(&id) {
            return id;
        }
        let fact = Fact {
            id,
            origin: "nex-calc".into(),
            content: encode_number(value),
            creator: "user".into(),
        };
        self.coord_counter += 1;
        self.facts.insert(id, fact);
        id
    }

    /// Read a number from a Fact in the FIH space.
    ///
    /// Returns `None` if the fact does not exist or does not contain
    /// a valid number encoding.
    pub fn get(&self, fact_id: &FihHash) -> Option<i64> {
        let fact = self.facts.get(fact_id)?;
        decode_number(&fact.content)
    }

    /// Look up a Fact by its short hex prefix.
    ///
    /// Returns `None` if no Fact matches or if the prefix is ambiguous.
    pub fn find_fact(&self, prefix: &str) -> Option<&Fact> {
        let prefix_lower = prefix.to_lowercase();
        let matches: Vec<&Fact> = self
            .facts
            .values()
            .filter(|f| f.id.to_string().to_lowercase().starts_with(&prefix_lower))
            .collect();
        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        }
    }

    // ── Intent operations ─────────────────────────────────────────

    /// Create an operator Intent in the FIH space.
    ///
    /// The Intent encodes the operator type and references to operand
    /// Facts. It represents a directional vector through the FIH
    /// coordinate space: from (lhs, rhs) toward a result coordinate.
    pub fn op(&mut self, op: OpType, lhs_id: &FihHash, rhs_id: &FihHash) -> Result<FihHash, CalcError> {
        // Verify operands exist.
        if !self.facts.contains_key(lhs_id) {
            return Err(CalcError::FactNotFound(lhs_id.to_string()));
        }
        if !self.facts.contains_key(rhs_id) {
            return Err(CalcError::FactNotFound(rhs_id.to_string()));
        }

        let id = make_intent_id(op, lhs_id, rhs_id);
        let intent = Intent {
            id,
            from_facts: vec![*lhs_id, *rhs_id],
            description: format!("op:{}", op),
            creator: "user".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: Some(self.coord_counter),
            is_concluded: false,
            concluded_at: None,
        };
        self.intents.insert(id, intent);
        Ok(id)
    }

    /// Resolve an Intent — this IS the computation.
    ///
    /// Resolution traverses the FIH space:
    /// 1. Read operand Facts from the blackboard
    /// 2. Decode numbers from Facts
    /// 3. Apply operand-level Hint transforms
    /// 4. Apply the operator
    /// 5. Check result-level Hint constraints
    /// 6. Create a result Fact at a new coordinate
    /// 7. Link the Intent to its result Fact
    ///
    /// The traversal itself is the computation. The result Fact's
    /// coordinate reflects its position in the resolution history.
    pub fn resolve(&mut self, intent_id: &FihHash) -> Result<ResolvedIntent, CalcError> {
        // Look up the Intent.
        let intent = self
            .intents
            .get(intent_id)
            .ok_or_else(|| CalcError::IntentNotFound(intent_id.to_string()))?;

        if intent.is_concluded {
            return Err(CalcError::AlreadyResolved(intent_id.to_string()));
        }

        // Parse the operator from the Intent's description.
        let op_str = intent
            .description
            .strip_prefix("op:")
            .ok_or_else(|| CalcError::OpError("invalid intent description".into()))?;
        let op = OpType::parse(op_str)
            .ok_or_else(|| CalcError::OpError("unknown operator in intent".into()))?;

        // Read operand Facts.
        let lhs_id = intent.from_facts.first().copied().unwrap_or(FihHash([0; 32]));
        let rhs_id = intent.from_facts.get(1).copied().unwrap_or(FihHash([0; 32]));

        let lhs = self
            .get(&lhs_id)
            .ok_or_else(|| CalcError::FactNotFound(lhs_id.to_string()))?;
        let rhs = self
            .get(&rhs_id)
            .ok_or_else(|| CalcError::FactNotFound(rhs_id.to_string()))?;

        // Apply operand-level Hint transforms.
        let (lhs, rhs) = self.apply_operand_transforms(lhs, rhs);

        // Apply the operator.
        let raw_result = op
            .apply(lhs, rhs)
            .map_err(|e| CalcError::OpError(e.to_string()))?;

        // Check result-level Hint constraints.
        self.check_constraints(raw_result)?;

        // Create the result Fact at a new coordinate.
        let result_id = make_number_fact_id(raw_result);
        if !self.facts.contains_key(&result_id) {
            self.coord_counter += 1;
            let result_fact = Fact {
                id: result_id,
                origin: format!("nex-calc:resolve:{}", intent_id),
                content: encode_number(raw_result),
                creator: "nex-calc".into(),
            };
            self.facts.insert(result_id, result_fact);
        }

        // Mark the Intent as concluded.
        let intent = self.intents.get_mut(intent_id).unwrap();
        intent.is_concluded = true;
        intent.to_fact_id = Some(result_id);
        intent.concluded_at = Some(self.coord_counter);

        Ok(ResolvedIntent {
            intent_id: *intent_id,
            op,
            lhs,
            rhs,
            result_id,
            result_value: raw_result,
        })
    }

    // ── Hint operations ───────────────────────────────────────────

    /// Add a constraint Hint to the engine.
    ///
    /// Hints are applied in order during Intent resolution.
    /// Operand transforms are applied before operator execution;
    /// result constraints are checked after.
    pub fn constrain(&mut self, constraint: Constraint) -> FihHash {
        let id = make_hint_id(&constraint);
        let hint = Hint {
            id,
            content: constraint.to_string(),
            creator: "user".into(),
        };
        self.hints.push((id, hint, constraint));
        id
    }

    /// Remove all Hints from the engine.
    pub fn clear_hints(&mut self) {
        self.hints.clear();
    }

    // ── Query operations ──────────────────────────────────────────

    /// Return all stored Facts, ordered by coordinate (insertion order).
    pub fn list_facts(&self) -> Vec<&Fact> {
        let mut facts: Vec<&Fact> = self.facts.values().collect();
        facts.sort_by_key(|f| f.id.to_string());
        facts
    }

    /// Return all Intents with their status.
    pub fn list_intents(&self) -> Vec<&Intent> {
        let mut intents: Vec<&Intent> = self.intents.values().collect();
        intents.sort_by_key(|i| i.created_at);
        intents
    }

    /// Return all active Hints.
    pub fn list_hints(&self) -> Vec<(&FihHash, &Constraint)> {
        self.hints.iter().map(|(id, _, c)| (id, c)).collect()
    }

    /// Return the current coordinate counter (step count).
    pub fn step_count(&self) -> u64 {
        self.coord_counter
    }

    /// Total number of Facts (including result Facts from resolved Intents).
    pub fn fact_count(&self) -> usize {
        self.facts.len()
    }

    /// Number of pending (unresolved) Intents.
    pub fn pending_count(&self) -> usize {
        self.intents.values().filter(|i| !i.is_concluded).count()
    }

    // ── Internal helpers ──────────────────────────────────────────

    fn apply_operand_transforms(&self, lhs: i64, rhs: i64) -> (i64, i64) {
        let mut l = lhs;
        let mut r = rhs;
        for (_, _, constraint) in &self.hints {
            let (nl, nr) = constraint.transform_operands(l, r);
            l = nl;
            r = nr;
        }
        (l, r)
    }

    fn check_constraints(&self, result: i64) -> Result<(), CalcError> {
        for (hint_id, _, constraint) in &self.hints {
            if !constraint.check(result) {
                return Err(CalcError::ConstraintViolated {
                    hint_id: hint_id.to_string(),
                    constraint: constraint.to_string(),
                    result,
                });
            }
        }
        Ok(())
    }
}

impl Default for CalcEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Number encoding ───────────────────────────────────────────────

/// Encode an i64 into Content using little-endian bytes.
fn encode_number(value: i64) -> Content {
    Content {
        mime_type: NUMBER_MIME.into(),
        data: value.to_le_bytes().to_vec(),
    }
}

/// Decode an i64 from Content.
fn decode_number(content: &Content) -> Option<i64> {
    if content.mime_type != NUMBER_MIME {
        return None;
    }
    if content.data.len() != 8 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&content.data);
    Some(i64::from_le_bytes(bytes))
}

// ── Content-addressed ID generation ───────────────────────────────

/// Generate a content-addressed FihHash for a number Fact.
fn make_number_fact_id(value: i64) -> FihHash {
    FihHash::new(&[&value.to_string()], "nex-calc-number")
}

/// Generate a content-addressed FihHash for an operator Intent.
fn make_intent_id(op: OpType, lhs_id: &FihHash, rhs_id: &FihHash) -> FihHash {
    FihHash::new(
        &[
            &lhs_id.to_string(),
            &rhs_id.to_string(),
            op.symbol(),
        ],
        "nex-calc-intent",
    )
}

/// Generate a content-addressed FihHash for a Hint.
fn make_hint_id(constraint: &Constraint) -> FihHash {
    FihHash::new(&[&constraint.to_string()], "nex-calc-hint")
}


// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_put_and_get() {
        let mut engine = CalcEngine::new();
        let id = engine.put(42);
        assert_eq!(engine.get(&id), Some(42));
    }

    #[test]
    fn test_put_deduplicates() {
        let mut engine = CalcEngine::new();
        let id1 = engine.put(42);
        let id2 = engine.put(42);
        assert_eq!(id1, id2);
        assert_eq!(engine.fact_count(), 1);
    }

    #[test]
    fn test_add_intent_and_resolve() {
        let mut engine = CalcEngine::new();
        let a = engine.put(3);
        let b = engine.put(5);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        let resolved = engine.resolve(&intent_id).unwrap();
        assert_eq!(resolved.result_value, 8);
        assert_eq!(resolved.op, OpType::Add);
    }

    #[test]
    fn test_subtract() {
        let mut engine = CalcEngine::new();
        let a = engine.put(10);
        let b = engine.put(3);
        let intent_id = engine.op(OpType::Sub, &a, &b).unwrap();
        let resolved = engine.resolve(&intent_id).unwrap();
        assert_eq!(resolved.result_value, 7);
    }

    #[test]
    fn test_multiply() {
        let mut engine = CalcEngine::new();
        let a = engine.put(6);
        let b = engine.put(7);
        let intent_id = engine.op(OpType::Mul, &a, &b).unwrap();
        let resolved = engine.resolve(&intent_id).unwrap();
        assert_eq!(resolved.result_value, 42);
    }

    #[test]
    fn test_divide() {
        let mut engine = CalcEngine::new();
        let a = engine.put(42);
        let b = engine.put(6);
        let intent_id = engine.op(OpType::Div, &a, &b).unwrap();
        let resolved = engine.resolve(&intent_id).unwrap();
        assert_eq!(resolved.result_value, 7);
    }

    #[test]
    fn test_division_by_zero() {
        let mut engine = CalcEngine::new();
        let a = engine.put(10);
        let b = engine.put(0);
        let intent_id = engine.op(OpType::Div, &a, &b).unwrap();
        let result = engine.resolve(&intent_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_constraint_violation() {
        let mut engine = CalcEngine::new();
        engine.constrain(Constraint::GreaterThan(10));
        let a = engine.put(3);
        let b = engine.put(5);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        let result = engine.resolve(&intent_id);
        assert!(matches!(result, Err(CalcError::ConstraintViolated { .. })));
    }

    #[test]
    fn test_constraint_satisfied() {
        let mut engine = CalcEngine::new();
        engine.constrain(Constraint::GreaterThan(5));
        let a = engine.put(3);
        let b = engine.put(5);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        let resolved = engine.resolve(&intent_id).unwrap();
        assert_eq!(resolved.result_value, 8);
    }

    #[test]
    fn test_map_double_transform() {
        let mut engine = CalcEngine::new();
        engine.constrain(Constraint::MapDouble);
        let a = engine.put(3);
        let b = engine.put(5);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        let resolved = engine.resolve(&intent_id).unwrap();
        // (3*2) + (5*2) = 6 + 10 = 16
        assert_eq!(resolved.result_value, 16);
    }

    #[test]
    fn test_already_resolved() {
        let mut engine = CalcEngine::new();
        let a = engine.put(1);
        let b = engine.put(2);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        engine.resolve(&intent_id).unwrap();
        let result = engine.resolve(&intent_id);
        assert!(matches!(result, Err(CalcError::AlreadyResolved(_))));
    }

    #[test]
    fn test_fact_not_found_in_op() {
        let mut engine = CalcEngine::new();
        let fake_id = make_number_fact_id(999);
        let b = engine.put(5);
        let result = engine.op(OpType::Add, &fake_id, &b);
        assert!(matches!(result, Err(CalcError::FactNotFound(_))));
    }

    #[test]
    fn test_multiple_hints() {
        let mut engine = CalcEngine::new();
        engine.constrain(Constraint::MapDouble);
        engine.constrain(Constraint::GreaterThan(10));
        engine.constrain(Constraint::IsEven);
        let a = engine.put(2);
        let b = engine.put(3);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        // (2*2) + (3*2) = 4 + 6 = 10, 10 > 10 is false
        let result = engine.resolve(&intent_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_result_fact_persists() {
        let mut engine = CalcEngine::new();
        let a = engine.put(7);
        let b = engine.put(8);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        let resolved = engine.resolve(&intent_id).unwrap();
        // The result Fact should now be readable.
        assert_eq!(engine.get(&resolved.result_id), Some(15));
        // Original Facts still exist.
        assert_eq!(engine.get(&a), Some(7));
        assert_eq!(engine.get(&b), Some(8));
    }

    #[test]
    fn test_clear_hints() {
        let mut engine = CalcEngine::new();
        engine.constrain(Constraint::GreaterThan(10));
        engine.clear_hints();
        let a = engine.put(3);
        let b = engine.put(5);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        let resolved = engine.resolve(&intent_id).unwrap();
        // No constraints after clear, so 8 passes.
        assert_eq!(resolved.result_value, 8);
    }

    #[test]
    fn test_fact_count_includes_results() {
        let mut engine = CalcEngine::new();
        let a = engine.put(1);
        let b = engine.put(2);
        let intent_id = engine.op(OpType::Add, &a, &b).unwrap();
        assert_eq!(engine.fact_count(), 2);
        engine.resolve(&intent_id).unwrap();
        assert_eq!(engine.fact_count(), 3); // includes result fact
    }
}
