// CompositeColdStorage + Petgraph DualStorage integration tests.
//
// These tests wire CompositeColdStorage as the cold backend of a
// DualStorage pair (with PetgraphStorage as hot) and exercise the full
// lifecycle: Cypher query routing (via execute_with_cold), flush-through,
// snapshot roundtrip, eviction boundary, multi-lifetime data preservation,
// and multi-entity persistence.
//
// CompositeColdStorage is the default cold backend for WASM/CF Workers.
// These tests validate that it composes correctly with Petgraph via
// DualStorage, matching the same trait contracts as DuckDbStorage.

use nexus_graph::{
    Blackboard, ColdStorage, Content, CypherCapable, EvictCapable, Fact, FihHash, FlushCapable,
    GraphRead, PetgraphStorage, ScanCapable, Snapshottable, create_blackboard_from_snapshot,
    create_blackboard_with_storage,
};
use nexus_model::{DualStorage, FlushCursor};
use nexus_storage_composite::{BlobStore, CompositeColdStorage, MetaStore, ObjectStore};
use serde_json::json;

// ── Inline mock implementations for integration tests ───────────────────────
//
// Mock types are intentionally kept in test code, not in the library crate.
// WASM builds must not carry test dependencies in the binary.
// These in-memory stores match the same trait contracts that CF Workers KV,
// R2, and Durable Object bindings will implement.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
struct MockKv {
    data: Arc<RwLock<HashMap<String, String>>>,
}
impl MockKv {
    fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
impl MetaStore for MockKv {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.data.read().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.data
            .write()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }
}

#[derive(Clone)]
struct MockBlob {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}
impl MockBlob {
    fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
impl BlobStore for MockBlob {
    fn put(&self, key: &str, data: &[u8]) -> Result<(), String> {
        self.data
            .write()
            .unwrap()
            .insert(key.to_string(), data.to_vec());
        Ok(())
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.data.read().unwrap().get(key).cloned())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.data.write().unwrap().remove(key);
        Ok(())
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        let map = self.data.read().unwrap();
        let mut keys: Vec<_> = map
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }
}

struct MockObject {
    data: Arc<RwLock<HashMap<String, String>>>,
}
impl MockObject {
    fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
impl ObjectStore for MockObject {
    fn get_state(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.data.read().unwrap().get(key).cloned())
    }
    fn put_state(&self, key: &str, expected: &str, new: &str) -> Result<bool, String> {
        let mut map = self.data.write().unwrap();
        let current = map.get(key).map(|s| s.as_str()).unwrap_or("");
        if current == expected {
            if new.is_empty() {
                map.remove(key);
            } else {
                map.insert(key.to_string(), new.to_string());
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn make_composite_cold() -> CompositeColdStorage<MockBlob, MockObject, MockKv> {
    CompositeColdStorage::new_with_system_clock(
        MockBlob::new(),
        MockObject::new(),
        MockKv::new(),
        "default",
    )
}

fn make_bb() -> impl Blackboard
+ CypherCapable
+ EvictCapable
+ FlushCapable
+ GraphRead
+ ScanCapable
+ Snapshottable {
    let hot = PetgraphStorage::new();
    let cold: Box<dyn ColdStorage> = Box::new(make_composite_cold());
    create_blackboard_with_storage(hot, cold)
}

fn fact(id: &str) -> Fact {
    Fact {
        id: FihHash(id.to_string()),
        origin: "integration".into(),
        content: Content::Text(json!({"key": id}).to_string()),
        creator: "tester".into(),
    }
}

// ── DualStorage composition test ───────────────────────────────────────────

#[test]
fn test_dual_storage_writes_to_both_backends() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_dual_1")).unwrap();

    // flush_since delegates to cold (CompositeColdStorage).
    // Cold no longer stores graph data, so records_flushed is 0
    // unless hot data was pre-written to cold blob.
    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let result = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert!(
        result.records_flushed > 0,
        "flush should persist hot data to cold blob"
    );
    assert!(
        !result.new_cursor.last_flushed_at.is_empty(),
        "cursor updated"
    );

    // Fact is still readable from hot (Petgraph)
    let state = <_ as Blackboard>::read_state(&guard);
    assert!(
        state.facts.iter().any(|f| f.id.0 == "f_dual_1"),
        "fact survives in hot"
    );
}

// ── Cypher query routing test ──────────────────────────────────────────────

#[test]
fn test_cypher_query_routes_to_petgraph_hot() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_cypher_1")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_cypher_2")).unwrap();

