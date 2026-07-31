//! Shared benchmark fixtures for nexus-bench.
//!
//! Both bench targets (`instant` and `criterion`) populate FihStorage with
//! controlled axis distributions so that queries exercise specific
//! fast-path and CoordSpaceN code paths deterministically.

use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, AsyncIntentCapable, CoordId, Fact, FihStorage, Intent};
use nexus_storage_sim::SimIo;

// ── Constants (controlled axis distribution) ────────────────────────────

pub const N_FACTS: usize = 50_000;
pub const N_ORIGINS: usize = 50;
pub const N_CREATORS: usize = 20;
pub const N_TIMEBUCKETS: usize = 100;
pub const N_INTS: usize = 10_000;

// ── Fixture: 10K facts (time-aware CoordId axes) ────────────────────────

pub fn populate_10k(store: &FihStorage<SimIo>) -> u64 {
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    for i in 0..10_000u32 {
        let cid = CoordId::from_axes(
            ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16,
            ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16,
            0,
            (i % 50) as u16,
            (i % 20) as u16,
            (i / 1000) as u16,
        )
        .unwrap();
        block_on(store.submit_fact(&Fact::with_id(
            cid,
            format!("origin-{}", i % 50),
            format!("c-{}", i).into(),
            format!("creator-{}", i % 20),
        )))
        .unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    clock
}

// ── Fixture: 50K facts (sequential CoordId) ─────────────────────────────

pub fn populate_50k(store: &FihStorage<SimIo>) {
    for i in 0..50_000u32 {
        block_on(store.submit_fact(&Fact::with_id(
            CoordId::new(i as u64),
            format!("origin-{}", i % 500),
            format!("c-{}", i).into(),
            format!("creator-{}", i % 200),
        )))
        .unwrap();
    }
    block_on(store.flush_pending()).unwrap();
}

// ── Fixture: 50K facts with explicit multi-axis CoordId ────────────────

pub fn build_fact_store() -> FihStorage<SimIo> {
    let io = SimIo::new();
    let store = FihStorage::with_clock(io, "multi-axis", Box::new(nex_core::SystemClock));

    for i in 0..N_FACTS {
        let cid = CoordId::from_axes(
            (i % N_TIMEBUCKETS) as u16,
            0,
            0, // entity = Fact
            (i % N_ORIGINS) as u16,
            (i % N_CREATORS) as u16,
            i as u16,
        )
        .unwrap();
        let fact = Fact::with_id(
            cid,
            format!("origin-{}", i % N_ORIGINS),
            format!("content-{}", i).into(),
            format!("creator-{}", i % N_CREATORS),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    store
}

// ── Fixture: fact store + 10K intents ───────────────────────────────────

pub fn build_intent_store() -> FihStorage<SimIo> {
    let store = build_fact_store();
    for i in 0..N_INTS {
        let cid = CoordId::from_axes(
            (i % 10) as u16,
            0,
            1, // entity = Intent
            (i % N_ORIGINS) as u16,
            (i % N_CREATORS) as u16,
            (N_FACTS + i) as u16,
        )
        .unwrap();
        let intent = Intent::new(
            cid,
            vec![
                CoordId::from_axes(
                    (i % N_TIMEBUCKETS) as u16,
                    0,
                    0,
                    (i % N_ORIGINS) as u16,
                    (i % N_CREATORS) as u16,
                    i as u16,
                )
                .unwrap(),
            ],
            None,
            format!("intent-{}", i),
            format!("creator-{}", i % N_CREATORS),
        );
        block_on(store.submit_intent(&intent)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    store
}

// ── Fixture: research knowledge base (10K docs + 1K intents) ────────────

pub fn build_knowledge_base() -> FihStorage<SimIo> {
    let io = SimIo::new();
    let store = FihStorage::with_clock(io, "kb", Box::new(nex_core::SystemClock));

    // 10K documents across 10 projects, 20 authors, 50 time buckets
    for i in 0..10_000 {
        let cid = CoordId::from_axes(
            (i / 2000) as u16,        // [0] time: every 2000 docs = 1 time bucket
            (i % 2000) as u16,        // [1] sequence within bucket
            0,                        // [2] entity: Fact
            (i % 10) as u16,          // [3] origin: project (0..9)
            ((i / 1000) % 20) as u16, // [4] creator: author (0..19)
            i as u16,                 // [5] serial
        )
        .unwrap();
        let fact = Fact::with_id(
            cid,
            format!("project-{}", i % 10),
            format!("Document {}: research content about paradigm shift", i).into(),
            format!("author-{}", (i / 1000) % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }

    // 1000 intents referencing the last 1000 facts
    for i in 0..1000 {
        let fact_idx = 10_000 - 1000 + i; // last 1000 facts
        let cid = CoordId::from_axes(
            (fact_idx / 2000) as u16,
            (fact_idx % 2000) as u16,
            0,
            (fact_idx % 10) as u16,
            ((fact_idx / 1000) % 20) as u16,
            fact_idx as u16,
        )
        .unwrap();
        let intent_id = CoordId::from_axes(50, i as u16, 1, 0, 0, (200_000 + i) as u16).unwrap();
        let intent = Intent::new(
            intent_id,
            vec![cid],
            None,
            format!("analyze document {}", fact_idx),
            "detector".into(),
        );
        block_on(store.submit_intent(&intent)).unwrap();
    }

    block_on(store.flush_pending()).unwrap();
    store
}
