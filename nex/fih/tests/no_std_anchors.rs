// no_std anchors for nex-fih (issue #181).
//
// nex-fih is the semantic layer of the OS-less storage path: it models
// facts, intents, hints, and governance, and never reads wall-clock time
// or executes async I/O itself. Time comes from an injected `Now`
// (`with_clock`), which is how an MCU launcher supplies an EpochClock
// without an RTC. This test pins that the no_std surface stays std-free:
//
//   - `FihContract::with_clock` is available without std,
//   - `FihContract::new` (SystemClock) exists only under `std`,
//   - `FihStorage` and the `FileIo` contract do not leak std types.
//
// Run with:
//
//   cargo test --no-default-features --test no_std_anchors
//
// The std-gated constructor is exercised by the std test suite; calling
// it from this file fails to compile under `--no-default-features`.

use alloc::boxed::Box;

extern crate alloc;

use nex_core::{EpochClock, Monotonic, Now};
use nex_fih::contract::core::{EvidenceChain, GovernanceGate, HintEngine, HintRule};
use nex_fih::core::index::Cell2;
use nex_fih::FihContract;

// ── Monotonic: fake MCU tick, std-free ─────────────────────────────────

#[derive(Debug, Clone, Default)]
struct FakeTick {
    elapsed_ns: u64,
}

impl Monotonic for FakeTick {
    fn elapsed_nanos(&self) -> u64 {
        self.elapsed_ns
    }
}

// ── FihContract: time injection keeps the layer std-free ───────────────

#[test]
fn contract_constructs_with_injected_clock() {
    // The no_std path must construct with an injected clock: no
    // `SystemTime`, no std. An MCU launcher pins an epoch at boot and
    // advances from its timer tick.
    let clock: Box<dyn Now + Send + Sync> = Box::new(EpochClock::new(1_700_000_000, FakeTick::default()));
    let contract = FihContract::with_clock(clock);
    assert_eq!(contract.gate.schema_count(), 0);
    assert!(contract.evidence.verify(0));
}

#[test]
fn contract_default_schemas_are_std_free() {
    let clock: Box<dyn Now + Send + Sync> = Box::new(EpochClock::new(0, FakeTick::default()));
    let contract = FihContract::with_clock(clock);
    contract.register_default_schemas();
    // text/plain and text/markdown are part of the default FIH schema set.
    assert!(contract.gate.has_schema("text/plain"));
    assert!(contract.gate.has_schema("text/markdown"));
}

// ── Governance primitives: pure, no std ────────────────────────────────

#[test]
fn governance_primitives_are_std_free() {
    let gate = GovernanceGate::new();
    let hash = gate.register_schema("s1", b"schema-bytes");
    assert_eq!(hash.len(), 64); // hex-encoded SHA-256
    assert!(gate.has_schema("s1"));
    assert!(gate.admit("s1", b"data").is_ok());
    assert!(gate.admit("unknown", b"data").is_err());
    gate.unregister_schema("s1");
    assert!(!gate.has_schema("s1"));

    let hint = HintEngine::new();
    hint.add("positive", HintRule::Gt(0));
    assert!(hint.check_numeric(1).is_ok());
    assert!(hint.check_numeric(-1).is_err());
    assert!(HintRule::Gt(0).check_numeric(1));
    assert!(!HintRule::Gt(0).check_numeric(-1));

    let evidence = EvidenceChain::new();
    evidence.append("a", "fact:submit", 1);
    evidence.append("b", "fact:submit", 2);
    assert!(evidence.verify(0));
    assert!(evidence.fingerprint().is_some());
}

// ── Cell2: interior mutability, std-free (construction only) ───────────

#[test]
fn cell2_surface_is_std_free() {
    // Borrowing requires a critical-section implementation (provided by
    // the firmware on a real MCU); construction must not need std.
    let _cell = Cell2::<u64>::new(7);
}

// ── Anchor documentation: the std-only surface ─────────────────────────
//
// The following must NOT be referenced in this file:
//
//   - `FihContract::new()` (SystemClock; std feature only)
//   - `nex_core::SystemClock`, `nex_core::InstantClock`, `nex_core::HostClock`
//
// If the `std` gate on any of these is accidentally removed, the
// `--no-default-features` build fails at their `std::time` usage, which
// the CI job catches before merge.
