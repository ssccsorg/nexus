// ── Structural filter index search (issue #179) ────────────────────────
//
// Candidate multi-axis fact search over the CoordSpaceN<6> structural
// filter index. The baseline `read_state_filtered` scans the
// application-layer record maps (HashMap) with record-field predicates;
// this path prunes the candidate id set with `iter_prefix` over the
// leading axes and then re-applies the exact predicates on
// materialization, so both paths return identical result sets.
//
// Axis layout (`structural_path`): [0]=time_hi, [1]=time_lo, [2]=entity,
// [3]=origin, [4]=creator, [5]=status. Origin and creator axes are hash
// fingerprints (advisory ordering only), so the exact string predicates
// are mandatory: a fingerprint collision can place a record under a
// candidate path whose record fields do not match the filter, and the
// re-filter drops it. Time axes enter the prefix only when both `since`
// and `until` are present; without a bounded window the path falls back
// to a full-tree walk (see below). `limit` and `offset` are not applied
// by this method; callers page over the returned ids if needed.
//
// The contiguous-prefix property of `iter_prefix` bounds pruning power
// by axis order: creator can only enter the prefix when origin is fixed,
// because origin sits before creator in the path. The leading time axes
// gate all pruning: without both `since` and `until` the prefix cannot
// start at entity (a `[entity]` prefix would select time_hi=0 instead),
// so the path falls back to a full-tree walk and the exact predicates
// carry all selectivity.

use tagma_core::Coord;

use crate::core::store::{FihStorage, hash_str};
use crate::io::file_io::FileIo;
use crate::{CoordId, StateFilter};

/// Nanoseconds in one day (the structural time axis is day-granular).
const DAY_NS: u64 = 86_400_000_000_000;
/// Valid coordinate slot count (Coord::N_VALID).
const N_SLOTS: u64 = 11_172;

impl<I: FileIo> FihStorage<I> {
    /// Fact ids matching `filter` via the structural filter index.
    ///
    /// Prunes with `iter_prefix` on the leading axes [time_hi, time_lo,
    /// entity, origin, creator], unions the id sets, then re-applies the
    /// exact record-field predicates (origin, creator, since, until,
    /// fact_ids) on the authoritative record maps. Returns sorted
    /// canonical ids. `limit` and `offset` are not applied; the caller
    /// pages over the returned ids if needed.
    pub fn structural_fact_ids(&self, filter: &StateFilter) -> Vec<String> {
        let since: Option<u64> = filter.since.as_ref().and_then(|s| s.parse().ok());
        let until: Option<u64> = filter.until.as_ref().and_then(|s| s.parse().ok());
        let origin_v = filter.origin.as_deref().map(hash_str);
        let creator_v = filter.creator.as_deref().map(hash_str);

        // Ids are duplicate-free by construction through the submit paths
        // (each id lives at exactly one structural path, and place_record
        // dedups within a path); sort plus dedup below is the safety net for
        // the documented direct-writer edge. A Vec avoids the hashing and
        // allocation cost of a set, which dominates the no-pruning fallback.
        let mut candidates: Vec<String> = Vec::new();
        match (since, until) {
            (Some(s), Some(u)) => {
                // Bounded day walk: each day contributes the subtree at
                // that (time, entity, origin, creator) prefix. The exact
                // ns predicates below trim within the day. The walk width
                // is the window in days, so an open-ended `until` (for
                // example u64::MAX) runs about 213k navigations; callers
                // with unbounded ranges should use the record-map scan.
                let store = self.store.borrow();
                let mut day = s / DAY_NS;
                let last = u / DAY_NS;
                while day <= last {
                    collect_day_ids(&store, day, origin_v, creator_v, &mut candidates);
                    day += 1;
                }
            }
            _ => {
                // No bounded time window: the leading time axes cannot
                // form a prefix (a prefix starting at entity would select
                // the time_hi=0 subtree, not all times), so fall back to
                // a full-tree walk and let the exact predicates carry all
                // selectivity. The pruning win exists only for
                // time-bounded queries.
                let store = self.store.borrow();
                if let Some(iter) = store.iter_prefix(&[]) {
                    for (_path, ids) in iter {
                        candidates.extend(ids.iter().cloned());
                    }
                }
            }
        }

        // Exact re-filter on the authoritative record layer.
        let recs = self.fact_records.borrow();
        // Normalize the explicit fact-id filter once: the structural index
        // stores canonical ids, and resolve() derives a canonical id from a
        // label, so pre-normalizing keeps the per-candidate comparison a
        // plain set membership test instead of repeated string derivation.
        let wanted_ids: Option<Vec<String>> = filter.fact_ids.as_ref().map(|ids| {
            let mut v: Vec<String> = ids
                .iter()
                .map(|x| CoordId::resolve(x).to_string())
                .collect();
            v.sort();
            v.dedup();
            v
        });
        let mut out: Vec<String> = candidates
            .into_iter()
            .filter(|id| {
                let Some(r) = recs.get(id) else {
                    return false;
                };
                if let Some(ref want) = filter.origin
                    && &r.origin != want
                {
                    return false;
                }
                if let Some(ref want) = filter.creator
                    && &r.creator != want
                {
                    return false;
                }
                if let Some(ts) = since
                    && r.submitted_at < ts
                {
                    return false;
                }
                if let Some(ts) = until
                    && r.submitted_at > ts
                {
                    return false;
                }
                if let Some(wanted) = wanted_ids.as_ref() {
                    let canonical = CoordId::resolve(id).to_string();
                    if !wanted.iter().any(|x| x == &canonical) {
                        return false;
                    }
                }
                true
            })
            .collect();
        // Dedup guards the documented direct-writer edge where the same id
        // could be placed at two paths; the submit paths never produce one.
        out.sort();
        out.dedup();
        out
    }
}

/// Union the id sets under the day/time/entity/origin/creator prefix.
fn collect_day_ids(
    store: &tagma_core::CoordSpaceN<6, Vec<String>>,
    day: u64,
    origin_v: Option<u16>,
    creator_v: Option<u16>,
    out: &mut Vec<String>,
) {
    let Some(iter) = store.iter_prefix(&fact_prefix(day, origin_v, creator_v)) else {
        return;
    };
    for (_path, ids) in iter {
        out.extend(ids.iter().cloned());
    }
}

/// Contiguous leading-axis prefix for facts under the given day and
/// fixed origin/creator fingerprints. The creator axis can only follow
/// the origin axis, so creator is pruned only when origin is fixed too.
fn fact_prefix(day: u64, origin_v: Option<u16>, creator_v: Option<u16>) -> Vec<Coord> {
    let mut prefix = Vec::with_capacity(5);
    let hi = ((day / N_SLOTS) % N_SLOTS) as u16;
    let lo = (day % N_SLOTS) as u16;
    prefix.push(Coord::new(hi).expect("time_hi coord"));
    prefix.push(Coord::new(lo).expect("time_lo coord"));
    // Entity axis: fact = 0.
    prefix.push(Coord::new(0).expect("entity coord"));
    if let Some(o) = origin_v {
        prefix.push(Coord::new(o).expect("origin coord"));
        if let Some(c) = creator_v {
            prefix.push(Coord::new(c).expect("creator coord"));
        }
    }
    prefix
}
