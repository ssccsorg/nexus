// The host surface of the storage core: the portable bundle, the session lifecycle, and
// the synchronous blackboard.
//
// These are the paths a host uses around the async storage rather than through it. The
// bundle is how a state space leaves one volume and enters another, the session is how a
// caller holds a hydrate-to-flush window, and the blackboard is the sync face the process
// layer drives. Each is a small wrapper, and each is what a consumer depends on.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture, SyncFileIo};
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead, Content, ContentMeta,
    CoordId, EvictCapable, Fact, FactCapable, FihBlackboard, FihSession, FihStorage, Hint,
    HintCapable, Intent, StorageRead, export_from_io, import_into_io,
};

/// An in-memory medium shared by every storage that holds a clone of it.
#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl FileIo for MemoryIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move { Ok(map.lock().unwrap().get(path).cloned()) })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            map.lock().unwrap().insert(path.to_string(), data.to_vec());
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            let mut keys: Vec<String> = map
                .lock()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect();
            keys.sort();
            Ok(keys)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            map.lock().unwrap().remove(path);
            Ok(())
        })
    }
}

fn fact(payload: &str) -> Fact {
    Fact::new(
        "origin/source".into(),
        Content::from(payload),
        "agent".into(),
    )
}

/// A volume with one fact, one intent over it, and one hint.
fn seeded() -> (MemoryIo, Fact) {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "host");
    let fact = fact("the payload");
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
    block_on(storage.submit_intent(&Intent::new(
        CoordId::resolve("i_bundle"),
        vec![fact.id],
        None,
        "an intent over the fact".to_string(),
        "agent".to_string(),
    )))
    .expect("the intent is accepted");
    block_on(storage.submit_hint(&Hint {
        id: CoordId::resolve("h_bundle"),
        content: "a hint".to_string(),
        creator: "agent".to_string(),
    }))
    .expect("the hint is accepted");
    block_on(storage.flush_pending()).expect("the writes reach the medium");
    (medium, fact)
}

#[test]
fn a_bundle_carries_the_records_and_the_blobs_to_another_volume() {
    let (source, fact) = seeded();

    let bundle = export_from_io(&SyncFileIo::new(source)).expect("the volume exports");
    assert!(
        bundle.starts_with(b"FIHBUNDL"),
        "the bundle names its format"
    );

    let target = MemoryIo::default();
    import_into_io(&SyncFileIo::new(target.clone()), &bundle).expect("the bundle imports");

    let reader = FihStorage::new(target, "host");
    block_on(reader.rebuild_cache()).expect("the imported volume opens");
    let state = block_on(reader.read_state());

    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].id, fact.id);
    assert_eq!(
        state.facts[0].content.data, b"the payload",
        "a bundle carries the blob rather than a reference to it"
    );
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].description, "an intent over the fact");
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.hints[0].content, "a hint");
}

#[test]
fn a_bundle_that_is_not_this_format_is_refused() {
    let sync = SyncFileIo::new(MemoryIo::default());

    for bytes in [b"NOTAFIHB".to_vec(), b"FIH".to_vec()] {
        let error = import_into_io(&sync, &bytes).expect_err("a foreign bundle is refused");
        assert!(error.contains("bad magic"), "{error}");
    }
}

#[test]
fn a_bundle_from_a_later_version_is_refused() {
    let (source, _) = seeded();
    let mut bundle = export_from_io(&SyncFileIo::new(source)).expect("the volume exports");

    // postcard encodes the u16 version as a one-byte varint at the start of the payload.
    bundle[8] = 2;
    let error = import_into_io(&SyncFileIo::new(MemoryIo::default()), &bundle)
        .expect_err("a later version is refused");
    assert_eq!(error, "unsupported bundle version: 2");
}

#[test]
fn a_truncated_bundle_is_refused() {
    let (source, _) = seeded();
    let bundle = export_from_io(&SyncFileIo::new(source)).expect("the volume exports");

    let error = import_into_io(&SyncFileIo::new(MemoryIo::default()), &bundle[..8])
        .expect_err("a bundle with no payload is refused");
    assert!(error.starts_with("invalid bundle:"), "{error}");
}

