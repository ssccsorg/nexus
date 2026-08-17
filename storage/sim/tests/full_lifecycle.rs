// Full lifecycle test for FihStorage<SimIo>.

use nex_fih::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead, Content, CoordId, Fact, Intent,
};
use nexus_storage_sim::{FihStorage, SimIo};

#[test]
fn test_sim_fact_submit_and_read() {
    let io = SimIo::new();
    let storage = FihStorage::new(io, "test");

    futures_executor::block_on(storage.submit_fact(&Fact::with_id(
        CoordId::resolve("f001"),
        "sim".into(),
        Content::from("hello from sim"),
        "tester".into(),
    )))
    .unwrap();

    let state = futures_executor::block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content.as_str().unwrap(), "hello from sim");
}

#[test]
fn test_sim_full_lifecycle() {
    let io = SimIo::new();
    let storage = FihStorage::new(io, "test");

    futures_executor::block_on(storage.submit_fact(&Fact::with_id(
        CoordId::resolve("f_base"),
        "sim".into(),
        Content::from("base"),
        "alice".into(),
    )))
    .unwrap();

    futures_executor::block_on(storage.submit_intent(&Intent {
        id: CoordId::resolve("i001"),
        from_facts: vec![CoordId::resolve("f_base")],
        description: "test intent".into(),
        creator: "bob".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }))
    .unwrap();

    futures_executor::block_on(storage.claim_intent("i001", "alice")).unwrap();
    futures_executor::block_on(storage.heartbeat("i001", "alice")).unwrap();
    let result = futures_executor::block_on(storage.conclude_intent("i001", "done")).unwrap();

    assert_eq!(
        result.id.to_string().chars().count(),
        6,
        "CoordId should be 6 Hangul characters"
    );
    let state = futures_executor::block_on(storage.read_state());
    assert_eq!(state.facts.len(), 2);
    assert_eq!(state.intents.len(), 1);
    assert!(state.intents[0].concluded_at.is_some());
}

// ── FihSession hydrate/flush test ─────────────────────────────────────────

#[test]
fn test_session_hydrate_flush() {
    use nex_fih::{AsyncFactCapable, AsyncStorageRead, Content, CoordId, Fact};
    use nexus_storage_sim::FihSession;

    let io = SimIo::new();
    let mut session = FihSession::new(io.clone(), "test");

    // Write a fact via storage
    let fact = Fact::with_id(
        CoordId::resolve("f001"),
        "test".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"hello".to_vec(),
        },
        "alice".into(),
    );
    futures_executor::block_on(session.storage.submit_fact(&fact)).unwrap();

    // Not yet flushed → data is in buffer, not in IO
    assert!(!session.is_flushed());

    // Flush → data goes to IO
    session.flush().unwrap();
    assert!(session.is_flushed());

    // Read back from a fresh session on the same IO instance
    let mut session2 = FihSession::new(io, "test");
    session2.hydrate().unwrap();
    let state = futures_executor::block_on(session2.storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(
        state.facts[0].id.to_string(),
        CoordId::resolve("f001").to_string()
    );
}
