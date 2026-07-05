use crate::error::BlackboardError;

/// Backend supports governance: schema admission, constraint evaluation,
/// and evidence recording.
///
/// A storage that implements this trait can gate writes with schema checks
/// and hint constraints, and maintain an append-only audit chain.
/// Together with `FihPersistence`, a governed blackboard is formed:
///
/// ```ignore
/// pub trait GovernedBlackboard: FihPersistence + GovernanceCapable {}
/// ```
pub trait GovernanceCapable {
    /// Register a schema for admission. Returns the SHA-256 hex hash.
    fn register_schema(&self, schema_id: &str, schema: &[u8]) -> String;

    /// Admit-or-reject a fact write against a registered schema.
    fn admit_fact(&self, schema_id: &str, data: &[u8]) -> Result<(), BlackboardError>;

    /// Check a numeric constraint against all active hints.
    fn check_hints(&self, value: i64) -> Result<(), BlackboardError>;

    /// Return the evidence chain tip hash, if any entries exist.
    fn evidence_tip(&self) -> Option<String>;

    /// Returns true if governance checks are active.
    fn governance_enabled(&self) -> bool;
}

impl<T: GovernanceCapable> GovernanceCapable for &T {
    fn register_schema(&self, schema_id: &str, schema: &[u8]) -> String {
        (**self).register_schema(schema_id, schema)
    }
    fn admit_fact(&self, schema_id: &str, data: &[u8]) -> Result<(), BlackboardError> {
        (**self).admit_fact(schema_id, data)
    }
    fn check_hints(&self, value: i64) -> Result<(), BlackboardError> {
        (**self).check_hints(value)
    }
    fn evidence_tip(&self) -> Option<String> {
        (**self).evidence_tip()
    }
    fn governance_enabled(&self) -> bool {
        (**self).governance_enabled()
    }
}

impl<T: GovernanceCapable> GovernanceCapable for &mut T {
    fn register_schema(&self, schema_id: &str, schema: &[u8]) -> String {
        (**self).register_schema(schema_id, schema)
    }
    fn admit_fact(&self, schema_id: &str, data: &[u8]) -> Result<(), BlackboardError> {
        (**self).admit_fact(schema_id, data)
    }
    fn check_hints(&self, value: i64) -> Result<(), BlackboardError> {
        (**self).check_hints(value)
    }
    fn evidence_tip(&self) -> Option<String> {
        (**self).evidence_tip()
    }
    fn governance_enabled(&self) -> bool {
        (**self).governance_enabled()
    }
}
