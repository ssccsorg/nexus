// OS-less storage path verification on the host (issue #181).
//
// This crate is support software: it exercises the no_std storage core
// (nex-core, nex-fih, chton) built with the std feature off, driving it
// from a caller-provided critical-section implementation exactly as an
// MCU firmware or launcher would. The host harness supplies a no-op
// critical section and a no-op waker, so nothing on the path needs std:
//   - Cell2 borrow semantics, including cross-thread serialization
//   - EpochClock time flow from a monotonic tick
//   - a FihStorage submit -> flush -> reopen -> read round trip, with the
//     filtered read and the intent and hint records sharing the layer
//   - the read path a device uses: a store that keeps no record maps,
//     answering a query by walking the medium one record at a time, which
//     is the pair a memory-bounded consumer runs (issue #213)
//
// The compile-time no_std property is pinned by the no_std anchor tests in
// the nex sub-workspace; this crate verifies the runtime behavior of that
// same no_std build.

use std::sync::Arc;
use std::thread;

use chton::cell::Cell2;
use chton::io::FileIo;
use nex_core::{EpochClock, Monotonic, Now};
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead,
    BoardState, Content, CoordId, Fact, FihStorage, Hint, Intent, StateFilter,
};
use nexus_verify_support::{FlatIo, block_on};

// The critical-section implementation is provided by critical-section's
// std feature (a real process-wide mutex), standing in for the MCU HAL
// that supplies the symbols on the target. A no-op implementation would
// leave the cross-thread Cell2 check racy; the std implementation is
// what makes the serialization assertion sound on the host.

/// A monotonic tick the harness advances by hand, so the clock under test is the injected
/// one and nothing on the path reaches a system clock.
#[derive(Clone)]
struct Tick(Arc<core::sync::atomic::AtomicU64>);

impl Tick {
    fn at(nanos: u64) -> Self {
        Self(Arc::new(core::sync::atomic::AtomicU64::new(nanos)))
    }
}

impl Monotonic for Tick {
    fn elapsed_nanos(&self) -> u64 {
        self.0.load(core::sync::atomic::Ordering::Relaxed)
    }
}

/// A clock over a fresh tick, which is what a session that has just powered on has.
fn epoch_clock(epoch_secs: u64) -> Box<dyn Now + Send + Sync> {
    Box::new(EpochClock::new(epoch_secs, Tick::at(0)))
}

fn main() {
    // 1. Cell2: value round trip and independent cells.
    let cell = Cell2::new(0u64);
    *cell.borrow_mut() = 41;
    assert_eq!(*cell.borrow(), 41);
    let a = Cell2::new(1u64);
    let b = Cell2::new(2u64);
    {
        let ga = a.borrow();
        let gb = b.borrow();
        assert_eq!(*ga + *gb, 3);
    }
    *a.borrow_mut() = 10;
    assert_eq!(*b.borrow(), 2);

    // 2. Cell2 cross-thread serialization on the OS-less build.
    let shared = Arc::new(Cell2::new(0u64));
    let t1 = {
        let c = Arc::clone(&shared);
        thread::spawn(move || {
            for _ in 0..1000 {
                *c.borrow_mut() += 1;
            }
        })
    };
    let t2 = {
        let c = Arc::clone(&shared);
        thread::spawn(move || {
            for _ in 0..1000 {
                *c.borrow_mut() += 1;
            }
        })
    };
    t1.join().unwrap();
    t2.join().unwrap();
    assert_eq!(*shared.borrow(), 2000);

    // 3. EpochClock time flow from a monotonic tick.
    let tick = Tick::at(0);
    let epoch_secs: u64 = 1_700_000_000;
    let clock = EpochClock::new(epoch_secs, tick.clone());
    let epoch_ns = epoch_secs.saturating_mul(1_000_000_000);
    assert_eq!(clock.now_nanos(), epoch_ns);
    tick.0
        .store(5_000_000_000, core::sync::atomic::Ordering::Relaxed);
    assert_eq!(clock.now_nanos(), epoch_ns + 5_000_000_000);
    assert_eq!(clock.now_secs(), epoch_secs + 5);

    // 4. FihStorage round trip on the no_std build, then a reopen over the same
    // medium. The reopen rebuilds the record layer from the channel, which is
    // the path a device takes every time it powers on.
    let io = FlatIo::new();
    let fact = Fact::new(
        "osless".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"hello osless".to_vec(),
        },
        "harness".into(),
    );
    let id = {
        let storage = FihStorage::with_clock(io.clone(), "osless", Box::new(clock));
        let id = block_on(storage.submit_fact(&fact)).expect("submit_fact");
        block_on(storage.flush_pending()).expect("flush_pending");
        id
    };

    let tick = Tick::at(0);
    let storage = FihStorage::with_clock(
        io.clone(),
        "osless",
        Box::new(EpochClock::new(epoch_secs, tick)),
    );
    block_on(storage.rebuild_cache()).expect("rebuild_cache");

    let state: BoardState = block_on(storage.read_state());
    let found = state
        .facts
        .iter()
        .any(|f| f.id == id && f.content.data == b"hello osless");
    assert!(
        found,
        "submitted fact not found in read-back state after reopen"
    );

    // 5. A filtered read scans the rebuilt record maps; it must agree with the
    // unfiltered read.
    let filtered: BoardState = block_on(storage.read_state_filtered(&StateFilter {
        origin: Some("osless".into()),
        ..Default::default()
    }));
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].origin, "osless");

    // 6. Intents and hints share the record layer and its ordering contract, so
    // they are exercised on the same no_std build.
    let intent = Intent {
        id: CoordId::resolve("i_osless"),
        from_facts: vec![id],
        description: "osless intent".into(),
        creator: "harness".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };
    block_on(storage.submit_intent(&intent)).expect("submit_intent");
    let hint = Hint {
        id: CoordId::resolve("h_osless"),
        content: "osless hint".into(),
        creator: "harness".into(),
    };
    block_on(storage.submit_hint(&hint)).expect("submit_hint");
    block_on(storage.flush_pending()).expect("flush_pending");

    let state: BoardState = block_on(storage.read_state());
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.intents[0].from_facts.len(), 1);
    assert_eq!(state.intents[0].from_facts[0], id);

    // 7. The read path a device uses. A store that keeps no record maps answers a query by
    // walking the medium one record at a time, which is the combination the memory-bounded
    // consumer runs (issue #213).
    device_read_path(epoch_secs);

    println!("nexus-osless-verify: all OS-less surface checks passed");
}

