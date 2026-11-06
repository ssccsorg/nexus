// ── Core primitives ─────────────────────────────────────────────────────
//
// The entity-store family chton owns, and the ordered index. A record, its lifecycle, and
// the store that holds it live in the reference node, not here.

pub mod index;

// Store surface ownership: chton is the behavior layer. The EntityStore
// family (trait + memory + materialized impls) lives in chton::store and
// is re-exported here so nexus consumers keep a stable crate path.
pub use chton::store::{CoordEntityStore, EntityStore, MapEntityStore, MemoryEntityStore};
pub use index::{Cell2, OrderedIndex};