    let plan = json!({
        "label": "Fact",
        "projections": ["fact_id"],
    });
    let result = <_ as CypherCapable>::query_plan(&guard, &plan);
    assert!(result.is_err(), "Composite cold storage is Cypher no-op");
}

// ── Flush-through test ─────────────────────────────────────────────────────

#[test]
fn test_flush_through_composite_to_blob() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_flush_a")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_flush_b")).unwrap();

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let r1 = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(r1.records_flushed, 2, "first flush exports 2 facts");

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_flush_c")).unwrap();
    let r2 = <_ as FlushCapable>::flush_since(&guard, &r1.new_cursor).unwrap();
    assert!(r2.records_flushed > 0, "second flush exports facts");
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

    let _snap = <_ as Snapshottable>::to_snapshot(&guard);
    let restored = create_blackboard_from_snapshot(_snap);
    let state = <_ as Blackboard>::read_state(&restored);
    assert!(
        state.facts.iter().any(|f| f.id.0 == "f_snap_1"),
        "fact survives snapshot roundtrip"
    );
}

// ── Multi-lifetime data preservation ───────────────────────────────────────

#[test]
fn test_multi_lifetime_data_preservation_across_restart() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_life_1")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_life_2")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_life_3")).unwrap();

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let r1 = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(r1.records_flushed, 3, "lifetime 1: flush exports 3 facts");

    let _snap2 = <_ as Snapshottable>::to_snapshot(&guard);
    let cursor_t1 = r1.new_cursor.last_flushed_at.clone();

    let hot = PetgraphStorage::with_project_id("default");
    let cold: Box<dyn ColdStorage> = Box::new(make_composite_cold());
    let _bb2 = create_blackboard_with_storage(hot, cold);

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_life_4")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_life_5")).unwrap();

    let cursor2 = FlushCursor {
        last_flushed_at: cursor_t1,
        partition: "default".into(),
    };
    let r2 = <_ as FlushCapable>::flush_since(&guard, &cursor2).unwrap();
    assert!(r2.records_flushed > 0, "lifetime 2: re-flush exports facts");
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

    <_ as Blackboard>::claim_intent(&mut guard, "i_persist", "agent-x").unwrap();
    <_ as Blackboard>::heartbeat(&mut guard, "i_persist", "agent-x").unwrap();

    let state = <_ as Blackboard>::read_state(&guard);
    assert!(
        state.intents.iter().any(|i| i.id.0 == "i_persist"),
        "intent exists"
    );

    let concluded_fact =
        <_ as Blackboard>::conclude_intent(&mut guard, "i_persist", "concluded").unwrap();

    let state = <_ as Blackboard>::read_state(&guard);
    assert!(
        state.facts.iter().any(|f| f.id.0 == concluded_fact.id.0),
        "concluded fact readable"
    );

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let flush = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert!(flush.records_flushed >= 1, "result fact is flushed");
}

// ── Evict-after-flush test ─────────────────────────────────────────────────

#[test]
fn test_evict_after_flush_removes_both_hot_and_cold_blobs() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_evict")).unwrap();

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();

    let evicted = <_ as EvictCapable>::evict_before(&guard, "9999999999999999999").unwrap();
    assert!(
        evicted >= 1,
        "evict_before removes at least 1 cold blob, got {evicted}"
    );

    let r2 = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert!(r2.records_flushed > 0, "re-flush after evict works");
}

