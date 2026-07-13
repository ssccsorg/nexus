use crate::coord::Coord;

/// A fixed-size, collision-free, memory-optimized bit array for presence
/// checking of [`Coord`] values.
///
/// Backed by a 175-element `u64` array (11,172 bits ≈ 1.4 KB).  No heap
/// allocation, no hashing, no collisions — and fully `no_std` compatible
/// without an allocator.
///
/// # Operations
///
/// | Operation | Implementation |
/// |-----------|----------------|
/// | `insert` / `remove` / `contains` | Single bit test/set |
/// | `union` | Bitwise OR — O(175) |
/// | `intersection` | Bitwise AND — O(175) |
/// | `difference` | Bitwise AND NOT — O(175) |
/// | `iter` | Scan set bits — O(11172) |
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CoordSet {
    bits: [u64; Self::WORD_COUNT],
}

impl CoordSet {
    const BITS: usize = Coord::N_VALID; // 11172
    const WORD_BITS: usize = u64::BITS as usize; // 64
    const WORD_COUNT: usize = Self::BITS.div_ceil(Self::WORD_BITS); // 175

    #[inline]
    fn word_bit(coord: Coord) -> (usize, u64) {
        let idx = coord.index() as usize;
        (idx / Self::WORD_BITS, 1u64 << (idx % Self::WORD_BITS))
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl CoordSet {
    /// Creates an empty `CoordSet`.
    #[inline]
    pub const fn new() -> Self {
        CoordSet {
            bits: [0u64; Self::WORD_COUNT],
        }
    }
}

impl Default for CoordSet {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

impl CoordSet {
    /// Inserts `coord` into the set.
    ///
    /// Returns `true` if `coord` was not already present.
    #[inline]
    pub fn insert(&mut self, coord: Coord) -> bool {
        let (w, b) = Self::word_bit(coord);
        let old = self.bits[w];
        self.bits[w] = old | b;
        old & b == 0
    }

    /// Removes `coord` from the set.
    ///
    /// Returns `true` if `coord` was present.
    #[inline]
    pub fn remove(&mut self, coord: Coord) -> bool {
        let (w, b) = Self::word_bit(coord);
        let old = self.bits[w];
        self.bits[w] = old & !b;
        old & b != 0
    }

    /// Returns `true` if `coord` is in the set.
    #[inline]
    pub fn contains(&self, coord: Coord) -> bool {
        let (w, b) = Self::word_bit(coord);
        self.bits[w] & b != 0
    }

    /// Clears all elements from the set.
    #[inline]
    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    /// Returns the number of elements in the set (popcount).
    pub fn len(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Returns `true` if the set contains no elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&w| w == 0)
    }

    /// The maximum number of elements the set can hold (always 11 172).
    #[inline]
    pub const fn capacity(&self) -> usize {
        Coord::N_VALID
    }

    /// Returns a reference to the coordinate if present (mirrors `HashSet::get`).
    #[inline]
    pub fn get<'a>(&self, coord: &'a Coord) -> Option<&'a Coord> {
        if self.contains(*coord) {
            Some(coord)
        } else {
            None
        }
    }

    /// Removes and returns the coordinate if present (mirrors `HashSet::take`).
    #[inline]
    pub fn take(&mut self, coord: &Coord) -> Option<Coord> {
        if self.remove(*coord) {
            Some(*coord)
        } else {
            None
        }
    }

