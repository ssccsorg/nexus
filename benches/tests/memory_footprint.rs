// Memory probe: the structural filter index footprint across record
// counts (issue #179). Not part of the regular gate. Run with:
//
//   cargo test -p nexus-bench --test memory_footprint -- --ignored --nocapture
//
// Two measurements:
//
//  1. Multidim fixture with a FIXED axis-combo space (10 days x 10
//     origins x 10 creators = 1000 leaves) while the record count grows
//     from 10k to 1m. The tree footprint must stay constant; the record
//     layer grows linearly. The reported constant (intercept) is the
//     spatial index plus fixed overheads, and the marginal is the
//     per-record cost (record maps plus pending/io record storage).
//
//  2. Conclusion-origin cardinality: c facts with unique
//     "conclusion:{i}" origins on one day. A high-cardinality axis
//     inflates the tree linearly in distinct axis values (one 268 KB
//     leaf plus one 89 KB branch per origin), which bounds the
//     "constant index" claim: constant in record count, linear in
//     distinct axis combos.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

const DAY_NS: u64 = 86_400_000_000_000;
const T0_NS: u64 = 1_000_000_000_000_000_000;
const N_DAYS: usize = 10;
const N_ORIGINS: usize = 10;
const N_CREATORS: usize = 10;

/// Clock with a shared handle: the fixture sets the timestamp at each
/// day boundary, so every fact in a day group shares one day bucket.
#[derive(Clone)]
struct StepDayClock(Arc<Mutex<u64>>);

impl StepDayClock {
    fn new(start: u64) -> Self {
        Self(Arc::new(Mutex::new(start)))
    }
    fn set(&self, ts: u64) {
        *self.0.lock().unwrap() = ts;
    }
}

impl nex_core::Now for StepDayClock {
    fn now_nanos(&self) -> u64 {
        *self.0.lock().unwrap()
    }
    fn now_secs(&self) -> u64 {
        *self.0.lock().unwrap() / 1_000_000_000
    }
}

/// Multidim fixture with a fixed axis-combo space: the structural index
/// is identical at every record count, only the record layer grows.
fn build_multidim_store(n_facts: usize) -> FihStorage<SimIo> {
    let clock = StepDayClock::new(T0_NS);
    let store = FihStorage::with_clock(SimIo::new(), "fp", Box::new(clock.clone()));
    let per_day = n_facts / N_DAYS;
    let mut i = 0usize;
    for day in 0..N_DAYS {
        clock.set(T0_NS + (day as u64) * DAY_NS);
        for _ in 0..per_day {
            let cid = CoordId::from_label(&format!("fp-{i}"));
            let fact = Fact::with_id(
                cid,
                format!("origin-{}", i % N_ORIGINS),
                Content {
                    mime_type: "text/plain".into(),
                    data: format!(
                        "Document {}: research content about paradigm shift",
                        i % 100
                    )
                    .into_bytes(),
                },
                format!("creator-{}", (i / N_ORIGINS) % N_CREATORS),
            );
            block_on(store.submit_fact(&fact)).unwrap();
            i += 1;
        }
    }
    block_on(store.flush_pending()).unwrap();
    store
}

/// `c` facts with unique `conclusion:{i}` origins on one day: isolates
/// the axis-cardinality cost of high-cardinality origins.
fn build_conclusion_store(c: usize) -> FihStorage<SimIo> {
    let clock = StepDayClock::new(T0_NS);
    let store = FihStorage::with_clock(SimIo::new(), "fp-concl", Box::new(clock.clone()));
    for i in 0..c {
        let cid = CoordId::from_label(&format!("concl-{i}"));
        let fact = Fact::with_id(
            cid,
            format!("conclusion:{i}"),
            Content {
                mime_type: "text/plain".into(),
                data: format!("conclusion body {i}").into_bytes(),
            },
            "worker".into(),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    store
}

#[test]
#[ignore]
fn report_structural_index_footprint() {
    let mut totals = std::collections::HashMap::new();
    for n in [10_000usize, 100_000, 1_000_000] {
        let baseline = LIVE.load(Ordering::Relaxed);
        let store = build_multidim_store(n);
        let total = LIVE.load(Ordering::Relaxed) - baseline;
        totals.insert(n, total);
        println!("multidim {n:>9} facts: total live heap delta {total:>12} bytes");
        drop(store);
    }
    let t10k = totals[&10_000];
    let t100k = totals[&100_000];
    let t1m = totals[&1_000_000];
    let per_record = (t1m - t100k) as f64 / 900_000.0;
    let tree_10k = t10k as f64 - 10_000.0 * per_record;
    let tree_100k = t100k as f64 - 100_000.0 * per_record;
    let tree_1m = t1m as f64 - 1_000_000.0 * per_record;
    println!("per-record marginal (1m vs 100k): {per_record:.0} bytes");
    println!(
        "constant structural index (intercept): 10k {tree_10k:.0}, 100k {tree_100k:.0}, 1m {tree_1m:.0} bytes"
    );

    let mut concl = Vec::new();
    for c in [100usize, 400] {
        let baseline = LIVE.load(Ordering::Relaxed);
        let store = build_conclusion_store(c);
        let total = LIVE.load(Ordering::Relaxed) - baseline;
        concl.push((c, total));
        println!("conclusion origins {c:>4}: total live heap delta {total:>12} bytes");
        drop(store);
    }
    let (c1, t1) = concl[0];
    let (c2, t2) = concl[1];
    println!(
        "per distinct conclusion origin (leaf + branch + record): {:.0} bytes",
        (t2 - t1) as f64 / (c2 - c1) as f64
    );
}
