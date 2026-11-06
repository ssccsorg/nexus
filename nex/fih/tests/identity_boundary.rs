// The identity boundary, held as a test rather than as a review rule.
//
// Three properties, and one test so the allocation count is never measured beside another test's allocations.
//
//  1. The record maps are keyed by the address. The assertions name the key type, so a map keyed by its text stops
//     this file from compiling.
//  2. A typed door never parses. A function whose name ends in `_at`, and every `medium_` reach, takes or builds an
//     address; a parse there is the conversion the door exists to avoid.
//  3. A lookup that holds an address allocates its result and nothing else. A render of the key allocates, so any
//     reintroduced conversion shows up here as a count rather than as a measurement nobody takes.

#![cfg(feature = "std")]

use nex_fih::FihId20 as _;
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, AsyncIntentCapable, Cell2, Content, CoordId, Fact, FactRecord, FihStorage,
    HintRecord, Intent, IntentRecord,
};

/// Counts allocation events, which is what a conversion of an identity costs.
struct CountingAlloc;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

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

/// Calls a reported allocation count is taken over.
const CALLS: usize = 100;

/// The name of a function declared on a line, if the line declares one.
fn function_name(line: &str) -> Option<String> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix("pub(crate) ").unwrap_or(rest);
    let rest = rest.strip_prefix("pub ").unwrap_or(rest);
    let rest = rest.strip_prefix("pub(super) ").unwrap_or(rest);
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    let rest = rest.strip_prefix("const ").unwrap_or(rest);
    let rest = rest.strip_prefix("unsafe ").unwrap_or(rest);
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

#[test]
fn the_identity_is_a_value_in_the_containers_and_text_only_at_the_doors() {
    let storage = FihStorage::new(MemoryIo::default(), "identity-boundary");

    // A map keyed by its text stops this from compiling.
    let _: &Cell2<HashMap<CoordId, FactRecord>> = &storage.fact_records;
    let _: &Cell2<HashMap<CoordId, IntentRecord>> = &storage.intent_records;
    let _: &Cell2<HashMap<CoordId, HintRecord>> = &storage.hint_records;

    let source = include_str!("../src/core/store.rs");
    let mut inside = String::new();
    for (index, line) in source.lines().enumerate() {
        if let Some(name) = function_name(line) {
            inside = name;
        }
        let typed = inside.ends_with("_at") || inside.starts_with("medium_");
        assert!(
            !(typed && line.contains("CoordId::resolve")),
            "{inside} takes an address and parses at line {}; pass the address it is given",
            index + 1
        );
    }

    let fact = Fact::with_id(
        CoordId::resolve("f_identity_boundary"),
        "origin/one".into(),
        Content::from("the payload"),
        "agent".into(),
    );
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
    let intent = Intent::new(
        CoordId::resolve("i_identity_boundary"),
        vec![fact.id],
        None,
        "the intent".into(),
        "agent".into(),
    );
    block_on(storage.submit_intent(&intent)).expect("the intent is accepted");

    // Warm whatever the first call builds, so the count is the lookup alone.
    assert_eq!(storage.intents_by_fact_id(fact.id).len(), 1);

    let before = ALLOCATIONS.load(Ordering::SeqCst);
    for _ in 0..CALLS {
        assert_eq!(storage.intents_by_fact_id(fact.id).len(), 1);
    }
    let at_address = ALLOCATIONS.load(Ordering::SeqCst) - before;

    let canonical = fact.id.to_string();
    let before = ALLOCATIONS.load(Ordering::SeqCst);
    for _ in 0..CALLS {
        assert_eq!(storage.intents_by_fact(&canonical).len(), 1);
    }
    let by_name = ALLOCATIONS.load(Ordering::SeqCst) - before;

    // One vector and one identifier, which is the result and only the result. A render of the key would raise this
    // by one a call, and a clone the caller did not ask for by one more.
    assert_eq!(
        at_address,
        2 * CALLS,
        "a lookup at an address allocates its result and nothing else"
    );
    // A canonical name parses into a fixed array, so the text door costs the same as the address door.
    assert_eq!(
        by_name,
        2 * CALLS,
        "a canonical name parses without allocating"
    );
}
