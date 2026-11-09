// A read costs the same whatever the store holds.
//
// This is the property the layer rests on: reading one record must not walk or copy the rest
// of them. A structure that grows with the record count shows up here as a rising cost per
// read, which a small fixture hides and a deployed volume does not.
//
// One test in the file, because the counter is global and a second test would measure beside
// this one's allocations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, Content, Fact};
use nex_io::SimIo;
use nexus_storage_ref::RefFihStorage;

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

const SMALL: usize = 16;
const LARGE: usize = 160;
const READS_PER_BATCH: usize = 64;
const BATCHES: usize = 8;

/// A store holding `count` facts, every write committed.
fn seeded(count: usize) -> (RefFihStorage<SimIo>, String) {
    let storage = RefFihStorage::new(SimIo::new(), "read-shape");
    let mut first = None;
    for at in 0..count {
        let fact = Fact::new(
            format!("did:example:{at}"),
            Content {
                mime_type: "text/plain".into(),
                data: format!("record {at}").into_bytes(),
            },
            "writer".into(),
        );
        let address = block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
        block_on(storage.flush_pending()).expect("the write reaches the medium");
        if at == 0 {
            first = Some(address);
        }
    }
    (storage, first.expect("a record was written").to_string())
}

/// Allocations per read, as the smallest batch, so interference only ever raises a count.
fn allocations_per_read(storage: &RefFihStorage<SimIo>, id: &str) -> usize {
    let _ = block_on(storage.get_fact_by_id(id));
    let mut cheapest = usize::MAX;
    for _ in 0..BATCHES {
        let before = ALLOCATIONS.load(Ordering::SeqCst);
        for _ in 0..READS_PER_BATCH {
            assert!(block_on(storage.get_fact_by_id(id)).is_some());
        }
        let per_read = (ALLOCATIONS.load(Ordering::SeqCst) - before) / READS_PER_BATCH;
        cheapest = cheapest.min(per_read);
    }
    cheapest
}

#[test]
fn a_read_costs_the_same_whatever_the_store_holds() {
    let (small, id) = seeded(SMALL);
    let (large, large_id) = seeded(LARGE);
    assert_eq!(id, large_id, "the same record is read in both stores");

    let at_small = allocations_per_read(&small, &id);
    let at_large = allocations_per_read(&large, &id);
    assert!(
        at_large <= at_small + 2,
        "a read allocated {at_small} times with {SMALL} records and {at_large} with {LARGE}"
    );
}
