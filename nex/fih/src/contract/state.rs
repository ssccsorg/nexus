// ── The state network's read surface ─────────────────────────────────────
//
// nexus is a network of states: it names a record, bounds how a record changes, and answers
// what a network holds. It is neutral about where the records live, so nothing here flushes,
// rebuilds, or names a medium. Holding records is a layer that is implemented using this
// network, and it supplies those operations to itself.
//
// The async traits in `fih-model` carry submission, the lifecycle, and the filtered read. The
// two below complete the read surface: one record by identity, and an enumeration that does
// not materialize the whole state before answering.

#![allow(async_fn_in_trait)]

use crate::{Content, FihHash};
use alloc::string::String;

/// What a fact's record says about itself, without its payload.
///
/// The read surface names this rather than a storage encoding, so an implementation is free
/// to write a different record and answer with the same framing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactFraming {
    pub id: String,
    pub origin: String,
    pub creator: String,
    /// Submission time in nanoseconds since the UNIX epoch.
    pub submitted_at: u64,
    pub content_hash: FihHash,
}

/// Read one record by its identifier.
pub trait FactById {
    /// The content, hash, origin, and creator of a fact, or none when the network holds no
    /// record at that identifier.
    async fn fact_by_id(&self, id: &str) -> Option<(Content, FihHash, String, String)>;

    /// The framing of a fact: what the record says about itself, without its payload.
    async fn fact_framing(&self, id: &str) -> Option<FactFraming>;
}

/// Visit the facts a network holds, one at a time.
///
/// The visitor returns whether to keep going, so a read that is looking for one record stops
/// at it rather than reading the rest of the network.
pub trait FactWalk {
    async fn for_each_fact<F>(&self, visit: F) -> Result<(), String>
    where
        F: FnMut(&FactFraming, Content) -> bool;

    /// The same walk without the payload of each record.
    async fn for_each_fact_record<F>(&self, visit: F) -> Result<(), String>
    where
        F: FnMut(&FactFraming) -> bool;
}