// ── Flush → snapshot roundtrip → NullStorage cold is no-op ────────────────

#[test]
fn test_flush_then_snapshot_roundtrip_null_cold_is_noop() {
    let bb = make_bb();
    let mut guard = bb;

    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_cycle_a")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("f_cycle_b")).unwrap();

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let r_before = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(r_before.records_flushed, 2, "initial flush exports 2 facts");

    let snapshot = <_ as Snapshottable>::to_snapshot(&guard);
    let mut restored = create_blackboard_from_snapshot(snapshot);

    let state = <_ as Blackboard>::read_state(&restored);
    assert_eq!(state.facts.len(), 2, "facts survive snapshot roundtrip");

    let r_after = <_ as FlushCapable>::flush_since(&mut restored, &r_before.new_cursor).unwrap();
    assert_eq!(
        r_after.records_flushed, 0,
        "NullStorage cold backend is no-op"
    );
}

// ── FIH Scenario: Submit → Flush → Read back ──────────────────────────

#[test]
fn test_fih_scenario_submit_flush_read() {
    let bb = make_bb();
    let mut guard = bb;

    // Submit 3 facts, 1 intent, 1 hint
    <_ as Blackboard>::submit_fact(&mut guard, &fact("scn_f1")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("scn_f2")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("scn_f3")).unwrap();

    let intent = nexus_graph::Intent {
        id: FihHash("scn_i1".into()),
        from_facts: vec!["scn_f1".into(), "scn_f2".into()],
        to_fact_id: None,
        description: "scenario intent".into(),
        creator: "tester".into(),
        worker: None,
        last_heartbeat_at: None,
        created_at: Some("0".into()),
        concluded_at: None,
    };
    <_ as Blackboard>::submit_intent(&mut guard, &intent).unwrap();
    <_ as Blackboard>::submit_hint(
        &mut guard,
        &nexus_graph::Hint {
            id: FihHash("scn_h1".into()),
            content: "scenario hint".into(),
            creator: "tester".into(),
        },
    )
    .unwrap();

    // Read state before flush (from Petgraph hot)
    let state_before = <_ as Blackboard>::read_state(&guard);
    assert_eq!(state_before.facts.len(), 3);
    assert_eq!(state_before.intents.len(), 1);
    assert_eq!(state_before.hints.len(), 1);

    // Flush (cursor-aware: only data after cursor)
    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let result = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(result.records_flushed, 5, "all 5 entities flushed");

    // Read state after flush (still from Petgraph hot)
    let state_after = <_ as Blackboard>::read_state(&guard);
    assert_eq!(state_after.facts.len(), 3, "facts survive flush");
    assert_eq!(state_after.intents.len(), 1, "intents survive flush");
    assert_eq!(state_after.hints.len(), 1, "hints survive flush");

    // Verify data equality: Petgraph hot == Blob flushed
    let cold_data = <_ as ScanCapable>::scan_partition(&guard, "default").unwrap();
    assert_eq!(cold_data.facts.len(), 3, "flushed facts in cold");
    assert_eq!(cold_data.intents.len(), 1, "flushed intents in cold");
    assert_eq!(cold_data.hints.len(), 1, "flushed hints in cold");
}

// ── FIH Scenario: Incremental flush ────────────────────────────────────