/// A store that keeps no record maps answers a query by walking the medium.
///
/// The maps hold every record the volume contains, at 394 to 406 bytes per record on
/// riscv32, which is the whole read budget of a small part. The mode is complete only if
/// the store that keeps nothing can also answer, so what this holds is that the writes
/// reach the medium, that a session keeping nothing reads them back one at a time, that a
/// volume larger than one page is walked to its end, that the walk and a state read of one
/// volume reach the same records, that a question about one record reads one record, and
/// that a write the part refused is still answered for.
fn device_read_path(epoch_secs: u64) {
    let medium = FlatIo::new();

    // The writer keeps no maps either: a device that has just taken a reading and has
    // nowhere to put it except the part.
    {
        let writer =
            FihStorage::without_maps(medium.clone(), "osless/device", epoch_clock(epoch_secs));
        for payload in ["one", "two", "three"] {
            let fact = Fact::new(
                "osless/device".into(),
                Content {
                    mime_type: "text/plain".into(),
                    data: payload.as_bytes().to_vec(),
                },
                "harness".into(),
            );
            block_on(writer.submit_fact(&fact)).expect("submit_fact");
        }
        block_on(writer.flush_pending()).expect("flush_pending");

        // The mode's defining property: a store without maps holds nothing about the
        // volume, which is what keeps opening a volume independent of how many records it
        // holds. A path that started remembering shows up here as a map with entries.
        assert_eq!(
            writer.fact_records.borrow().len(),
            0,
            "a store without maps kept a fact"
        );
    }

    // The reader is a later session that keeps nothing either: the device that reboots and is
    // asked about a past it wrote, with the medium as the only thing between the two.
    let reader = FihStorage::without_maps(medium.clone(), "osless/device", epoch_clock(epoch_secs));

    let before = medium.traffic();
    let mut walked: Vec<(String, String)> = Vec::new();
    block_on(reader.for_each_fact(|record, content| {
        walked.push((
            record.id.clone(),
            String::from_utf8_lossy(&content.data).into_owned(),
        ));
        true
    }))
    .expect("the walk reaches the medium");
    let made = medium.traffic().since(before);

    // The walk reads the medium; it does not build the maps it exists to avoid.
    assert_eq!(
        reader.fact_records.borrow().len(),
        0,
        "a walk kept the record maps a store without maps must not hold"
    );

    let mut payloads: Vec<String> = walked.iter().map(|(_, text)| text.clone()).collect();
    payloads.sort();
    assert_eq!(
        payloads,
        vec!["one".to_string(), "three".to_string(), "two".to_string()],
        "a store without maps walked back the volume it wrote: {walked:?}"
    );
    // The precise read-cost claim is the framing walk's below. What this holds is that the
    // records came from the medium rather than from memory.
    assert!(
        made.reads >= walked.len() as u32 && made.pages >= 1,
        "a content walk read the medium and enumerated it: {made:?}"
    );

    // The framing walk is the same visit without the payload reads, which is the read a count
    // or an identifier listing spends.
    let before = medium.traffic();
    let mut framing: Vec<String> = Vec::new();
    block_on(reader.for_each_fact_record(|record| {
        framing.push(record.id.clone());
        true
    }))
    .expect("the framing walk reaches the medium");
    let made = medium.traffic().since(before);

    let mut content_ids: Vec<String> = walked.iter().map(|(id, _)| id.clone()).collect();
    content_ids.sort();
    framing.sort();
    assert_eq!(framing, content_ids, "the two walks visit the same records");
    assert_eq!(
        made.reads,
        framing.len() as u32,
        "a framing walk reads one key per record and no payload: {made:?}"
    );

    // A question about one record reads one record, which is the other half of what makes a
    // walk affordable: the memory is a page and a record, and the work is what the question
    // reaches.
    let before = medium.traffic();
    let mut visited = 0;
    block_on(reader.for_each_fact_record(|_record| {
        visited += 1;
        false
    }))
    .expect("the framing walk reaches the medium");
    let made = medium.traffic().since(before);
    assert_eq!(visited, 1, "the walk stopped at the first record");
    assert_eq!(made.reads, 1, "the stop read one record: {made:?}");

    // A host reads the same volume through the maps, and the two have to agree about which
    // records the volume holds. The walk promises no order, so the agreement is about the set
    // rather than about the order.
    let keeper = FihStorage::with_clock(medium.clone(), "osless/device", epoch_clock(epoch_secs));
    block_on(keeper.rebuild_cache()).expect("rebuild_cache");
    let state: BoardState = block_on(keeper.read_state());
    let mut from_state: Vec<String> = state.facts.iter().map(|fact| fact.id.to_string()).collect();
    from_state.sort();
    assert_eq!(
        from_state, content_ids,
        "a walk and a state read reach the same records"
    );

    // A volume larger than one page. The walk reads the channel's keys sixteen at a time and
    // resumes from the cursor, so a channel that cannot resume, or a walk that loses its place,
    // shows up as records the walk never reaches rather than as a count that is merely small.
    let large = FlatIo::new();
    {
        let writer =
            FihStorage::without_maps(large.clone(), "osless/large", epoch_clock(epoch_secs));
        for index in 0..40 {
            let fact = Fact::new(
                "osless/large".into(),
                Content {
                    mime_type: "text/plain".into(),
                    data: format!("payload {index}").into_bytes(),
                },
                "harness".into(),
            );
            block_on(writer.submit_fact(&fact)).expect("submit_fact");
        }
        block_on(writer.flush_pending()).expect("flush_pending");
    }

    let reader = FihStorage::without_maps(large.clone(), "osless/large", epoch_clock(epoch_secs));
    let before = large.traffic();
    let mut seen = 0;
    block_on(reader.for_each_fact_record(|_record| {
        seen += 1;
        true
    }))
    .expect("the framing walk reaches the medium");
    let made = large.traffic().since(before);
    assert_eq!(seen, 40, "the walk reached every record of the volume");
    assert!(
        made.pages >= 3,
        "a walk of forty records with a page of sixteen reads more than one page: {made:?}"
    );

    // A part at the end of its life refuses the write, and a refused flush must not make the
    // volume unreadable: the writes the medium refused stay pending and are visited from
    // memory. Without that, a worn part turns a record the session still holds into one that
    // is gone.
    let worn = FlatIo::new();
    worn.refuse_writes(true);
    let storage = FihStorage::without_maps(worn.clone(), "osless/worn", epoch_clock(epoch_secs));
    let fact = Fact::new(
        "osless/worn".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"a write the part refuses".to_vec(),
        },
        "harness".into(),
    );
    block_on(storage.submit_fact(&fact)).expect("submit_fact");
    assert!(
        block_on(storage.flush_pending()).is_err(),
        "the medium accepted a write it refuses"
    );

    let mut survived = 0;
    block_on(storage.for_each_fact(|_record, content| {
        if content.data == b"a write the part refuses" {
            survived += 1;
        }
        true
    }))
    .expect("the walk reaches the medium");
    assert_eq!(survived, 1, "the walk lost a refused write");

    let landed = block_on(worn.list("")).expect("list");
    assert!(
        landed.is_empty(),
        "the medium kept a write it had refused: {landed:?}"
    );
}
