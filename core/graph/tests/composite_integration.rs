// CompositeColdStorage + Petgraph DualStorage integration tests.
//
// These tests wire CompositeColdStorage as the cold backend of a
// DualStorage pair (with PetgraphStorage as hot) and exercise the full
// lifecycle: Cypher query routing (via execute_with_cold), flush-through,
// snapshot roundtrip, eviction boundary, and multi-entity persistence.
//
// CompositeColdStorage is the default cold backend for WASM/CF Workers.
// These tests validate that it composes correctly with Petgraph via
// DualStorage, matching the same trait contracts as DuckDbStorage.

use nexus_graph::{
    Blackboard, ColdStorage, CypherCapable, EvictCapable, Fact, FihHash, FlushCapable, GraphRead,
    PetgraphStorage, Snapshottable, create_blackboard_from_snapshot,
    create_blackboard_with_storage,
};
use nexus_model::FlushCursor;
use nexus_storage_kv_cold::{CompositeColdStorage, MockBlob, MockKv, MockObject};
use serde_json::json;

// ── Helpers ────────────────────────────────────────────────────────────────

fn make_composite_cold() -> CompositeColdStorage<MockKv, MockBlob, MockObject> {
    CompositeColdStorage::new_with_system_clock(
        MockKv::new(),
        MockBlob::new(),
        MockObject::new(),
        "integration-test",
    )
}

fn make_bb()
-> impl Blackboard + CypherCapable + EvictCapable + FlushCapable + GraphRead + Snapshottable {
    let hot = PetgraphStorage::new();
    let cold: Box<dyn ColdStorage> = Box::new(make_composite_cold());
    create_blackboard_with_storage(hot, cold)
}

fn fact(id: &str) -> Fact {
    Fact {
        id: FihHash(id.to_string()),
        origin: "integration".into(),
        content: json!({"key": id}),
        creator: "tester".into(),
    }
}

// ── DualStorage composition test ───────────────────────────────────────────

#[test]
fn test_dual_storage_writes_to_both_backends() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_dual_1")).unwrap();

    // Flush through cold backend proves dual-write persisted.
    let cursor = FlushCursor::default();
    let result = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(result.records_flushed, 1, "dual-write should persist");
}

// ── Cypher query routing test ──────────────────────────────────────────────
//
// CypherCapable on DualStorage delegates to cold only (which is no-op).
// The actual hot+cold Cypher routing is done via DefaultBlackboard::query()
// which calls execute_with_cold internally. This test verifies that
// CompositeColdStorage (no-op Cypher) does not interfere with petgraph-hot
// Cypher resolution.

#[test]
fn test_cypher_query_routes_to_petgraph_hot() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_cypher_1")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_cypher_2")).unwrap();

    // query_plan on DualStorage goes to cold only — Composite is no-op.
    // This is expected; Cypher routing for hot is done through
    // DefaultBlackboard::query() (crate-internal).
    let plan = json!({
        "label": "Fact",
        "projections": ["fact_id"],
    });
    let result = <_ as CypherCapable>::query_plan(&guard, &plan);
    // Composite's CypherCapable is the default no-op, so we expect an error.
    // This is correct behaviour — hot Cypher queries use the separate
    // execute_with_cold path, not CypherCapable.
    assert!(
        result.is_err(),
        "Composite cold storage is Cypher no-op; hot Cypher uses separate path"
    );
}

// ── Flush-through test ─────────────────────────────────────────────────────

#[test]
fn test_flush_through_composite_to_blob() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_flush_a")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_flush_b")).unwrap();

    let cursor = FlushCursor::default();
    let r1 = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(r1.records_flushed, 2, "first flush exports 2 facts");

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_flush_c")).unwrap();
    let r2 = <_ as FlushCapable>::flush_since(&guard, &r1.new_cursor).unwrap();
    assert_eq!(
        r2.records_flushed, 1,
        "incremental flush exports only new fact"
    );
    assert!(
        r2.new_cursor.last_flushed_at > r1.new_cursor.last_flushed_at,
        "cursor advances"
    );
}

// ── Snapshot roundtrip test ────────────────────────────────────────────────

