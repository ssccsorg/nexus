// The reference node's boundary, held as a test rather than as a review rule.
//
// The node demonstrates the contract. It is not the engine that lives elsewhere: it links no
// index over the axes, and its record maps stay keyed by the address. Either one going the
// other way is how a demonstration turns back into the thing it demonstrates.

use std::collections::BTreeMap;

use nex_fih::{Cell2, CoordId};
use nex_io::FileIo;
use nexus_storage_ref::{FactRecord, HintRecord, IntentRecord, RefFihStorage};

/// A box is answered from the listing, so the node links no index over the axes.
#[test]
fn the_node_links_no_index_over_the_axes() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("the manifest sits beside the tests");
    for engine in ["tagma-map", "tagma_map"] {
        assert!(
            !manifest.contains(engine),
            "the reference node depends on {engine}, which is the index that belongs to the engine"
        );
    }
}

/// The record maps are keyed by the address, so a map keyed by its text stops this building.
///
/// The body is never called: it is here to be type-checked, which is what makes the key type
/// a property of the build rather than of a review.
#[allow(dead_code)]
fn the_maps_are_keyed_by_the_address<I: FileIo>(storage: &RefFihStorage<I>) {
    let _: &Cell2<BTreeMap<CoordId, FactRecord>> = &storage.fact_records;
    let _: &Cell2<BTreeMap<CoordId, IntentRecord>> = &storage.intent_records;
    let _: &Cell2<BTreeMap<CoordId, HintRecord>> = &storage.hint_records;
}
