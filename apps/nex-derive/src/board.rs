// The board: goals as points in the work x intent x contract product space.
//
// A derivation records that a goal yielded a fact, so the board holds one
// derivation per goal. The direction lives in the record: the goal is the
// premise, the fact is the conclusion. Resolution of a conjunctive goal is the
// inner step and is symmetric; the record is what carries the direction.
//
// Three structures share one coordinate index space:
//
//   occupancy      the goals that hold a derivation
//   postings       one bitset per (axis, value): the goals carrying it
//   slots          goal coordinate to derivation index, for reading it back
//
// A conjunctive goal intersects the postings it names. The result is exact,
// not a candidate set, because every posting is indexed by the same space:
// there is no cross-axis false positive to filter afterwards.

use std::time::{Duration, Instant};

use crate::axis::{Axis, Names, pack};
use crate::bits::Bits;
use crate::layout::{AXES, AXIS_CARD, COORD_SPACE};
use crate::query::{Condition, Query};

const EMPTY_SLOT: u32 = u32::MAX;

/// A goal resolved to a fact. The goal is `(work, intent, hint)`; the fact is
/// the conclusion recorded for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derivation {
    pub work: u8,
    pub intent: u8,
    pub hint: u8,
    pub fact: String,
}

impl Derivation {
    pub fn goal(&self) -> [u8; AXES] {
        [self.work, self.intent, self.hint]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveOutcome {
    Derived,
    /// The goal already held a derivation; the call changed nothing.
    AlreadyDerived,
}

pub struct Board {
    names: Names,
    derivations: Vec<Derivation>,
    slots: Vec<u32>,
    occupancy: Bits,
    postings: Vec<Bits>,
}

impl Board {
    pub fn new() -> Self {
        let mut postings = Vec::with_capacity(AXES * AXIS_CARD);
        for _ in 0..AXES * AXIS_CARD {
            postings.push(Bits::new());
        }
        Self {
            names: Names::new(),
            derivations: Vec::new(),
            slots: vec![EMPTY_SLOT; COORD_SPACE],
            occupancy: Bits::new(),
            postings,
        }
    }

    pub fn names(&self) -> &Names {
        &self.names
    }

    pub fn names_mut(&mut self) -> &mut Names {
        &mut self.names
    }

    pub fn derivations(&self) -> &[Derivation] {
        &self.derivations
    }

    pub fn len(&self) -> usize {
        self.derivations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.derivations.is_empty()
    }

    /// Records that `goal` derives `fact`. One derivation per goal.
    pub fn derive(&mut self, work: u8, intent: u8, hint: u8, fact: String) -> DeriveOutcome {
        let coord = pack([work, intent, hint]);
        if self.occupancy.test(coord) {
            return DeriveOutcome::AlreadyDerived;
        }
        let index = self.derivations.len() as u32;
        self.slots[coord] = index;
        self.occupancy.set(coord);
        for (axis, value) in axes_of(work, intent, hint) {
            self.postings[posting_index(axis, value)].set(coord);
        }
        self.derivations.push(Derivation {
            work,
            intent,
            hint,
            fact,
        });
        DeriveOutcome::Derived
    }

    pub fn derivation(&self, coord: usize) -> Option<&Derivation> {
        let index = *self.slots.get(coord)?;
        if index == EMPTY_SLOT {
            None
        } else {
            self.derivations.get(index as usize)
        }
    }

    /// The goals allowed by every condition of the query, as a bitset.
    pub fn intersection(&self, query: &Query) -> Bits {
        let mut acc = self.occupancy.clone();
        for condition in &query.conditions {
            let Some(bits) = self.condition_bits(condition) else {
                continue;
            };
            acc.and_assign(&bits);
            if acc.is_empty() {
                break;
            }
        }
        acc
    }

    /// Resolves `query` to the derivations whose goal satisfies every
    /// condition, as indices into `derivations`, in ascending order. The order
    /// matches `scan`, so the two paths compare directly.
    pub fn resolve(&self, query: &Query) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .intersection(query)
            .iter()
            .map(|coord| self.slots[coord] as usize)
            .collect();
        indices.sort_unstable();
        indices
    }

    /// The naive reference: test every derivation against the query. Cost
    /// grows with the derivation count; the resolution's cost does not.
    pub fn scan(&self, query: &Query) -> Vec<usize> {
        self.derivations
            .iter()
            .enumerate()
            .filter(|(_, derivation)| query.matches(derivation.goal()))
            .map(|(index, _)| index)
            .collect()
    }

    /// The candidate count after each condition is applied in turn. The
    /// sequence is non-increasing.
    pub fn funnel(&self, query: &Query) -> Vec<(Option<Axis>, usize)> {
        let mut acc = self.occupancy.clone();
        let mut steps = vec![(None, acc.count())];
        for condition in &query.conditions {
            if let Some(bits) = self.condition_bits(condition) {
                acc.and_assign(&bits);
            }
            steps.push((Some(condition.axis), acc.count()));
        }
        steps
    }

    /// The posting union for a condition, or `None` when it narrows nothing.
    fn condition_bits(&self, condition: &Condition) -> Option<Bits> {
        if condition.is_unconstrained() {
            return None;
        }
        let base = condition.axis.index() * AXIS_CARD;
        // A single allowed value is the common case and needs no union: the
        // posting is used directly, so the cost is one copy and one AND.
        if condition.count() == 1 {
            let value = condition.allow.trailing_zeros() as usize;
            return Some(self.postings[base + value].clone());
        }
        let mut bits = Bits::new();
        let mut allowed = condition.allow;
        while allowed != 0 {
            let value = allowed.trailing_zeros() as usize;
            bits.or_assign(&self.postings[base + value]);
            allowed &= allowed - 1;
        }
        Some(bits)
    }

    /// Fills a fresh board with `n` distinct goals drawn from a deterministic
    /// stream, then times the resolution against the scan over the same
    /// queries.
    pub fn bench(n: usize, queries: usize, seed: u64) -> BenchLine {
        let mut board = Board::new();
        let target = n.min(COORD_SPACE);
        let mut state = seed;
        while board.len() < target {
            let work = (splitmix64(&mut state) % AXIS_CARD as u64) as u8;
            let intent = (splitmix64(&mut state) % AXIS_CARD as u64) as u8;
            let hint = (splitmix64(&mut state) % AXIS_CARD as u64) as u8;
            board.derive(work, intent, hint, String::new());
        }

        let queries: Vec<Query> = (0..queries.max(1))
            .map(|_| random_query(&mut state))
            .collect();

        let mut resolved = Vec::with_capacity(queries.len());
        let intersection_start = Instant::now();
        for query in &queries {
            resolved.push(board.intersection(query).count());
        }
        let intersection = intersection_start.elapsed();

        // The scan is measured against the resolution counts so the comparison
        // is timed on its own, and so the two paths are checked for agreement
        // in the same pass.
        let mut agree = true;
        let scan_start = Instant::now();
        for (query, expected) in queries.iter().zip(&resolved) {
            if board.scan(query).len() != *expected {
                agree = false;
            }
        }
        let scan = scan_start.elapsed();

        BenchLine {
            n: board.len(),
            queries: queries.len(),
            matched: resolved.iter().sum(),
            agree,
            intersection,
            scan,
        }
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct BenchLine {
    pub n: usize,
    pub queries: usize,
    pub matched: usize,
    pub agree: bool,
    pub intersection: Duration,
    pub scan: Duration,
}

fn axes_of(work: u8, intent: u8, hint: u8) -> [(Axis, u8); AXES] {
    [
        (Axis::Work, work),
        (Axis::Intent, intent),
        (Axis::Hint, hint),
    ]
}

fn posting_index(axis: Axis, value: u8) -> usize {
    axis.index() * AXIS_CARD + value as usize
}

/// SplitMix64: a small deterministic stream, so runs reproduce without a
/// random-number dependency.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn random_query(state: &mut u64) -> Query {
    let mut conditions = Vec::new();
    for axis in Axis::ALL {
        // A third of the axes are left unconstrained, which exercises the
        // skip path in `condition_bits`.
        if splitmix64(state).is_multiple_of(3) {
            continue;
        }
        let value = (splitmix64(state) % AXIS_CARD as u64) as u8;
        conditions.push(Condition::single(axis, value));
    }
    Query::new(conditions)
}
