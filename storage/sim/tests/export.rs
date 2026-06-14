// Integration tests for export/import bundle functionality.
//
// These tests verify the full FIH bundle export and import cycle:
// encoding, decoding, corrupt/malformed input detection, version
// handling, and end-to-end round trips with various storage backends.

use nexus_storage_sim::export::{export_from_io, import_into_io};
use nexus_storage_sim::intent_status::IntentStatus;
use nexus_storage_sim::io::SyncFileIo;
use nexus_storage_sim::record::{
    ContentMeta, FactRecord, HintRecord, IntentRecord,
};
use nexus_storage_sim::sim_io::SimIo;

// ── Helper: seed a SimIo with sample records ─────────────────────────────

fn seeded_io() -> SimIo {
    let io = SimIo::new();
    let sync = SyncFileIo::new(io.clone());

    let fact = FactRecord {
        id: "f001".into(),
        blob_hash: "deadbeef".into(),
        origin: "test".into(),
        creator: "alice".into(),
        submitted_at: 1000,
    };
    sync
        .write(&fact.key(), &postcard::to_allocvec(&fact).unwrap())
        .unwrap();

    let intent = IntentRecord {
        id: "i001".into(),
        from_facts: vec!["f001".into()],
        description_hash: String::new(),
        creator: "bob".into(),
        status: IntentStatus::Submitted,
        created_at: 1001,
    };
    sync
        .write(&intent.key(), &postcard::to_allocvec(&intent).unwrap())
        .unwrap();

    let hint = HintRecord {
        id: "h001".into(),
        content: "test hint".into(),
        creator: "tester".into(),
        submitted_at: 1002,
        ttl_secs: None,
    };
    sync
        .write(&hint.key(), &postcard::to_allocvec(&hint).unwrap())
        .unwrap();

    sync.write("blob/deadbeef.bin", b"hello world").unwrap();
    let meta = ContentMeta {
        mime_type: "text/plain".into(),
        size: 11,
    };
    sync
        .write(
            "blob/deadbeef.bin.meta",
            &postcard::to_allocvec(&meta).unwrap(),
        )
        .unwrap();

    io
}

// ── Round-trip test ──────────────────────────────────────────────────────

#[test]
fn test_export_round_trip() {
    let io = seeded_io();
    let sync = SyncFileIo::new(io);
    let bundle = export_from_io(&sync).unwrap();
    assert!(!bundle.is_empty());

    let dst = SimIo::new();
    let dst_sync = SyncFileIo::new(dst);
    import_into_io(&dst_sync, &bundle).unwrap();

    assert!(dst_sync.read("facts/f_f001.fact").unwrap().is_some());
    assert!(dst_sync.read("intents/i_i001.intent").unwrap().is_some());
    assert!(dst_sync.read("hints/h_h001.hint").unwrap().is_some());
    assert!(dst_sync.read("blob/deadbeef.bin").unwrap().is_some());
    assert!(dst_sync
        .read("blob/deadbeef.bin.meta")
        .unwrap()
        .is_some());
}

// ── Empty IO test ────────────────────────────────────────────────────────

#[test]
fn test_export_empty_io() {
    let io = SimIo::new();
    let sync = SyncFileIo::new(io);
    let bundle = export_from_io(&sync).unwrap();
    let dst = SimIo::new();
    let dst_sync = SyncFileIo::new(dst);
    import_into_io(&dst_sync, &bundle).unwrap();
    assert!(dst_sync.list("facts/").unwrap().is_empty());
}

// ── Invalid magic rejected ───────────────────────────────────────────────

#[test]
fn test_invalid_magic_rejected() {
    let io = SimIo::new();
    let sync = SyncFileIo::new(io);
    let result = import_into_io(&sync, b"NOTABUNDL");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("bad magic"));
}

