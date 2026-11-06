// ── Storage contract: what a store implementation supplies ──────────────
//
// These traits are the port a fabric exposes. An implementation supplies them; this
// crate supplies one. The set covers reading a record by identity, visiting the records
// a store holds, and settling the state an implementation keeps for itself. Nothing here
// says how a record is stored, which is what lets a store live outside this crate.
//
// The async traits in `fih-model` cover submission and state reads. The operations below
// are the ones a consumer needs to read one record, to enumerate without materializing the
// whole state, and to close a session.

#![allow(async_fn_in_trait)]

use crate::core::record::FactRecord;
use crate::{Content, FihHash};
use alloc::string::String;

/// Read one record by its identifier.
///
/// A reader opens a store with [`StoreMaintenance::rebuild`] before reading by identity: an
/// implementation that answers from what it derives may hold nothing until it has loaded.
pub trait FactById {
    /// The content, hash, origin, and creator of a fact, or none when the store holds no
    /// record at that identifier.
    async fn fact_by_id(&self, id: &str) -> Option<(Content, FihHash, String, String)>;

    /// The framing of a fact: what the record says about itself, without its payload.
    async fn fact_framing(&self, id: &str) -> Option<FactRecord>;
}

/// Visit the facts a store holds, one at a time.
///
/// The visitor returns whether to keep going, so a read that is looking for one record
/// stops at it rather than reading the rest of the volume.
pub trait FactWalk {
    async fn for_each_fact<F>(&self, visit: F) -> Result<(), String>
    where
        F: FnMut(&FactRecord, Content) -> bool;

    /// The same walk without the payload of each record.
    async fn for_each_fact_record<F>(&self, visit: F) -> Result<(), String>
    where
        F: FnMut(&FactRecord) -> bool;
}

/// Operations an implementation performs on the state it holds.
pub trait StoreMaintenance {
    /// Hand everything the session holds to the medium.
    async fn flush(&self) -> Result<(), String>;

    /// Rebuild whatever the implementation derives from the records.
    ///
    /// This is the open step. A reader calls it once before reading by identity, and a
    /// writer calls it once before accepting a write that has to know what the store holds.
    async fn rebuild(&self) -> Result<(), String>;
}
