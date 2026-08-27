// Deterministic same-result test for the multi-dimensional search paths
// (issue #179): `read_state_filtered` (record-map scan with field
// predicates) and `structural_fact_ids` (iter_prefix over the structural
// filter index) must return the identical fact id set for the same
// filter, across selectivity shapes and edge cases.
//
// The fixture mirrors the bench scenario at a small axis-combo scale so
// the tree stays small: 5 days x 4 origins x 5 creators = 100 leaves
// (the CoordSpaceN dense-node cost is 11,172 slots per node, so the
// tree is bounded by axis combos, not record count). Origin and creator
// axes are hash fingerprints, so the candidate re-filters exactly; a
// fingerprint collision would surface here as an id-set mismatch.

use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncIntentCapable, CoordId, Fact, FihStorage, Intent,
    StateFilter,
};
use nexus_storage_sim::SimIo;

// Real SSCCS documentation manifest (generated from docs/_llms/llms.txt):
// (section, area, title) used by the lifecycle scenario.
#[path = "../llms_manifest.rs"]
mod llms_manifest;

const DAY_NS: u64 = 86_400_000_000_000;
const T0_NS: u64 = 1_000_000_000_000_000_000;
const N_ORIGINS: usize = 4;
const N_CREATORS: usize = 5;
const N_DAYS: usize = 5;

/// Clock with a shared handle: the fixture sets the timestamp at each
/// day boundary, so every fact in a day group shares one day bucket.
#[derive(Clone)]
struct StepClock(Arc<Mutex<u64>>);

impl StepClock {
    fn new(start: u64) -> Self {
        Self(Arc::new(Mutex::new(start)))
    }
    fn set(&self, ts: u64) {
        *self.0.lock().unwrap() = ts;
    }
}

impl nex_core::Now for StepClock {
    fn now_nanos(&self) -> u64 {
        *self.0.lock().unwrap()
    }
    fn now_secs(&self) -> u64 {
        *self.0.lock().unwrap() / 1_000_000_000
    }
}

fn build_store(n_facts: usize) -> FihStorage<SimIo> {
    let clock = StepClock::new(T0_NS);
    let store = FihStorage::with_clock(SimIo::new(), "structural-search", Box::new(clock.clone()));
    let per_day = n_facts / N_DAYS;
    let mut i = 0usize;
    for day in 0..N_DAYS {
        clock.set(T0_NS + (day as u64) * DAY_NS);
        for _ in 0..per_day {
            let cid = CoordId::from_label(&format!("search-{i}"));
            let fact = Fact::with_id(
                cid,
                format!("origin-{}", i % N_ORIGINS),
                format!(
                    "Document {}: research content about paradigm shift",
                    i % 100
                )
                .into(),
                format!("creator-{}", (i / N_ORIGINS) % N_CREATORS),
            );
            block_on(store.submit_fact(&fact)).unwrap();
            i += 1;
        }
    }
    block_on(store.flush_pending()).unwrap();
    store
}

fn baseline_ids(store: &FihStorage<SimIo>, filter: &StateFilter) -> Vec<String> {
    let state = block_on(store.read_state_filtered(filter));
    let mut ids: Vec<String> = state.facts.iter().map(|f| f.id.to_string()).collect();
    ids.sort();
    ids
}

fn since(day: usize) -> String {
    (T0_NS + (day as u64) * DAY_NS).to_string()
}

fn until(day: usize) -> String {
    (T0_NS + (day as u64 + 1) * DAY_NS - 1).to_string()
}

fn assert_same_result(store: &FihStorage<SimIo>, filter: &StateFilter) {
    let base = baseline_ids(store, filter);
    let cand = store.structural_fact_ids(filter);
    assert_eq!(
        base, cand,
        "paths must return identical id sets for {filter:?}"
    );
}

#[test]
fn three_axis_narrow_and_wide_agree() {
    let store = build_store(10_000);
    // Origin + creator + a one-day window (high selectivity).
    assert_same_result(
        &store,
        &StateFilter {
            origin: Some("origin-3".into()),
            creator: Some("creator-2".into()),
            since: Some(since(4)),
            until: Some(until(4)),
            ..Default::default()
        },
    );
    // Origin + creator + a full-range window (low selectivity).
    assert_same_result(
        &store,
        &StateFilter {
            origin: Some("origin-1".into()),
            creator: Some("creator-4".into()),
            since: Some(since(0)),
            until: Some(until(4)),
            ..Default::default()
        },
    );
}

