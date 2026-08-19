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
// pseudo-random content-derived ids nexus actually uses), so the record
// layer gets one entry per record.
//
// Measured with this probe (16 facts, full identity variation):
//   - before Step 1 (19 axes):               3.40 MB/fact
//   - after Step 1  (12 axes):               3.58 MB/fact
//   - after Step 2  (6-axis filter index):  40 KB/fact average
//                                            ~500 B/fact marginal (last
//                                            fact, steady state)
//
// The Step 2 drop is the L2 restructure: the tree holds id sets at
// structural paths (memory bounded by axis cardinality, the ~650 KB
// one-time cost of the probe's structural space), the record bodies live
// in HashMap record maps, and the id-keyed entity stores are
// HashMap-backed. The 0.5 MB/fact target from the briefing is exceeded:
// the remaining per-record cost is a few hundred bytes.

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
    let mut prev = baseline;
    let mut last_marginal = 0;
    for i in 0..N {
        block_on(store.submit_fact(&fact_at(i as u32))).unwrap();
        let now = LIVE.load(Ordering::Relaxed);
        last_marginal = now - prev;
        prev = now;
    }
    let delta = LIVE.load(Ordering::Relaxed) - baseline;
    println!(
        "per-fact live heap cost: {} bytes average ({} facts, total delta {} bytes); last-fact marginal: {} bytes",
        delta / N,
        N,
        delta,
        last_marginal
    );
}