    /// Retains only the coordinates satisfying the predicate.
    pub fn retain<F: FnMut(&Coord) -> bool>(&mut self, mut f: F) {
        for i in 0..Coord::N_VALID {
            let coord = Coord::new(i as u16).unwrap();
            if self.contains(coord) && !f(&coord) {
                self.remove(coord);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Set operations
// ---------------------------------------------------------------------------

impl CoordSet {
    /// Returns the union of `self` and `other` (elements in either set).
    #[inline]
    pub fn union(&self, other: &Self) -> Self {
        Self::from_bitwise(self, other, |a, b| a | b)
    }

    /// Returns the intersection of `self` and `other` (elements in both).
    #[inline]
    pub fn intersection(&self, other: &Self) -> Self {
        Self::from_bitwise(self, other, |a, b| a & b)
    }

    /// Returns the difference `self \ other` (elements in `self` but not `other`).
    #[inline]
    pub fn difference(&self, other: &Self) -> Self {
        Self::from_bitwise(self, other, |a, b| a & !b)
    }

    /// Returns the symmetric difference (elements in exactly one of the two sets).
    #[inline]
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        Self::from_bitwise(self, other, |a, b| a ^ b)
    }

    /// Returns `true` if `self` is a subset of `other`.
    #[inline]
    pub fn is_subset(&self, other: &Self) -> bool {
        self.bits
            .iter()
            .zip(&other.bits)
            .all(|(&a, &b)| a & !b == 0)
    }

    /// Returns `true` if `self` is a superset of `other`.
    #[inline]
    pub fn is_superset(&self, other: &Self) -> bool {
        other.is_subset(self)
    }

    /// Returns `true` if the sets have no elements in common.
    #[inline]
    pub fn is_disjoint(&self, other: &Self) -> bool {
        self.bits.iter().zip(&other.bits).all(|(&a, &b)| a & b == 0)
    }

    #[inline]
    fn from_bitwise<F: FnMut(u64, u64) -> u64>(a: &Self, b: &Self, mut op: F) -> Self {
        let mut bits = [0u64; Self::WORD_COUNT];
        for (out, (wa, wb)) in bits
            .iter_mut()
            .zip(a.bits.iter().zip(&b.bits))
            .take(Self::WORD_COUNT)
        {
            *out = op(*wa, *wb);
        }
        CoordSet { bits }
    }
}

// ---------------------------------------------------------------------------
// Iteration
// ---------------------------------------------------------------------------

/// An iterator over the elements of a `CoordSet`.
pub struct Iter {
    bits: [u64; CoordSet::WORD_COUNT],
    word_idx: usize,
}

impl Iterator for Iter {
    type Item = Coord;

    fn next(&mut self) -> Option<Self::Item> {
        while self.word_idx < CoordSet::WORD_COUNT {
            let w = self.bits[self.word_idx];
            if w != 0 {
                let bit = w.trailing_zeros();
                let idx = (self.word_idx * CoordSet::WORD_BITS) + bit as usize;
                self.bits[self.word_idx] = w & (w - 1); // clear lowest set bit
                return Some(Coord::new(idx as u16).unwrap());
            }
            self.word_idx += 1;
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(CoordSet::BITS))
    }
}

impl CoordSet {
    /// An iterator over all coordinates in the set, in index order.
    #[inline]
    pub fn iter(&self) -> Iter {
        Iter {
            bits: self.bits,
            word_idx: 0,
        }
    }
}

impl IntoIterator for &CoordSet {
    type Item = Coord;
    type IntoIter = Iter;

    #[inline]
    fn into_iter(self) -> Iter {
        self.iter()
    }
}

// ---------------------------------------------------------------------------
// FromIterator
// ---------------------------------------------------------------------------

impl FromIterator<Coord> for CoordSet {
    fn from_iter<I: IntoIterator<Item = Coord>>(iter: I) -> Self {
        let mut set = Self::new();
        for coord in iter {
            set.insert(coord);
        }
        set
    }
}

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

impl core::ops::Index<Coord> for CoordSet {
    type Output = bool;

    #[inline]
    fn index(&self, coord: Coord) -> &bool {
        if self.contains(coord) {
            &true
        } else {
            &false
        }
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl core::fmt::Display for CoordSet {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{{")?;
        for (i, coord) in self.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", coord)?;
        }
        write!(f, "}}")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn new_set_is_empty() {
        let set = CoordSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn insert_and_contains() {
        let mut set = CoordSet::new();
        let c = Coord::new(0).unwrap();
        assert!(!set.contains(c));
        assert!(set.insert(c));
        assert!(set.contains(c));
    }

    #[test]
    fn insert_duplicate() {
        let mut set = CoordSet::new();
        let c = Coord::new(0).unwrap();
        assert!(set.insert(c));
        assert!(!set.insert(c)); // second insert returns false
    }

    #[test]
    fn remove() {
        let mut set = CoordSet::new();
        let c = Coord::new(0).unwrap();
        set.insert(c);
        assert!(set.remove(c));
        assert!(!set.contains(c));
        assert!(!set.remove(c)); // second remove returns false
    }

    #[test]
    fn len() {
        let mut set = CoordSet::new();
        for i in 0u16..50 {
            set.insert(Coord::new(i).unwrap());
        }
        assert_eq!(set.len(), 50);
    }

    #[test]
    fn clear() {
        let mut set = CoordSet::new();
        set.insert(Coord::new(0).unwrap());
        set.insert(Coord::new(100).unwrap());
        set.clear();
        assert!(set.is_empty());
    }

    #[test]
    fn union_basic() {
        let mut a = CoordSet::new();
        let mut b = CoordSet::new();
        a.insert(Coord::new(0).unwrap());
        b.insert(Coord::new(1).unwrap());
        let u = a.union(&b);
        assert!(u.contains(Coord::new(0).unwrap()));
        assert!(u.contains(Coord::new(1).unwrap()));
    }

    #[test]
    fn intersection_basic() {
        let mut a = CoordSet::new();
        let mut b = CoordSet::new();
        a.insert(Coord::new(0).unwrap());
        a.insert(Coord::new(1).unwrap());
        b.insert(Coord::new(1).unwrap());
        b.insert(Coord::new(2).unwrap());
        let i = a.intersection(&b);
        assert!(!i.contains(Coord::new(0).unwrap()));
        assert!(i.contains(Coord::new(1).unwrap()));
        assert!(!i.contains(Coord::new(2).unwrap()));
    }

    #[test]
    fn difference_basic() {
        let mut a = CoordSet::new();
        let mut b = CoordSet::new();
        a.insert(Coord::new(0).unwrap());
        a.insert(Coord::new(1).unwrap());
        b.insert(Coord::new(1).unwrap());
        let d = a.difference(&b);
        assert!(d.contains(Coord::new(0).unwrap()));
        assert!(!d.contains(Coord::new(1).unwrap()));
    }

    #[test]
    fn symmetric_difference() {
        let mut a = CoordSet::new();
        let mut b = CoordSet::new();
        a.insert(Coord::new(0).unwrap());
        b.insert(Coord::new(1).unwrap());
        let sd = a.symmetric_difference(&b);
        assert!(sd.contains(Coord::new(0).unwrap()));
        assert!(sd.contains(Coord::new(1).unwrap()));
        // Both have it → not in symmetric diff
        a.insert(Coord::new(2).unwrap());
        b.insert(Coord::new(2).unwrap());
        let sd2 = a.symmetric_difference(&b);
        assert!(!sd2.contains(Coord::new(2).unwrap()));
    }

    #[test]
    fn subset() {
        let mut a = CoordSet::new();
        let mut b = CoordSet::new();
        a.insert(Coord::new(0).unwrap());
        a.insert(Coord::new(1).unwrap());
        b.insert(Coord::new(0).unwrap());
        b.insert(Coord::new(1).unwrap());
        b.insert(Coord::new(2).unwrap());
        assert!(a.is_subset(&b));
        assert!(!b.is_subset(&a));
    }

    #[test]
    fn superset() {
        let mut a = CoordSet::new();
        let mut b = CoordSet::new();
        a.insert(Coord::new(0).unwrap());
        a.insert(Coord::new(1).unwrap());
        a.insert(Coord::new(2).unwrap());
        b.insert(Coord::new(0).unwrap());
        b.insert(Coord::new(1).unwrap());
        assert!(a.is_superset(&b));
        assert!(!b.is_superset(&a));
    }

    #[test]
    fn disjoint() {
        let mut a = CoordSet::new();
        let mut b = CoordSet::new();
        a.insert(Coord::new(0).unwrap());
        b.insert(Coord::new(1).unwrap());
        assert!(a.is_disjoint(&b));
        b.insert(Coord::new(0).unwrap());
        assert!(!a.is_disjoint(&b));
    }

    #[test]
    fn iter_empty() {
        let set = CoordSet::new();
        assert_eq!(set.iter().count(), 0);
    }

    #[test]
    fn iter_non_empty() {
        let mut set = CoordSet::new();
        set.insert(Coord::new(0).unwrap());
        set.insert(Coord::new(11171).unwrap());
        let v: Vec<_> = set.iter().collect();
        assert_eq!(v.len(), 2);
        assert!(v.contains(&Coord::new(0).unwrap()));
        assert!(v.contains(&Coord::new(11171).unwrap()));
    }

    #[test]
    fn into_iter() {
        let mut set = CoordSet::new();
        set.insert(Coord::new(5).unwrap());
        let v: Vec<_> = (&set).into_iter().collect();
        assert_eq!(v, vec![Coord::new(5).unwrap()]);
    }

    #[test]
    fn from_iterator() {
        let coords: Vec<_> = (0..10u16).map(|i| Coord::new(i).unwrap()).collect();
        let set: CoordSet = coords.into_iter().collect();
        assert_eq!(set.len(), 10);
    }

    #[test]
    fn index_trait() {
        let mut set = CoordSet::new();
        let c = Coord::new(7).unwrap();
        assert!(!set[c]); // Index<bool> returns &bool
        set.insert(c);
        assert!(set[c]);
    }

    #[test]
    fn fill_all() {
        let mut set = CoordSet::new();
        for i in 0u16..11172 {
            set.insert(Coord::new(i).unwrap());
        }
        assert_eq!(set.len(), 11172);
        assert!(!set.is_empty());
        for i in 0u16..11172 {
            assert!(set.contains(Coord::new(i).unwrap()));
        }
    }

    #[test]
    fn remove_all() {
        let mut set = CoordSet::new();
        for i in 0u16..11172 {
            set.insert(Coord::new(i).unwrap());
        }
        for i in 0u16..11172 {
            set.remove(Coord::new(i).unwrap());
        }
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn display_format() {
        let mut set = CoordSet::new();
        set.insert(Coord::new(0).unwrap());
        let s = format!("{}", set);
        assert!(s.contains("가")); // U+AC00
    }

    #[test]
    fn clone_eq() {
        let mut a = CoordSet::new();
        a.insert(Coord::new(0).unwrap());
        let b = a;
        assert_eq!(a, b);
        assert!(a.contains(Coord::new(0).unwrap()));
    }

    #[test]
    fn default_is_empty() {
        let set: CoordSet = Default::default();
        assert!(set.is_empty());
    }

    #[test]
    fn get_present() {
        let mut set = CoordSet::new();
        let c = Coord::new(42).unwrap();
        set.insert(c);
        assert_eq!(set.get(&c), Some(&c));
    }

    #[test]
    fn get_absent() {
        let set = CoordSet::new();
        assert_eq!(set.get(&Coord::new(0).unwrap()), None);
    }

    #[test]
    fn take_present() {
        let mut set = CoordSet::new();
        let c = Coord::new(42).unwrap();
        set.insert(c);
        assert_eq!(set.take(&c), Some(c));
        assert!(!set.contains(c));
    }

    #[test]
    fn take_absent() {
        let mut set = CoordSet::new();
        assert_eq!(set.take(&Coord::new(0).unwrap()), None);
    }

    #[test]
    fn retain_all() {
        let mut set = CoordSet::new();
        for i in 0u16..10 {
            set.insert(Coord::new(i).unwrap());
        }
        set.retain(|_| true);
        assert_eq!(set.len(), 10);
    }

    #[test]
    fn retain_odd() {
        let mut set = CoordSet::new();
        for i in 0u16..10 {
            set.insert(Coord::new(i).unwrap());
        }
        set.retain(|c| c.index() % 2 == 0);
        assert_eq!(set.len(), 5);
        for i in 0u16..10 {
            let c = Coord::new(i).unwrap();
            assert_eq!(set.contains(c), i % 2 == 0);
        }
    }

    #[test]
    fn retain_empty() {
        let mut set = CoordSet::new();
        set.retain(|_| true);
        assert!(set.is_empty());
    }

    #[test]
    fn capacity_instance() {
        let set = CoordSet::new();
        assert_eq!(set.capacity(), 11172);
    }
}
