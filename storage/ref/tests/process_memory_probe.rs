// Memory probe: marginal live-heap cost per fact in the reference node.
//
// Measurement tool, not a gate. Run with:
//
//   cargo test -p nexus-storage-ref --test process_memory_probe -- --ignored --nocapture
//
// The test binary installs a counting global allocator, so the live heap of RefFihStorage is
// observable. Facts are placed through submit_fact with identity coordinates spread across
// the axes, so the record layer gets one entry per record.
//
// The figures below were taken while the axis filter index and its structural paths lived
// here. Those structures left with the record layer, so this tree measures the record maps
// alone, and the earlier rows describe a tree that no longer exists:
//
//   - before Step 1 (19 axes):               3.40 MB/fact
//   - after Step 1  (12 axes):               3.58 MB/fact
//   - after Step 2  (6-axis filter index):  40 KB/fact average, ~0.8 KB/fact marginal
//   - after Step 4  (CoordId ids):          41 KB/fact average, ~1.2 KB/fact marginal
//   - after the entity store merge:         40 KB/fact average, ~0.9 KB/fact marginal
//
// This probe reports a number, so a drift is visible but not enforced. The read path's cost
// is held by `read_shape.rs`, which asserts that a read costs the same at any store size.

use nex_fih::FihId20 as _;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, Content, CoordId, Fact};
use nexus_storage_ref::{RefFihStorage};
use nex_io::SimIo;

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
    // Distinct ids via label derivation (ids are opaque 20-coordinate
    // hashes since the CoordId migration).
    let cid = CoordId::from_label(&format!("probe-{i}"));
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
    let store = RefFihStorage::new(SimIo::new(), "memory-probe");
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
