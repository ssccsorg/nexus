# 179: real-scenario multi-dimensional search over the structural filter index

## Context

Issue #179 asks whether the `CoordSpaceN<6>` structural filter index
(`iter_prefix` pruning over the leading axes) beats the application-layer
record-map scan (`read_state_filtered` with field predicates) for
multi-dimensional queries, and to record the decision: wire `axis_hints`
pruning, or document the crossover and defer.

The production framing behind the benchmark: an ever-accumulating FIH
knowledge network over classical storage (SQLite, files, object stores)
queried spatio-temporally, origin plus creator plus time range, on local
hardware. The claim under test is that the spatial index stays constant
as the record count grows, so query cost does not degrade with
accumulation. The work is on branch `179-multidim-search-bench`.

## What was done

### Candidate path

`structural_fact_ids` in `nex/fih/src/core/structural.rs` prunes the
candidate id set with `iter_prefix` over `[time_hi, time_lo, entity,
origin, creator]`, unions the id sets, then re-applies the exact
record-field predicates (origin, creator, since, until, fact_ids) on the
authoritative record maps. The origin and creator axes are hash
fingerprints, advisory for ordering only, so the exact string predicates
are mandatory and keep the result identical to the scan path.

Two structural properties bounded the design. The contiguous-prefix
contract of `iter_prefix` means creator can only enter the prefix when
origin is fixed, because origin precedes creator in the axis order. The
leading time axes gate all pruning: without both `since` and `until` the
prefix cannot start at entity, so the path falls back to a full-tree
walk and the exact predicates carry all selectivity.

### Benches and tests

`benches/bench.rs` gained two groups. `fih/multidim_{100k,1m}` uses a
controlled day-bucket fixture (10 days by 10 origins by 10 creators, so
the structural index is identical at every scale) with wide, narrow, and
creator-only query shapes. `fih/kb_lifecycle` ingests the real
docs.ssccs.org/llms.txt manifest (96 documents embedded in
`benches/llms_manifest.rs`) and simulates the FIH lifecycle per
document: fact, intent, claim, conclude, conclusion fact, across three
accumulation phases.

`benches/tests/structural_search.rs` asserts both paths return the
identical id set across selectivity shapes, edge cases, and the
lifecycle store. `benches/tests/memory_footprint.rs` (ignored probe)
measures the structural index footprint across record counts.

## Measured outcomes

All timings are criterion median of 10 samples on Apple M1, release
profile, 2026-08-26. The scan side materializes content and constructs
BoardState; the structural side measures id selection plus record-map
lookups, so the ratios bound the pruning win, and a wired read path
pays materialization for the matched ids and lands between the two.

### Multi-dimensional search at scale

| Query | Scale | Scan | Structural | Gap |
|---|---|---|---|---|
| origin + creator + time, wide | 100k | 1.52 ms | 173 µs | 9x |
| origin + creator + time, narrow | 100k | 952 µs | 25.0 µs | 38x |
| origin + creator + time, wide | 1m | 25.7 ms | 3.24 ms | 8x |
| origin + creator + time, narrow | 1m | 18.3 ms | 226 µs | 81x |
| origin only, wide | 1m | 106 ms | 48.6 ms | 2.2x |
| creator only, wide | 1m | 108 ms | 381 ms | structural slower |

The structural path wins for time-bounded origin-plus-creator filters,
and the gap widens with scale (narrow: 38x at 100k, 81x at 1m). The
origin-only case, the origin-fixed creator-optional half of the decision,
is measured alongside and shows the same direction. The crossover sits
below 100k. The creator-only case is the axis-order limitation: without
origin fixed, the prefix cannot prune creator, the path does a full-tree
walk, and the scan is cheaper.

### Real lifecycle simulation

On the real llms.txt documents with the FIH lifecycle, the three-axis
documents query (origin `projects`, creator `nexus`, time window) is
faster on the structural path by 4.5x at phase 1 and 5.3x at phase 3.
The creator-only query on real data repeats the axis-order limitation:
the structural path takes 2.6 to 3.8 ms against 8 to 15 µs for the
scan.

### Memory footprint

The multidim fixture holds the axis-combo space fixed while the record
count grows:

| Facts | Total live heap | Structural intercept |
|---|---|---|
| 10k | 287 MB | ~278 MB |
| 100k | 354 MB | ~264 MB |
| 1m | 1165 MB | ~264 MB |

The marginal record cost is about 901 bytes per fact, which absorbs the
record maps, the record storage, the structural leaf id-set growth, and
the semantic index; the index is constant in node count, not in bytes.
The linear-fit intercept, about 264 MB for 1000 dense leaves, estimates
the fixed spatial-index cost. The equality of the 100k and 1m intercepts
is a property of the fit, because the per-record delta is derived from
those two points; the 10k intercept differs by about 5% (278 MB). The
direct evidence for the per-axis node cost is the 349 KB per distinct
origin measurement below. The claim holds within the axis-combo
boundary.

The boundary is the axis cardinality, measured directly: each distinct
origin costs about 349 KB (one 268 KB leaf plus one 89 KB branch, plus
the record). Conclusion facts carry unique `conclusion:<intent>`
origins, so a knowledge network whose conclusions accumulate unique
origins inflates the tree linearly in conclusion count. The constant
index is constant in record count, linear in distinct axis values.

## Decision

Wire the structural pruning into the filtered read path only for
filters that can form a leading-axis prefix: both time bounds present
and origin fixed, with creator optional. Keep the record-map scan as
the fallback for every other filter shape (no time bounds, or creator
without origin), where the structural path is strictly slower.

## chton and syntagma connection

The measured 349 KB per distinct origin is the node-density price: a
dense 11,172-slot node costs 89 KB as a branch and 268 KB as a leaf.
For constrained hardware, the densify candidate (sparse node encoding,
noted in the 176 devlog) is the lever; this bench no longer blocks it.
`iter_prefix` costs about 2 µs per prefix at depth 5 (the syntagma suite
reports 1.05 ms per 500 prefixes), so the day-walk cost is O(window
days) prefix navigations at that unit cost, and index maintenance stays
an id-set push per write. Concrete chton and syntagma improvements will be
filed separately once the wiring decision lands.