// ── Filesystem to Sim round-trip ─────────────────────────────────────────

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn test_export_import_fs_to_sim() {
    let dir =
        std::env::temp_dir().join(format!("fih_export_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let fs_io = nexus_storage_sim::fs_io::FsIo::new(&dir).unwrap();
    let sync = SyncFileIo::new(fs_io);

    let fact = FactRecord {
        id: "f_fs".into(),
        blob_hash: "cafe01".into(),
        origin: "fs_test".into(),
        creator: "fs_user".into(),
        submitted_at: 2000,
    };
    sync
        .write(&fact.key(), &postcard::to_allocvec(&fact).unwrap())
        .unwrap();

    let bundle = export_from_io(&sync).unwrap();

    let sim_io = SimIo::new();
    let sim_sync = SyncFileIo::new(sim_io);
    import_into_io(&sim_sync, &bundle).unwrap();

    let loaded: FactRecord =
        postcard::from_bytes(&sim_sync.read("facts/f_f_fs.fact").unwrap().unwrap())
            .unwrap();
    assert_eq!(loaded.id, "f_fs");
    assert_eq!(loaded.blob_hash, "cafe01");

    let _ = std::fs::remove_dir_all(dir);
}

// ── Scenario: full FIH lifecycle via export/import ───────────────────────

#[test]
fn test_export_full_fih_lifecycle() {
    use nex::StorageRead;
    use nexus_storage_sim::FihStorage;

    // Seed a SimIo directly with records (not through FihStorage).
    let io = SimIo::new();
    let sync = SyncFileIo::new(io.clone());

    let fa = FactRecord {
        id: "f_a".into(),
        blob_hash: String::new(),
        origin: "lifecycle".into(),
        creator: "tester".into(),
        submitted_at: 100,
    };
    sync
        .write(&fa.key(), &postcard::to_allocvec(&fa).unwrap())
        .unwrap();

    let fb = FactRecord {
        id: "f_b".into(),
        blob_hash: String::new(),
        origin: "lifecycle".into(),
        creator: "tester".into(),
        submitted_at: 101,
    };
    sync
        .write(&fb.key(), &postcard::to_allocvec(&fb).unwrap())
        .unwrap();

    let fc = FactRecord {
        id: "f_c".into(),
        blob_hash: String::new(),
        origin: "lifecycle".into(),
        creator: "tester".into(),
        submitted_at: 102,
    };
    sync
        .write(&fc.key(), &postcard::to_allocvec(&fc).unwrap())
        .unwrap();

    let intent1 = IntentRecord {
        id: "i_a".into(),
        from_facts: vec!["f_a".into(), "f_b".into()],
        description_hash: String::new(),
        creator: "tester".into(),
        status: IntentStatus::Submitted,
        created_at: 200,
    };
    sync
        .write(&intent1.key(), &postcard::to_allocvec(&intent1).unwrap())
        .unwrap();

    let intent2 = IntentRecord {
        id: "i_b".into(),
        from_facts: vec!["f_b".into(), "f_c".into()],
        description_hash: String::new(),
        creator: "tester".into(),
        status: IntentStatus::Submitted,
        created_at: 201,
    };
    sync
        .write(&intent2.key(), &postcard::to_allocvec(&intent2).unwrap())
        .unwrap();

    let hint1 = HintRecord {
        id: "h_a".into(),
        content: "lifecycle hint".into(),
        creator: "tester".into(),
        submitted_at: 300,
        ttl_secs: None,
    };
    sync
        .write(&hint1.key(), &postcard::to_allocvec(&hint1).unwrap())
        .unwrap();

    // Export from the seeded IO.
    let bundle = export_from_io(&sync).unwrap();

    // Import into a fresh SimIo.
    let fresh_io = SimIo::new();
    let fresh_sync = SyncFileIo::new(fresh_io.clone());
    import_into_io(&fresh_sync, &bundle).unwrap();

    // Rebuild a FihStorage on top of the fresh IO and verify.
    let storage = FihStorage::new(fresh_io, "lifecycle");
    storage.rebuild_cache().unwrap();

    let state = storage.read_state();
    assert_eq!(state.facts.len(), 3, "should have 3 facts");
    assert_eq!(state.intents.len(), 2, "should have 2 intents");
    assert_eq!(state.hints.len(), 1, "should have 1 hint");
}

// ── Scenario: corrupt bundle rejected ────────────────────────────────────

#[test]
fn test_export_corrupt_bundle_rejected() {
    let io = SimIo::new();
    let sync = SyncFileIo::new(io);

    // Random garbage bytes instead of a valid bundle.
    let garbage: Vec<u8> = (0..64).map(|i| (i * 17) as u8).collect();
    let result = import_into_io(&sync, &garbage);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("bad magic") || err.contains("invalid bundle"),
        "error should mention bad magic or invalid bundle, got: {err}"
    );
}

// ── Scenario: version mismatch rejected ──────────────────────────────────

/// Mirror of the crate-internal Bundle struct, used here only to produce
/// a postcard payload with an alternative version number.
#[derive(serde::Serialize, serde::Deserialize)]
struct TestBundle {
    version: u16,
    facts: Vec<FactRecord>,
    intents: Vec<IntentRecord>,
    hints: Vec<HintRecord>,
    blobs: Vec<TestBlobEntry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TestBlobEntry {
    hash: String,
    data: Vec<u8>,
    meta: ContentMeta,
}

#[test]
fn test_export_version_mismatch_rejected() {
    // Build a bundle with version 99 (unsupported — only version 1 is valid).
    let bad_bundle = {
        let bad_struct = TestBundle {
            version: 99,
            facts: vec![],
            intents: vec![],
            hints: vec![],
            blobs: vec![],
        };
        let mut buf = Vec::with_capacity(8 + 1024);
        buf.extend_from_slice(b"FIHBUNDL");
        let payload = postcard::to_allocvec(&bad_struct).unwrap();
        buf.extend_from_slice(&payload);
        buf
    };

    let io = SimIo::new();
    let sync = SyncFileIo::new(io);
    let result = import_into_io(&sync, &bad_bundle);

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("unsupported bundle version"),
        "error should mention unsupported version, got: {err}"
    );
    assert!(err.contains("99"), "error should mention version 99");
}
