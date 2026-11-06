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

use crate::core::record::FactRecord;
use crate::{Content, FihHash};
use alloc::string::String;

/// Read one record by its identifier.
pub trait FactById {
    /// The content, hash, origin, and creator of a fact, or none when the network holds no
    /// record at that identifier.
    async fn fact_by_id(&self, id: &str) -> Option<(Content, FihHash, String, String)>;

    /// The framing of a fact: what the record says about itself, without its payload.
    async fn fact_framing(&self, id: &str) -> Option<FactRecord>;
}

/// Visit the facts a network holds, one at a time.
///
/// The visitor returns whether to keep going, so a read that is looking for one record stops
/// at it rather than reading the rest of the network.
pub trait FactWalk {
    async fn for_each_fact<F>(&self, visit: F) -> Result<(), String>
    where
        F: FnMut(&FactRecord, Content) -> bool;

    /// The same walk without the payload of each record.
    async fn for_each_fact_record<F>(&self, visit: F) -> Result<(), String>
    where
        F: FnMut(&FactRecord) -> bool;
}
