# The thin nexus principle: storage down, semantics up

## Context

This devlog synthesizes the layering decision that shaped nexus across
three work items: issue #159 (workspace split, devlog 2026-07-01),
issue #172 (storage behavior onto chton surfaces), and issue #176 (the
L2 restructure, devlog 2026-08-20). It also ties in two reference
documents: the blockchain direction draft in the ssccs docs repository
and the `CoordId<20>` migration note. The unifying observation is a
single principle applied at two boundaries: nexus keeps semantics, and
low-level storage and IO move down, to the external chton project at
the crate boundary and to application-owned record maps at the internal
L2 boundary.

## Evidence in the current code

The store surface ownership is visible at the crate boundary.

`nex/fih/src/core/mod.rs` re-exports the entity store family from
chton and documents the ownership split in a comment: the `EntityStore`
trait plus memory and materialized implementations live in
`chton::store` and are re-exported so nexus consumers keep a stable
crate path. The re-export line is
`pub use chton::store::{CoordEntityStore, EntityStore, MapEntityStore, MemoryEntityStore};`.

`nex/fih/src/io/mod.rs` does the same for IO. It re-exports the
CoordMapStore-backed `FileIo` backend from `chton::io` as
`CoordMapStoreIo` and describes itself as a compatibility shim, so
`nex_fih::io::CoordMapStoreIo` resolves without naming chton directly.
The crate-level `lib.rs` forwards these items at the top level.

The chton dependency is a git dependency in the workspace manifest
(`chton = { git = "https://github.com/ssccsorg/chton", branch = "main" }`),
which is the source of the dependency drift cost documented below.

What remains inside nexus is semantics. `nex/fih/src/core/store.rs`
is 1,998 lines and owns the FIH lifecycle: the conflict guard in
`submit_fact` (line 1007), the O(1) occupancy lookup through
`existing_fact_content_hash`, the `fact_to_intents` inverse index
(`intents_by_fact`, line 461), the structural filter index maintained
by `place_record` and `vacate_record` (lines 491 and 589), the
deterministic `rebuild_cache` replay (line 241), and the partition scan
(`scan_partition`, line 1827).

## Historical basis

Devlog 2026-07-01 records the same decision at the workspace level.
Issue #159 removed the petgraph and composite storage crates and split
nexus-model into nex-core, nex-fih, and nex-storage. The recorded
pattern is a USB hub: nexus defines the trait contracts, and concrete
implementations are delegated to external backends such as Chton,
DuckDB, and Spin KV. Issue #172 continues that direction by moving
storage behavior onto chton surfaces (`chton::io`, `chton::store`),
which the current re-export shims complete at the consumer path.

## The L2 restructure as the internal application

The L2 restructure in issue #176 applied the same principle inside
nexus. The unified Tagma tree was reduced to a primitive: a 6-axis
structural filter index over low-cardinality axes (time, entity,
origin, creator, status) mapping to id sets. Record bodies, identity,
and relationships moved to HashMap record maps owned by the
application layer. This is the standard index-to-heap pattern, and it
is the internal mirror of the external chton split. The measured
outcomes support the split: per-fact heap dropped from about 3.4 MB to
about 40 KB average with a steady-state marginal around 0.9 KB, and
the conflict check dropped from 135 to 390 ms per call to 445 ns.
Memory is bounded by axis cardinality, not record count.

## Costs and counterpoints

Three costs balance the principle.

1. Dependency drift. The git-dependency externalization of chton makes
the workspace revision the coordination point. Stale lockfiles in the
app manifests broke three application builds
(nex-calc-fihcontract, nex-api, nex-spinwasi) when an old chton
revision referenced a crate that had been renamed upstream. The fix
was a workspace revision update; the cross-repository coordination
cost is real.

2. Type-level coupling. The public re-exports keep the consumer path
stable under churn, but the coupling itself remains. `nex_fih` still
exposes chton types such as `CoordEntityStore` and `CoordMapStoreIo`
in its public API. A stable path with a bound type is a promise about
churn rate, not an independence from the upstream crate.

3. aether. The aether transport layer is outside the verified scope of
this document. If it is a transport and network layer, it is plausibly
a continuation of the same downward pattern, but the concrete content
is conjecture and is treated as such.

## Documented design decisions

The code review of the L2 restructure left three weaknesses that are
deliberately documented as design decisions.

1. The conflict guard lives in `submit_fact` only. `place_record` is
`pub`, so a direct writer can place a record at an occupied id without
passing the guard. The id-stability requirement is documented on
`place_record` and asserted in debug builds; nex-calc is id-stable
because its id and content hash both derive from the value.

2. `import_into_io` is a documented bulk restore that overwrites. It
is the migration path for whole-storage import and is intentionally
not a conflict-checked per-record submit.

3. The 6-to-20 syllable id change has no automatic migration tool. The
id derivation itself changed, so an in-place tool would need to remap
facts, intents, and hints including `from_facts` references. The
recommended path for pre-1.0 data is re-ingestion through the semantic
layer, documented in `docs/2026-08-20-coord-id-20-migration.md`.

## Ledger-shaped convergence

The blockchain direction draft builds on the same record layer. The
`CoordId<20>` id is a full-injective encoding of the 256-bit digest
into 20 base-11172 coordinates, so two distinct contents collide only
up to SHA-256 collision resistance. `rebuild_cache` replays the record
maps and structural index deterministically, which is the seed of
offline state reconstruction. `scan_partition` gives per-channel data
flows, the seed of multi-channel semantics.

The thin nexus principle constrains the ledger direction in one
respect: the missing parts sit at the protocol level, while the
storage shape is already in place. Consensus, ordering, and an
adversary model live above the record layer, in the same place the FIH
lifecycle lives today. The staged direction in the draft
(deterministic state root, DAG reconciliation, propagation, optional
ordering) is an application-layer program over the record layer, which
is consistent with the storage-down split.

## Judgment

Analytic conclusion: the decision to push low-level storage and IO
down is confirmed by the current code and by the issue #159 devlog.
The semantics-focused shape of nexus follows from that decision, and
the L2 restructure is its internal instance.

Value judgment, hedged: the tradeoff was reasonable in this case. A
layer whose semantics change as quickly as nexus's benefits from a
thin core, and the drift and coupling costs are manageable at the
current scale. Externalization is not universally correct, and the
observation is scoped to this repository and its current stage.

## References

- Devlog 2026-07-01, issue #159: workspace refactoring, petgraph and
  composite removal, nex-core and nex-fih split
- Devlog 2026-08-20, issue #176: content hash conflict detection and
  the L2 restructure
- `docs/2026-08-20-coord-id-20-migration.md`: the breaking id migration
- `docs/projects/nexus/development/blockchain.qmd` in the ssccs docs
  repository: the ledger direction draft
- Issue #172: move nexus storage behavior onto chton surfaces