#[test]
fn partial_axis_filters_agree() {
    let store = build_store(10_000);
    // Creator + time: origin is not fixed, so the contiguous-prefix
    // property stops the prefix at entity; the exact predicate carries
    // the creator selectivity.
    assert_same_result(
        &store,
        &StateFilter {
            creator: Some("creator-3".into()),
            since: Some(since(2)),
            until: Some(until(4)),
            ..Default::default()
        },
    );
    // Origin + time.
    assert_same_result(
        &store,
        &StateFilter {
            origin: Some("origin-2".into()),
            since: Some(since(0)),
            until: Some(until(2)),
            ..Default::default()
        },
    );
    // Creator only, no time bounds: the prefix drops the time axes.
    assert_same_result(
        &store,
        &StateFilter {
            creator: Some("creator-0".into()),
            ..Default::default()
        },
    );
}

#[test]
fn lifecycle_docs_and_conclusions_agree() {
    // Real-manifest docs + intent lifecycle (claim/conclude produce
    // conclusion facts with unique "conclusion:<intent>" origins). Both
    // paths must agree, including the high-cardinality conclusion origin
    // axis and the intent status transitions.
    let clock = StepClock::new(T0_NS);
    let store = FihStorage::with_clock(SimIo::new(), "lifecycle", Box::new(clock.clone()));
    for rep in 0..2usize {
        clock.set(T0_NS + (rep as u64) * DAY_NS);
        for (i, (section, area, title)) in llms_manifest::LLMS_DOCS.iter().enumerate() {
            let doc_cid = CoordId::from_label(&format!("doc-{rep}-{i}"));
            let doc = Fact::with_id(
                doc_cid,
                (*section).into(),
                format!("{title}\n\nMarkdown body of the {title} document.").into(),
                (*area).into(),
            );
            block_on(store.submit_fact(&doc)).unwrap();
            let intent_id = format!("analyze-{rep}-{i}");
            let intent = Intent::new(
                CoordId::from_label(&intent_id),
                vec![doc_cid],
                None,
                format!("analyze {title}"),
                (*area).into(),
            );
            block_on(store.submit_intent(&intent)).unwrap();
            block_on(store.claim_intent(&intent_id, area)).unwrap();
            let conclusion = format!("conclusion for {title}");
            block_on(store.conclude_intent(&intent_id, &conclusion)).unwrap();
        }
    }
    block_on(store.flush_pending()).unwrap();

    // Section + area + window: doc facts only (conclusion origins differ).
    assert_same_result(
        &store,
        &StateFilter {
            origin: Some("projects".into()),
            creator: Some("nexus".into()),
            since: Some(since(0)),
            until: Some(until(1)),
            ..Default::default()
        },
    );
    // Creator-only + window: docs and conclusions by the notes maintainer.
    assert_same_result(
        &store,
        &StateFilter {
            creator: Some("notes".into()),
            since: Some(since(0)),
            until: Some(until(1)),
            ..Default::default()
        },
    );
    // Origin-only + window.
    assert_same_result(
        &store,
        &StateFilter {
            origin: Some("whitepaper".into()),
            since: Some(since(0)),
            until: Some(until(1)),
            ..Default::default()
        },
    );
}

#[test]
fn time_only_empty_and_unbounded_agree() {
    let store = build_store(10_000);
    // Time range only.
    assert_same_result(
        &store,
        &StateFilter {
            since: Some(since(1)),
            until: Some(until(3)),
            ..Default::default()
        },
    );
    // Empty window: the day range lies beyond the fixture days.
    assert_same_result(
        &store,
        &StateFilter {
            origin: Some("origin-3".into()),
            creator: Some("creator-3".into()),
            since: Some(since(20)),
            until: Some(until(21)),
            ..Default::default()
        },
    );
    // No filter at all: full state.
    assert_same_result(&store, &StateFilter::default());
}