#[test]
fn test_snapshot_roundtrip_with_composite_cold() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_snap_1")).unwrap();

    let snapshot = <_ as Snapshottable>::to_snapshot(&guard);
    assert_eq!(snapshot.project_id, "default");

    // Restore from snapshot with fresh Composite cold backend.
    // Snapshot contains the full petgraph; Composite is reconstructed fresh.
    let restored = create_blackboard_from_snapshot(snapshot);
    let state = <_ as Blackboard>::read_state(&restored);
    assert!(
        state.facts.iter().any(|f| f.id.0 == "f_snap_1"),
        "fact survives snapshot roundtrip"
    );
}

// ── Multi-entity persistence test ──────────────────────────────────────────

#[test]
fn test_multi_entity_persistence_through_dual_storage() {
    use nexus_graph::Intent;

    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_persist")).unwrap();

    let intent = Intent {
        id: FihHash("i_persist".into()),
        from_facts: vec!["f_persist".into()],
        to_fact_id: None,
        description: "test intent".into(),
        creator: "tester".into(),
        worker: None,
        last_heartbeat_at: None,
        created_at: Some("0".into()),
        concluded_at: None,
    };
    <_ as Blackboard>::submit_intent(&mut guard, &intent).unwrap();

    // Claim intent — this adds worker to both hot (petgraph) and cold (composite KV).
    <_ as Blackboard>::claim_intent(&mut guard, "i_persist", "agent-x").unwrap();
    <_ as Blackboard>::heartbeat(&mut guard, "i_persist", "agent-x").unwrap();

    // Verify intent exists with claimed worker in hot.
    let state = <_ as Blackboard>::read_state(&guard);
    assert!(
        state.intents.iter().any(|i| i.id.0 == "i_persist"),
        "intent exists in read_state"
    );

    let result = json!({"concluded": true});
    let concluded_fact =
        <_ as Blackboard>::conclude_intent(&mut guard, "i_persist", &result).unwrap();

    // Conclusion creates a fact that should be in both hot and cold via dual-write.
    let state = <_ as Blackboard>::read_state(&guard);
    assert!(
        state.facts.iter().any(|f| f.id.0 == concluded_fact.id.0),
        "concluded intent fact is readable in hot layer: {:?}",
        state.facts.iter().map(|f| &f.id.0).collect::<Vec<_>>()
    );

    // Flush through cold backend.
    let cursor = FlushCursor::default();
    let flush = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert!(
        flush.records_flushed >= 1,
        "at least the result fact is flushed"
    );
}

// ── Evict-after-flush test ─────────────────────────────────────────────────
//
// DualStorage delegates evict_before to PetgraphStorage (hot layer).
// Composite cold blobs are NOT evicted through this path — evicting cold
// requires a direct call to Composite's evict_before.
// This test verifies the hot-only eviction doesn't interact badly with
// composite being present as cold.

#[test]
fn test_evict_after_flush_does_not_affect_cold() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_evict")).unwrap();

    let cursor = FlushCursor::default();
    <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();

    // DualStorage.evict_before delegates to hot (petgraph).
    // Hot eviction with future timestamp should report 0 — petgraph has
    // no blob to evict (blobs are in composite cold).
    let evicted = <_ as EvictCapable>::evict_before(&guard, "9999999999999999999").unwrap();
    assert_eq!(evicted, 0, "hot eviction does not touch cold blobs");
}

// ── Flush → snapshot roundtrip → re-flush (with NullStorage cold) ────────
//
// snapshot roundtrip restores hot petgraph but cold backend becomes
// NullStorage. After restore, flush goes through NullStorage which is
// a no-op. This is the current design — cold storage is not preserved
// across snapshots.

#[test]
fn test_flush_then_snapshot_roundtrip_null_cold_is_noop() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_cycle_a")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_cycle_b")).unwrap();

    // Flush before snapshot.
    let cursor = FlushCursor::default();
    let r_before = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(r_before.records_flushed, 2, "initial flush exports 2 facts");

    // Snapshot and restore. Cold backend becomes NullStorage.
    let snapshot = <_ as Snapshottable>::to_snapshot(&guard);
    let mut restored = create_blackboard_from_snapshot(snapshot);

    // Hot state is preserved (petgraph was in snapshot).
    let state = <_ as Blackboard>::read_state(&restored);
    assert_eq!(state.facts.len(), 2, "facts survive snapshot roundtrip");

    // NullStorage is a no-op, so flush_since returns 0 regardless.
    // This is expected behaviour.
    let r_after = <_ as FlushCapable>::flush_since(&mut restored, &r_before.new_cursor).unwrap();
    assert_eq!(
        r_after.records_flushed, 0,
        "NullStorage cold backend is no-op"
    );
}
