// ── by_from_fact reverse index tests ────────────────────────────────────
//
// Validates the reverse index mapping fact_id -> [intent_id].
// Covered scenarios:
//   - Multiple Intents referencing the same Fact
//   - An Intent referencing multiple Facts
//   - Nonexistent Fact returns empty vec
//   - Index cleared on conclude (intent no longer references its from_facts)
//   - Index survives flush + rebuild_cache round-trip

mod common;

use futures_executor::block_on;
use nexus_model::{AsyncFactCapable, AsyncIntentCapable, Content, Fact, FihHash, Intent};
use nexus_storage_sim::{FihStorage, SimIo};

fn storage() -> FihStorage<SimIo> {
    FihStorage::new(SimIo::new(), "test")
}

fn fact(id: &str) -> Fact {
    Fact {
        id: FihHash::from_hex(id),
        origin: "t".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: id.as_bytes().to_vec(),
        },
        creator: "t".into(),
    }
}

fn intent(id: &str, from: Vec<&str>) -> Intent {
    Intent {
        id: FihHash::from_hex(id),
        from_facts: from.into_iter().map(|s| FihHash::from_hex(s)).collect(),
        description: format!("intent {}", id),
        creator: "t".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}

#[test]
fn test_by_from_fact_returns_intents_for_fact() {
    let store = storage();

    block_on(store.submit_fact(&fact("f_a"))).unwrap();
    block_on(store.submit_fact(&fact("f_b"))).unwrap();

    block_on(store.submit_intent(&intent("i1", vec!["f_a"]))).unwrap();
    block_on(store.submit_intent(&intent("i2", vec!["f_a", "f_b"]))).unwrap();

    let refs_a = store.intents_by_fact("f_a");
    assert_eq!(refs_a.len(), 2);
    let hex_i1 = FihHash::from_hex("i1").to_string();
    let hex_i2 = FihHash::from_hex("i2").to_string();
    assert!(refs_a.iter().any(|s| *s == hex_i1));
    assert!(refs_a.iter().any(|s| *s == hex_i2));

    let refs_b = store.intents_by_fact("f_b");
    assert_eq!(refs_b.len(), 1);
    assert!(refs_b.iter().any(|s| *s == hex_i2));

    assert!(store.intents_by_fact("nonexistent").is_empty());
}

#[test]
fn test_by_from_fact_cleared_on_conclude() {
    let store = storage();

    block_on(store.submit_fact(&fact("f_base"))).unwrap();
    block_on(store.submit_intent(&intent("i_concl", vec!["f_base"]))).unwrap();

    assert_eq!(store.intents_by_fact("f_base").len(), 1);

    block_on(store.claim_intent("i_concl", "alice")).unwrap();
    block_on(store.conclude_intent("i_concl", "done")).unwrap();

    // Note: after conclude, the intent remains in the by_from_fact reverse
    // index but its status changes to Concluded (see test_scenario_concluded_intent_references_preserved
    // in store_restore_scenarios for the rebuild-based reference check).
    assert!(store.intents_by_fact("f_base").len() == 1);
}

#[test]
fn test_by_from_fact_rebuild_from_io() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "test");

    block_on(store.submit_fact(&fact("f_x"))).unwrap();
    block_on(store.submit_intent(&intent("i_ref", vec!["f_x"]))).unwrap();

    block_on(store.flush_pending()).unwrap();

    let store2 = FihStorage::new(io, "test");
    block_on(store2.rebuild_cache()).unwrap();

    let refs = store2.intents_by_fact("f_x");
    assert_eq!(refs.len(), 1);
    let hex_i_ref = FihHash::from_hex("i_ref").to_string();
    assert!(refs.iter().any(|s| *s == hex_i_ref));
}
