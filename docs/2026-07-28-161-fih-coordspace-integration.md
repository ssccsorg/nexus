# 161-fih-coordspace-integration: Integrate FIH storage indexing into Tagma CoordSpace

## Context

PR [#161](https://github.com/ssccsorg/nexus/pull/161) completes the integration of FIH (Fact-Intent-Hint) storage with Tagma's CoordSpace coordinate addressing system. This is the culmination of a structural refactoring arc that spans four PRs and transforms the nexus storage engine from a SHA-256 hash-addressed HashMap system to a deterministic coordinate-addressed CoordSpace system.

### Prior art

| PR | Description | Status |
|----|-------------|--------|
| #153 | Tagma core primitive as workspace dependency (subtree from syntagma) | Merged |
| #156 | Tagma coordinate primitives into FIH storage (CoordRef field, multi-axis index) | Merged |
| #158 | Remove petgraph and composite storage crates | Merged |
| #160 | Split nexus-model into nex-core/nex-fih/nex-io/nex-process | Merged |
| **#161** | **Replace HashMap with CoordSpaceN, FihHash with CoordId, remove FihCoord** | **This PR** |

## What changed

### Phase 1: CoordEntityStore backend swap (1 commit)

- Added `CoordEntityStore<N, V>` implementing `EntityStore<V>` backed by `CoordSpaceN<N, V>`
- Replaced `MemoryEntityStore<FactRecord>` (HashMap) with `CoordEntityStore<6, FactRecord>` in `FihStorage`
- Same for IntentRecord and HintRecord
- All existing tests pass unchanged (EntityStore trait interface preserved)
- String key to CoordPath mapping: 6-char Hangul string → direct Coord lookup; other strings → byte decomposition fallback

**Key insight:** The `EntityStore` trait provided the abstraction boundary. Changing from `MemoryEntityStore` to `CoordEntityStore` required only changing the type in `FihStorage`'s struct definition and constructors. The 800+ lines of `store.rs` logic (submit_fact, submit_intent, read_state, etc.) required zero changes.

### Phase 2: ID migration — FihHash to CoordId (1 commit, 54 files)

- `Fact.id`: `FihHash` (SHA-256, 32 bytes) → `CoordId` (CoordPath<6>, 12 bytes)
- `Fact.coord: Option<CoordRef>` removed (merged into `id`)
- `Fact.content_hash: FihHash` added for content integrity (demoted from primary key)
- `Intent.id`, `Intent.from_facts`, `Intent.to_fact_id`: same migration
- `Hint.id`: same migration
- `CoordRef` renamed to `CoordId` throughout
- All trait return types: `Result<FihHash, _>` → `Result<CoordId, _>`

**Impact:** 54 files touched across the entire workspace — nex-fih, nex-process, nex-api, nex-calc, nex-calc-fihcontract, nex-spinwasi-ssccsdocs, storage-duckdb, serde-proxy, and all test files. The `Fact::new(id, origin, content, creator)` convenience constructor automatically computes `content_hash` from content, minimizing call-site changes.

### Phase 3: FihCoord removal (1 commit)

- Removed `FihCoord` struct (~470 lines, 9 index data structures) from `core/index.rs`
- Replaced with 3 focused fast-path lookup tables in `FihStorage`:
  - `fact_by_origin: Cell2<HashMap<String, HashSet<String>>>`
  - `fact_by_creator: Cell2<HashMap<String, HashSet<String>>>`
  - `intent_by_status: Cell2<HashMap<String, HashSet<String>>>`
- `semantic_stores` moved directly to `FihStorage` (no longer through FihCoord)
- `read_state_filtered()` rewritten to use fast-path tables instead of FihCoord index queries
- `rebuild_coord()` → `rebuild_fastpath()` (rebuilds lookup tables from entity stores)
- `time_range()`, `flush_since()` simplified (coord-based delta tracking removed)
- `intents_by_fact()` scans intent_store instead of using by_fact index (O(fan-out) acceptable for < 100 intents/fact)

**Key insight:** CoordSpaceN is self-indexing. The 9 separate index structures (by_origin, by_creator, by_status, by_time, by_fact, ref_counts, by_semantic, axis_index, string_interner) existed because HashMap has no structural index. CoordSpaceN's tree structure inherently provides O(1) path lookup — the path IS the index. The fast-path tables are only needed for `read_state_filtered()` queries that filter by origin/creator/status without knowing the CoordPath.

### Post-migration fixes (3 commits)

- Clippy warnings: needless_range_loop, needless_borrow, match_single_binding, unnecessary_cast
- `apps/nex-api/src/routes.rs`: Fact/Intent struct literals → constructor calls, FihHash → CoordId
- `apps/nex-calc-fihcontract/src/engine.rs`: Full type migration (FihHash → CoordId in all function signatures)
- `apps/nex-spinwasi-ssccsdocs/src/lib.rs`: Struct literals → constructors, rebuild_coord → rebuild_cache
- `playbooks/consumers/python_agent.py`: URL-encode Hangul CoordId in HTTP paths (ASCII requirement)

## Architecture after PR #161

```
FihStorage<I: FileIo>
  ├── fact_store:   CoordEntityStore<6, FactRecord>    ← CoordSpaceN<6, FactRecord>
  ├── intent_store: CoordEntityStore<6, IntentRecord>   ← CoordSpaceN<6, IntentRecord>
  ├── hint_store:   CoordEntityStore<6, HintRecord>     ← CoordSpaceN<6, HintRecord>
  ├── fact_by_origin: HashMap<String, HashSet<String>>  ← fast-path filter lookup
  ├── fact_by_creator: HashMap<String, HashSet<String>> ← fast-path filter lookup
  ├── intent_by_status: HashMap<String, HashSet<String>>← fast-path filter lookup
  ├── semantic_stores: Vec<DynSemanticStore>            ← BM25 / vector search
  └── pending: Vec<WriteOp>                             ← batch IO flush
```

**Removed:**
- `MemoryEntityStore` (HashMap backend) — replaced by CoordSpaceN
- `FihCoord` (9-index monolith) — replaced by self-indexing CoordSpaceN + 3 small lookups
- `StringInterner` — no longer needed (CoordPath handles string→address directly)
- `OrderedIndex<u64>` (by_time) — CoordPath top-level coord covers time
- `SHA-256` as address generation — demoted to content_hash only
- `FihHash` as primary key — replaced by CoordId (CoordPath<6>)

## Performance characteristics (supported by synTagma benchmarks)

| Operation | Before (HashMap + SHA-256) | After (CoordSpaceN + CoordId) | Improvement |
|-----------|---------------------------|-------------------------------|-------------|
| Point lookup | SHA-256 (227 ns) + HashMap | CoordPath + CoordSpaceN (0.39 ns) | ~582x |
| Nonexistent key | HashMap full scan (23 ms at 10M) | Structural None (1.65 ns) | ~14Mx |
| Spatial/proximity | Not supported | CoordCube (285 ns at 10M) | New |
| Index maintenance | 9 structures per write | 0 (CoordSpaceN is self-indexing) | Eliminated |
| Memory | Per-entry allocation + rehashing | Per-prefix tree nodes (fixed) | Predictable |

## Key decisions

### Why CoordPath depth 6?

11,172^6 ≈ 1.94 × 10^24 identifiers. This exceeds UUID space and all practical workload requirements (individual, team, and IoT-scale deployments). The CoordSpaceN tree allocates nodes lazily (44 KB per used prefix), so depth 6 does not imply 6× memory overhead — memory scales with actual entry count, not depth.

### Why keep fast-path lookups instead of pure CoordSpace axis filters?

`read_state_filtered()` needs to answer "facts by creator" and "intents by status" efficiently. With only 6 CoordPath axes (time, time-fine, entity-type, origin, creator, serial), axis-based filtering without knowing the full path prefix requires a tree scan. The fast-path HashMaps provide O(1) lookups for these common filter patterns with negligible memory overhead (one String→HashSet<String> entry per unique origin/creator/status value). These tables can be collapsed into CoordSpace axis operations in a future phase once CoordPath axis semantics are fully established.

### Why keep semantic stores separate?

BM25 and vector similarity search operate on content, not on structural coordinates. The semantic stores are plug-in components that index content independently of CoordSpace. This separation is intentional — semantic search and structural search are complementary, not competing.

## Benchmark results (10K facts, SimIo)

| Test | OLD (ssccs-nexus2) | NEW (PR #161) | Improvement |
|------|-------------------|---------------|-------------|
| write 10K | 230 ms | 34.6 ms | **6.6x faster** |
| read_state 10K | 21 ms | 23.6 ms | ~equivalent |
| filter creator 10x | 100 ms | 3.1 ms | **32x faster** |
| filter origin+creator | (no filter) | 5.2 ms | New capability |

Key insight: all improvement comes from removing FihCoord's 9-index maintenance. postcard + pending Vec overhead is identical between old and new.

## Testing

- Full workspace: `cargo test --workspace --exclude nexd` — all pass
- nexd integration tests: 27 scenarios — all pass
- nex-api HTTP integration tests: 5 scenarios — all pass
- nex-calc: 38 tests — all pass
- nex-process: 38 integration tests — all pass
- Clippy: `cargo clippy --workspace -- -D warnings` — clean

## What's next

### Chton (mmap persistence layer)

With CoordPath depth (N=6), FactRecord layout, and EntityStore interface all finalized, Chton can implement `FileIo` over mmap. A `ChtonIo` struct implementing the `FileIo` trait would allow `FihStorage<ChtonIo>` to compile immediately. Full benefits (zero-serialize, WAL-free crash recovery) require deeper integration.

### Rem dependency update

`rem/Cargo.toml` still references `nexus-model`, `nexus-storage-composite`, and `nexus-storage-petgraph` — all removed from the workspace. These must be updated to `nex-core` + `nex-fih` + `nex-io`.

### CoordPath axis convention

The 6 CoordPath axes currently have no formal semantic mapping (time, entity-type, origin, etc.). Establishing and documenting this convention will enable axis-based filtering directly on CoordSpaceN, potentially replacing the fast-path HashMaps.

## References

- [synTagma brief](https://docs.ssccs.org/projects/syntagma) — Tagma coordinate space primer
- [Tagma whitepaper](https://docs.ssccs.org/projects/syntagma/tagma/) — Benchmarks and hardware design
- [TagmaMap](https://docs.ssccs.org/projects/syntagma/tagma/map/) — Key-value store on coordinate primitives
- [Tagma-Geo](https://docs.ssccs.org/projects/syntagma/tagma/geo/) — Spatial query layer (CoordCube)
- [Chton pager](https://docs.ssccs.org/projects/ct/) — Persistent IO layer for spatial computing
