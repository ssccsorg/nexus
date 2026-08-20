# 176: content hash conflict detection and the L2 restructure

## Context

Issue #176 started as a hardening bound: nexus record ids carried about
40 bits of content-derived entropy, so two distinct contents could
collide on one id with a 50% birthday probability around 1.5M records.
The work ended as a full review of how nexus uses Tagma, concluding
with a layered (L2) restructure. This devlog is the handoff for that
work: what changed, the measured outcomes, and what remains.

## What was done

The work is on branch `176-content-hash-conflict-detection`, merged to
`main` through eight commits after `3a5a6cdd`.

### Conflict detection

`submit_fact` now rejects a second fact at an occupied id when the
`content_hash` differs, returning `BlackboardError::Conflict` instead of
silently overwriting. Identical content is an idempotent retry. The
occupancy check is an O(1) lookup in the record map, which is kept in
sync by `place_record`, the single chokepoint for the record layer.

### Step 1: content-hash axes removed from the record path

The unified record path dropped its seven content-hash axes
(`CoordPath<19>` to `CoordPath<12>`). The hash axes were pseudo-random,
so each record created about seven unique 89 KB branch nodes while
contributing nothing to spatial filters. The record map became the sole
defender against same-id collisions.

### Step 2: the L2 restructure

The unified tree stopped being a record store. It is now a 6-axis
structural filter index: `CoordPath<6>` (time, entity, origin, creator,
status) maps to the set of record ids at that path. Record bodies live
in HashMap record maps, and the id-keyed entity stores were merged into
those maps. Memory is bounded by axis cardinality, not record count.
The `iter_tree` ascending order contract is preserved.

### Step 3: O(1) reverse lookup

`intents_by_fact` is now an O(fan-out) inverse index
(`fact_to_intents`), maintained by `place_record` and `vacate_record`,
instead of an O(N) scan over all intents.

### Step 4: full-injective ids

Record ids switched to a full-injective `CoordId<20>` encoding.
`content_id` rehashes `content_hash + entity + origin + creator` and
encodes the 256-bit digest into 20 base-11172 coordinates. Twenty is
the minimum depth that holds all 256 bits (19 coords carry only 2^255.5).
Distinct contents collide only up to SHA-256 collision resistance; the
conflict guard stays as defense-in-depth. Ids are canonical 20-Hangul
strings; the old 6-syllable spelling is a breaking change documented in
`docs/2026-08-20-coord-id-20-migration.md`.

## Measured outcomes

The memory probe (`nex/process/tests/memory_probe.rs`, counting
allocator, 16 facts) tracks the per-fact heap cost across the work:

| Stage | Average | Steady-state marginal |
|---|---|---|
| 19-axis tree (before) | 3.40 MB | |
| 12-axis tree (Step 1) | 3.58 MB | |
| 6-axis filter index (Step 2) | 40 KB | ~0.8 KB |
| CoordId<20> ids (Step 4) | 41 KB | ~1.2 KB |
| Entity store merge | 40 KB | ~0.9 KB |

The structural index is a one-time ~650 KB for the probe's structural
space; the per-record cost is under a kilobyte, roughly 4,000x below the
pre-L2 baseline. The 0.5 MB per fact target from the original briefing
is exceeded by a wide margin.

## Architecture conclusion

Tagma is a thin, narrow bottleneck-removing layer: hash latency, scan
cost, and injective collision-free encoding. The unified
everything-in-coordinates direction over-applied it: pseudo-random ids
in a deep tree created per-record 89 KB branch nodes and per-record
leaves. The L2 restructure keeps Tagma where it is good (a structural
filter index over low-cardinality axes) and owns record bodies,
identity, and relationships in the application layer, following the
standard index-to-heap pattern.

## What remains

- The ssccs docs repo carries the whitepaper appendix, the Tagma Map
  insights, and the serialization assessment; that work is committed.
- The optional upstream syntagma change (densify the `CoordSpaceN`
  node encoding) would benefit any application putting random keys in a
  deep tree, but nexus no longer blocks on it.
- No follow-up is tracked for issue #176; it is closed as completed.

## References

- Issue #176: content hash conflict detection
- `docs/2026-08-20-coord-id-20-migration.md`: the breaking id migration
- `nex/process/tests/memory_probe.rs`: memory measurement harness
