// ── GovernanceGate: schema-based write admission ─────────────────────────
//
// Single choke-point for all write operations. Inspired by mind-mem's
// GovernanceGate.admit() pattern. Every raw-to-schemed-segment
// transformation passes through the gate before reaching storage.
//
// The gate maintains an in-memory schema registry. A schema must be
// registered (via register_schema) before data claiming that schema can
// be admitted. This is a structural check — in v1 the gate verifies the
// schema exists and tracks its identity hash.
//
// The gate keeps its schema registry behind Cell2, the store's
// platform-adaptive cell: critical-section interior mutability on native
// and MCU targets, a plain RefCell on wasm32-unknown-unknown. Shared
// borrows nest; a second exclusive borrow in the same thread panics.

use crate::core::index::Cell2;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// `std::collections::HashMap` exists only under the std feature; alloc has
// no HashMap. The no_std path substitutes a BTreeMap, which satisfies the
// same contract (iteration order is unspecified for HashMap, so callers
// cannot rely on it).
#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap as HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

/// Error returned when the governance gate blocks a write.
#[derive(Debug, Clone)]
pub struct GovernanceBypassError {
    pub reason: String,
}

impl core::fmt::Display for GovernanceBypassError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "GovernanceGate blocked: {}", self.reason)
    }
}

// ── GovernanceGate ──────────────────────────────────────────────────────

/// Schema-based write admission gate.
///
/// Every write must pass through `admit()` before touching storage.
/// The gate checks that the data's claimed schema has been registered
/// and (in future versions) that the data structurally conforms.
pub struct GovernanceGate {
    /// In-memory schema registry: schema_id → SHA-256 hex hash.
    schemas: Cell2<HashMap<String, String>>,
}

impl GovernanceGate {
    /// Create a new gate with an empty schema registry.
    pub fn new() -> Self {
        Self {
            schemas: Cell2::new(HashMap::new()),
        }
    }

    /// Register a schema. Returns the computed spec-hash (hex-encoded SHA-256).
    ///
    /// Once registered, data claiming this schema_id can be admitted.
    /// The spec-hash is stored so future `verify()` calls can detect drift.
    pub fn register_schema(&self, schema_id: &str, schema: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(schema);
        let hash = hex_encode(&h.finalize());
        self.schemas
            .borrow_mut()
            .insert(schema_id.to_string(), hash.clone());
        hash
    }

    /// Remove a schema from the registry.
    pub fn unregister_schema(&self, schema_id: &str) {
        self.schemas.borrow_mut().remove(schema_id);
    }

    /// Check whether a schema_id is registered.
    pub fn has_schema(&self, schema_id: &str) -> bool {
        self.schemas.borrow().contains_key(schema_id)
    }

    /// Admit-or-reject a write.
    ///
    /// Returns `Ok(())` if the schema_id is registered (v1: existence check).
    /// Returns `Err(GovernanceBypassError)` if the schema is unknown or
    /// the data does not conform.
    ///
    /// In v1 this is a lightweight existence check. Future versions will
    /// perform structural validation against the registered schema.
    pub fn admit(&self, schema_id: &str, _data: &[u8]) -> Result<(), GovernanceBypassError> {
        let schemas = self.schemas.borrow();
        match schemas.get(schema_id) {
            Some(_hash) => Ok(()),
            None => Err(GovernanceBypassError {
                reason: format!("schema '{}' is not registered", schema_id),
            }),
        }
    }

    /// Verify that a schema's current content matches its registered hash.
    ///
    /// This is the spec-drift detection pattern from mind-mem's SpecBinding.
    /// Returns `Ok(())` if the hash matches or the schema is unregistered.
    pub fn verify(&self, schema_id: &str, schema: &[u8]) -> Result<(), GovernanceBypassError> {
        use sha2::{Digest, Sha256};
        let schemas = self.schemas.borrow();
        match schemas.get(schema_id) {
            Some(expected) => {
                let mut h = Sha256::new();
                h.update(schema);
                let actual = hex_encode(&h.finalize());
                if *expected == actual {
                    Ok(())
                } else {
                    Err(GovernanceBypassError {
                        reason: format!(
                            "schema '{}' hash mismatch: expected {}... got {}...",
                            schema_id,
                            &expected[..16],
                            &actual[..16]
                        ),
                    })
                }
            }
            None => Ok(()),
        }
    }

    /// Return the number of registered schemas.
    pub fn schema_count(&self) -> usize {
        self.schemas.borrow().len()
    }

    /// Return a copy of all registered schema IDs.
    pub fn registered_schemas(&self) -> Vec<String> {
        self.schemas.borrow().keys().cloned().collect()
    }

    /// Clear all registered schemas.
    pub fn clear(&self) {
        self.schemas.borrow_mut().clear();
    }
}

impl Default for GovernanceGate {
    fn default() -> Self {
        Self::new()
    }
}

// ── hex_encode helper ───────────────────────────────────────────────────

/// Format a byte slice as a lowercase hex string without allocating the hex crate.
fn hex_encode(bytes: &[u8]) -> String {
    crate::contract::core::util::hex_encode(bytes)
}
