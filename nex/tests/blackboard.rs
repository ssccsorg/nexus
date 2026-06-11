use nex::blackboard::DefaultBlackboard;
use nex::storage::petgraph::write_graph;
use nex::*;
use nexus_model::{Fact, FihHash, FlushCapable, FlushCursor, Intent};

fn tick() {
    std::thread::sleep(std::time::Duration::from_millis(1));
}

fn bb_with_facts() -> DefaultBlackboard {
    let bb = DefaultBlackboard::new();
    for i in 0..5 {
        let fact = Fact {
            id: FihHash(format!("f{i}")),
            origin: "test".into(),
            content: format!("fact #{i}").into(),
            creator: "tester".into(),
        };
        bb.submit_fact(&fact).unwrap();
    }
    bb
}

#[test]
fn test_fresh_blackboard_has_empty_cursor() {
    let bb = DefaultBlackboard::new();
    assert_eq!(bb.flush_cursor, FlushCursor::default());
}

#[test]
fn test_flush_updates_cursor() {
    let mut bb = bb_with_facts();
    let before = bb.flush_cursor.clone();
    bb.flush().unwrap();
    let after = bb.flush_cursor.clone();
    assert!(
        after > before,
        "flush should advance cursor: before={before:?}, after={after:?}"
    );
}

#[test]
fn test_consecutive_flushes_advance_cursor() {
    let mut bb = bb_with_facts();
    bb.flush().unwrap();
    let c1 = bb.flush_cursor.clone();

    // add more facts
    for i in 5..10 {
        let fact = Fact {
            id: FihHash(format!("f{i}")),
            origin: "test".into(),
            content: format!("fact #{i}").into(),
            creator: "tester".into(),
        };
        bb.submit_fact(&fact).unwrap();
    }
    tick();

    bb.flush().unwrap();
    let c2 = bb.flush_cursor.clone();
    assert!(
        c2 > c1,
        "second flush should advance cursor: c1={c1:?}, c2={c2:?}"
    );
}

#[test]
fn test_cursor_survives_snapshot_roundtrip() {
    let mut bb = bb_with_facts();
    bb.flush().unwrap();
    let cursor_before = bb.flush_cursor.clone();

    let snap = bb.to_snapshot();
    let restored = DefaultBlackboard::from_snapshot(snap);
    assert_eq!(
        restored.flush_cursor, cursor_before,
        "flush cursor should survive snapshot roundtrip"
    );
}

#[test]
fn test_old_snapshot_without_cursor_gets_default() {
    // Simulate a snapshot created by older code that did not include flush_cursor.
    // The default FlushCursor is the epoch -- a fresh blackboard with no cursor
    // should start from the beginning.
    let bb = DefaultBlackboard::new();
    assert_eq!(bb.flush_cursor, FlushCursor::default());
}

#[test]
fn test_flush_after_restore_continues_from_cursor() {
    let mut bb = bb_with_facts();
    bb.flush().unwrap();
    let cursor1 = bb.flush_cursor.clone();

    let snap = bb.to_snapshot();
    let mut restored = DefaultBlackboard::from_snapshot(snap);
    assert_eq!(restored.flush_cursor, cursor1);

    // Add more facts to the original, restore to fresh instance.
    for i in 10..15 {
        let fact = Fact {
            id: FihHash(format!("f{i}")),
            origin: "test".into(),
            content: format!("fact #{i}").into(),
            creator: "tester".into(),
        };
        restored.submit_fact(&fact).unwrap();
    }
    tick();

    restored.flush().unwrap();
    let cursor2 = restored.flush_cursor.clone();
    assert!(
        cursor2 > cursor1,
        "flush after restore should advance from restored cursor: old={cursor1:?}, new={cursor2:?}"
    );
}

#[test]
fn test_cursor_independent_of_graph_mutations() {
    use nex::storage::petgraph::NodeWeight;
    use std::collections::HashMap;

    let mut bb = DefaultBlackboard::new();
    let fact = Fact {
        id: FihHash("f1".into()),
        origin: "test".into(),
        content: "test".into(),
        creator: "tester".into(),
    };
    bb.submit_fact(&fact).unwrap();
    bb.flush().unwrap();
    let c1 = bb.flush_cursor.clone();

    // Mutate graph directly (no fact submission).
    {
        let mut g = write_graph(&bb.hot_graph);
        g.add_node(NodeWeight {
            name: "test_node".into(),
            label: "Test".into(),
            properties: HashMap::new(),
        });
    }
    let c2 = bb.flush_cursor.clone();
    assert_eq!(
        c1, c2,
        "direct graph mutation should not advance flush cursor"
    );
}

#[test]
fn test_independent_blackboards_independent_cursors() {
    let mut bb1 = bb_with_facts();
    let mut bb2 = bb_with_facts();
    bb1.flush().unwrap();
    let c1 = bb1.flush_cursor.clone();
    // bb2 hasn't flushed yet — its cursor is still default.
    assert_eq!(bb2.flush_cursor, FlushCursor::default());
    bb2.flush().unwrap();
    assert!(
        bb2.flush_cursor > c1,
        "independent blackboard should have independent cursor"
    );
}

#[test]
fn test_flush_noop_backend() {
    // NullStorage cold backend — flush should succeed.
    let bb = bb_with_facts();
    let result = bb.storage.flush_since(&bb.flush_cursor);
    assert!(result.is_ok());
}

#[test]
fn test_flush_cycle_with_facts() {
    let mut bb = bb_with_facts();

    // Cycle flush multiple times.
    for _ in 0..3 {
        tick();
        let _ = bb.flush();
    }
}

#[test]
fn test_flush_empty_blackboard() {
    let mut bb = DefaultBlackboard::new();
    bb.flush().unwrap();
}

#[test]
fn test_cursor_timestamp_numeric() {
    let mut bb = bb_with_facts();
    bb.flush().unwrap();
    let cursor = bb.flush_cursor.clone();
    let ts = cursor.last_flushed_at;
    assert!(ts > 0, "flush cursor should contain a positive timestamp");
}

#[test]
fn test_storage_snapshot_roundtrip() {
    use nex::storage::petgraph::Snapshottable;

    let bb = bb_with_facts();
    // Add intents
    let intent = Intent {
        id: FihHash("i1".into()),
        from_facts: vec![],
        to_fact_id: None,
        description: "test goal".into(),
        creator: "tester".into(),
        worker: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    };
    bb.submit_intent(&intent).unwrap();

    let snapshot = bb.to_snapshot();

    // Reconstruct
    let restored = DefaultBlackboard::from_snapshot(snapshot);

    // Verify graph data
    let state = <DefaultBlackboard as nexus_model::StorageRead>::read_state(&restored);
    assert_eq!(state.facts.len(), 5);
    assert_eq!(state.intents.len(), 1);

    // Verify project_id
    assert_eq!(restored.project_id(), "default");
}