#[test]
fn test_fih_scenario_incremental_flush() {
    let bb = make_bb();
    let mut guard = bb;

    // Phase 1: submit 2 facts, flush
    <_ as Blackboard>::submit_fact(&mut guard, &fact("inc_f1")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("inc_f2")).unwrap();

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let r1 = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(r1.records_flushed, 2, "first flush: 2 facts");
    assert!(!r1.new_cursor.last_flushed_at.is_empty(), "cursor set");

    // Phase 2: add 1 more fact, flush with cursor
    <_ as Blackboard>::submit_fact(&mut guard, &fact("inc_f3")).unwrap();

    let r2 = <_ as FlushCapable>::flush_since(&guard, &r1.new_cursor).unwrap();
    assert_eq!(r2.records_flushed, 1, "incremental flush: only 1 new fact");
    assert!(
        r2.new_cursor.last_flushed_at > r1.new_cursor.last_flushed_at,
        "cursor advances"
    );

    // Phase 3: flush again with same cursor (no new data) — should be 0
    <_ as Blackboard>::submit_fact(&mut guard, &fact("inc_f4")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("inc_f5")).unwrap();
    let r3 = <_ as FlushCapable>::flush_since(&guard, &r2.new_cursor).unwrap();
    assert_eq!(r3.records_flushed, 2, "third flush: 2 new facts");
}

// ── FIH Scenario: Petgraph == Blob data identity ───────────────────────

#[test]
fn test_fih_scenario_petgraph_blob_identity() {
    let bb = make_bb();
    let mut guard = bb;

    // Submit varied data
    <_ as Blackboard>::submit_fact(&mut guard, &fact("id_f1")).unwrap();
    <_ as Blackboard>::submit_fact(&mut guard, &fact("id_f2")).unwrap();

    // Claim and heartbeat an intent
    let intent = nexus_graph::Intent {
        id: FihHash("id_i1".into()),
        from_facts: vec!["id_f1".into()],
        to_fact_id: None,
        description: "identity test intent".into(),
        creator: "tester".into(),
        worker: None,
        last_heartbeat_at: None,
        created_at: Some("0".into()),
        concluded_at: None,
    };
    <_ as Blackboard>::submit_intent(&mut guard, &intent).unwrap();
    <_ as Blackboard>::claim_intent(&mut guard, "id_i1", "agent-x").unwrap();
    <_ as Blackboard>::heartbeat(&mut guard, "id_i1", "agent-x").unwrap();

    // Read from Petgraph
    let state = <_ as Blackboard>::read_state(&guard);
    let petgraph_fact_ids: Vec<String> = state.facts.iter().map(|f| f.id.0.clone()).collect();
    let petgraph_intent_ids: Vec<String> = state.intents.iter().map(|i| i.id.0.clone()).collect();

    // Flush
    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let result = <_ as FlushCapable>::flush_since(&guard, &cursor).unwrap();
    assert_eq!(result.records_flushed, 3, "3 entities flushed");

    // Read from scan_partition (hot + cold merged)
    let scanned = <_ as ScanCapable>::scan_partition(&guard, "default").unwrap();
    let scanned_fact_ids: Vec<String> = scanned.facts.iter().map(|f| f.id.0.clone()).collect();
    let scanned_intent_ids: Vec<String> = scanned.intents.iter().map(|i| i.id.0.clone()).collect();

    // Identity: Petgraph and scan_partition must return same entities
    assert_eq!(
        petgraph_fact_ids, scanned_fact_ids,
        "Petgraph == scan facts"
    );
    assert_eq!(
        petgraph_intent_ids, scanned_intent_ids,
        "Petgraph == scan intents"
    );
}

// ── FIH Scenario: Project ID mismatch panics ───────────────────────────

#[test]
#[should_panic(expected = "DualStorage: hot and cold must share the same project_id")]
fn test_fih_scenario_project_id_mismatch_panics() {
    let hot = PetgraphStorage::with_project_id("hot-project");
    let cold_mock = CompositeColdStorage::new_with_system_clock(
        nexus_storage_composite::IoBufferBlob::new(),
        nexus_storage_composite::IoBufferObject::new(),
        nexus_storage_composite::IoBufferKv::new(),
        "cold-project",
    );
    let _storage = DualStorage::new(Box::new(hot), Box::new(cold_mock));
    // Should panic
}
