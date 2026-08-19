// Memory probe: marginal live-heap cost per fact in the unified store.
//
// Measurement tooling for the Step 1 path reduction and the L2 restructure
// (#176). Not part of the regular gate. Run with:
//
//   cargo test -p nex --test memory_probe -- --ignored --nocapture
//
// The test binary installs a counting global allocator so the live heap
// footprint of FihStorage is observable. Facts are placed through
// submit_fact with identity coordinates varying across all six axes (the
// pseudo-random content-derived ids nexus actually uses), so each record
// forces a fresh branch at every identity level and a per-record leaf.
//
// Measured with this probe (16 facts):
//   - before Step 1 (19 axes, serial-only identity variation): 3.40 MB/fact
//   - after Step 1  (12 axes, serial-only identity variation): 1.35 MB/fact
//   - after Step 1  (12 axes, full identity variation):        3.58 MB/fact
//
// The gap between the serial-only and full-variation figures is the leaf:
// CoordSpaceN keys the leaf node by the second-to-last coordinate
// (coord N-2), so records sharing that coordinate share one leaf. With
// pseudo-random ids every identity coordinate varies, the leaf is
// per-record again, and the Step 1 saving is bounded by the seven removed
// 89 KB branch nodes (~0.6 MB/fact). The 0.5 MB/fact target requires the
// L2 restructure, where the tree stops storing full records.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, Content, CoordId, Fact, FihStorage};
use nexus_storage_sim::SimIo;

struct CountingAlloc;
static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let delta = new_size as isize - layout.size() as isize;
        if delta > 0 {
            LIVE.fetch_add(delta as usize, Ordering::Relaxed);
        } else {
            LIVE.fetch_sub((-delta) as usize, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn fact_at(i: u32) -> Fact {
    // Identity coordinates varying across all six axes, like the
    // pseudo-random content-derived ids in real usage.
    let cid = CoordId::from_indices([i as u16, i as u16, i as u16, i as u16, i as u16, i as u16])
        .unwrap();
    Fact::with_id(
        cid,
        "probe".into(),
        Content {
            mime_type: "text/plain".into(),
            data: format!("payload-{i}").into_bytes(),
        },
        "probe".into(),
    )
}

#[test]
#[ignore]
fn report_per_fact_heap_cost() {
    const N: usize = 16;
    let store = FihStorage::new(SimIo::new(), "memory-probe");
    let baseline = LIVE.load(Ordering::Relaxed);
    for i in 0..N {
        block_on(store.submit_fact(&fact_at(i as u32))).unwrap();
    }
    let delta = LIVE.load(Ordering::Relaxed) - baseline;
    println!(
        "per-fact live heap cost: {} bytes ({} facts, total delta {} bytes)",
        delta / N,
        N,
        delta
    );
}
