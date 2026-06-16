// Full lifecycle test for FihStorage<SimIo>.

use nexus_model::{Content, Fact, FactCapable, FihHash, Intent, IntentCapable, StorageRead};
use nexus_storage_sim::{FihStorage, SimIo};

#[test]
fn test_sim_fact_submit_and_read() {
    let io = SimIo::new();
    let storage = FihStorage::new(io, "test");

    storage
        .submit_fact(&Fact {
            id: FihHash("f001".into()),
            origin: "sim".into(),
            content: Content::from("hello from sim"),
            creator: "tester".into(),
        })
        .unwrap();

    let state = storage.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content.as_str().unwrap(), "hello from sim");
}

#[test]
fn test_sim_full_lifecycle() {
    let io = SimIo::new();
    let storage = FihStorage::new(io, "test");

    storage
        .submit_fact(&Fact {
            id: FihHash("f_base".into()),
            origin: "sim".into(),
            content: Content::from("base"),
            creator: "alice".into(),
        })
        .unwrap();

    storage
        .submit_intent(&Intent {
            id: FihHash("i001".into()),
            from_facts: vec!["f_base".into()],
            description: "test intent".into(),
            creator: "bob".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        })
        .unwrap();

    storage.claim_intent("i001", "alice").unwrap();
    storage.heartbeat("i001", "alice").unwrap();
    let result = storage.conclude_intent("i001", "done").unwrap();

    assert!(result.id.0.starts_with("f_concl_"));
    let state = storage.read_state();
    assert_eq!(state.facts.len(), 2);
    assert_eq!(state.intents.len(), 1);
    assert!(state.intents[0].concluded_at.is_some());
}

// ── FihSession hydrate/flush test ─────────────────────────────────────────

#[test]
fn test_session_hydrate_flush() {
    use nexus_model::{Content, Fact, FactCapable, FihHash, StorageRead};
    use nexus_storage_sim::FihSession;

    let io = SimIo::new();
    let mut session = FihSession::new(io.clone(), "test");

    // Write a fact via storage
    let fact = Fact {
        id: FihHash("f001".into()),
        origin: "test".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"hello".to_vec(),
        },
        creator: "alice".into(),
    };
    session.storage.submit_fact(&fact).unwrap();

    // Not yet flushed → data is in buffer, not in IO
    assert!(!session.is_flushed());

    // Flush → data goes to IO
    session.flush().unwrap();
    assert!(session.is_flushed());

    // Read back from a fresh session on the same IO instance
    let mut session2 = FihSession::new(io, "test");
    session2.hydrate().unwrap();
    let state = session2.storage.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].id.0, "f001");
}