#[test]
fn a_blob_without_metadata_exports_a_default_entry() {
    // A blob written by a caller rather than by `submit_fact` has no meta beside it. The
    // export promises an entry either way, so the import must still be able to read it.
    let source = MemoryIo::default();
    let sync = SyncFileIo::new(source);
    sync.write("blob/deadbeef.bin", b"raw!")
        .expect("the blob is written");

    let bundle = export_from_io(&sync).expect("the volume exports");
    let target = MemoryIo::default();
    let target_sync = SyncFileIo::new(target);
    import_into_io(&target_sync, &bundle).expect("the bundle imports");

    let meta_bytes = target_sync
        .read("blob/deadbeef.bin.meta")
        .expect("the metadata is readable")
        .expect("the import writes the metadata the export defaulted");
    let meta: ContentMeta = postcard::from_bytes(&meta_bytes).expect("the metadata parses");
    assert_eq!(meta.mime_type, "application/octet-stream");
    assert_eq!(meta.size, 4);
    assert_eq!(
        target_sync
            .read("blob/deadbeef.bin")
            .expect("the blob is readable"),
        Some(b"raw!".to_vec())
    );
}

#[test]
fn a_blob_with_unreadable_metadata_exports_a_default_entry() {
    let source = MemoryIo::default();
    let sync = SyncFileIo::new(source);
    sync.write("blob/feedface.bin", b"data")
        .expect("the blob is written");
    sync.write("blob/feedface.bin.meta", b"\xff\xff")
        .expect("the metadata is written");

    let bundle = export_from_io(&sync).expect("the volume exports");
    let target = MemoryIo::default();
    let target_sync = SyncFileIo::new(target);
    import_into_io(&target_sync, &bundle).expect("the bundle imports");

    let meta_bytes = target_sync
        .read("blob/feedface.bin.meta")
        .expect("the metadata is readable")
        .expect("the import writes the metadata the export defaulted");
    let meta: ContentMeta = postcard::from_bytes(&meta_bytes).expect("the metadata parses");
    assert_eq!(meta.mime_type, "application/octet-stream");
    assert_eq!(meta.size, 4);
}

#[test]
fn a_session_holds_the_window_between_hydrate_and_flush() {
    let (medium, _) = seeded();

    let mut session = FihSession::new(medium.clone(), "host");
    assert_eq!(session.storage().project_id(), "host");
    assert!(session.is_flushed(), "a new session holds nothing to flush");
    assert!(
        block_on(session.storage().read_state()).facts.is_empty(),
        "the maps are empty until the session hydrates"
    );

    session.hydrate().expect("the session reads the volume");
    assert_eq!(block_on(session.storage().read_state()).facts.len(), 1);

    let second = fact("second payload");
    block_on(session.storage().submit_fact(&second)).expect("the fact is accepted");
    assert!(
        !session.is_flushed(),
        "a write the medium has not seen is not flushed"
    );

    session.flush().expect("the write reaches the medium");
    assert!(session.is_flushed());

    let mut reader = FihSession::new(medium, "host");
    reader
        .hydrate()
        .expect("the second session reads the volume");
    assert_eq!(
        block_on(reader.storage().read_state()).facts.len(),
        2,
        "the flushed write is in the volume"
    );
}

#[test]
fn the_blackboard_sync_surface_writes_reads_and_flushes() {
    let medium = MemoryIo::default();
    let board = FihBlackboard::new(medium.clone(), "board");

    let fact = fact("a payload");
    FactCapable::submit_fact(&board, &fact).expect("the board accepts the fact");
    HintCapable::submit_hint(
        &board,
        &Hint {
            id: CoordId::resolve("h_board"),
            content: "a hint".to_string(),
            creator: "agent".to_string(),
        },
    )
    .expect("the board accepts the hint");

    let state = StorageRead::read_state(&board);
    assert_eq!(StorageRead::project_id(&board), "board");
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.hints.len(), 1);
    assert!(EvictCapable::approximate_size(&board) > 0);

    board.flush_pending().expect("the writes reach the medium");
    board.rebuild_cache().expect("the board rebuilds its maps");
    assert_eq!(StorageRead::read_state(&board).facts.len(), 1);

    // The eviction surface is part of the sync face too, and a bound at the epoch evicts
    // nothing because no record predates it.
    assert_eq!(
        EvictCapable::evict_before(&board, "0").expect("the board evicts"),
        0
    );
    assert_eq!(
        EvictCapable::evict_stale_intents(&board, 60_000_000_000).expect("the board evicts"),
        0
    );
}
