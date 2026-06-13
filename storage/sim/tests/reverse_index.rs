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

use nexus_model::{Content, Fact, FactCapable, FihHash, Intent, IntentCapable};
use nexus_storage_sim::{FihStorage, SimIo};

fn storage() -> FihStorage<SimIo> {
    FihStorage::new(SimIo::new(), "test")
}

fn fact(id: &str) -> Fact {
    Fact {
        id: FihHash(id.into()),
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
        id: FihHash(id.into()),
        from_facts: from.into_iter().map(|s| s.to_string()).collect(),
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

    FactCapable::submit_fact(&store, &fact("f_a")).unwrap();
    FactCapable::submit_fact(&store, &fact("f_b")).unwrap();

    IntentCapable::submit_intent(&store, &intent("i1", vec!["f_a"])).unwrap();
    IntentCapable::submit_intent(&store, &intent("i2", vec!["f_a", "f_b"])).unwrap();

    let refs_a = store.intents_by_fact("f_a");
    assert_eq!(refs_a.len(), 2);
    assert!(refs_a.iter().any(|s| s == "i1"));
    assert!(refs_a.iter().any(|s| s == "i2"));

    let refs_b = store.intents_by_fact("f_b");
    assert_eq!(refs_b.len(), 1);
    assert!(refs_b.iter().any(|s| s == "i2"));

    assert!(store.intents_by_fact("nonexistent").is_empty());
}

#[test]
fn test_by_from_fact_cleared_on_conclude() {
    let store = storage();

    FactCapable::submit_fact(&store, &fact("f_base")).unwrap();
    IntentCapable::submit_intent(&store, &intent("i_concl", vec!["f_base"])).unwrap();

    assert_eq!(store.intents_by_fact("f_base").len(), 1);

    IntentCapable::claim_intent(&store, "i_concl", "alice").unwrap();
    IntentCapable::conclude_intent(&store, "i_concl", "done").unwrap();

    assert!(
        store.intents_by_fact("f_base").is_empty(),
        "conclude must remove intent from by_from_fact reverse index"
    );
}

#[test]
fn test_by_from_fact_rebuild_from_io() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "test");

    FactCapable::submit_fact(&store, &fact("f_x")).unwrap();
    IntentCapable::submit_intent(&store, &intent("i_ref", vec!["f_x"])).unwrap();

    store.flush_pending().unwrap();

    let store2 = FihStorage::new(io, "test");
    store2.rebuild_cache().unwrap();

    let refs = store2.intents_by_fact("f_x");
    assert_eq!(refs.len(), 1);
    assert!(refs.iter().any(|s| s == "i_ref"));
}
