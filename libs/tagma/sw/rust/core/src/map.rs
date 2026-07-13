use crate::coord::Coord;
use alloc::boxed::Box;

/// A collision-free, fixed-size, hash-less associative array indexed by
/// [`Coord`].
///
/// See the [module-level documentation](crate) for design rationale.
#[derive(Clone, Debug)]
pub struct CoordMap<V> {
    slots: Box<[Option<V>]>,
    len: usize,
}

// ---------------------------------------------------------------------------
// Core read / write
// ---------------------------------------------------------------------------

impl<V> CoordMap<V> {
    const N: usize = Coord::N_VALID;

    #[inline]
    fn idx(coord: Coord) -> usize {
        coord.index() as usize
    }

    #[inline]
    fn slot(&self, coord: Coord) -> &Option<V> {
        unsafe { self.slots.get_unchecked(Self::idx(coord)) }
    }

    #[inline]
    fn slot_mut(&mut self, coord: Coord) -> &mut Option<V> {
        unsafe { self.slots.get_unchecked_mut(Self::idx(coord)) }
    }

    // -- query ------------------------------------------------------------

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[inline]
    pub fn capacity(&self) -> usize {
        Self::N
    }

    // -- lookup -----------------------------------------------------------

    #[inline]
    pub fn get(&self, coord: Coord) -> Option<&V> {
        self.slot(coord).as_ref()
    }
    #[inline]
    pub fn get_mut(&mut self, coord: Coord) -> Option<&mut V> {
        self.slot_mut(coord).as_mut()
    }
    #[inline]
    pub fn get_key_value(&self, coord: Coord) -> Option<(Coord, &V)> {
        self.slot(coord).as_ref().map(|v| (coord, v))
    }
    #[inline]
    pub fn contains_key(&self, coord: Coord) -> bool {
        self.slot(coord).is_some()
    }

    // -- mutation ---------------------------------------------------------

    pub fn insert(&mut self, coord: Coord, value: V) -> Option<V> {
        let slot = self.slot_mut(coord);
        let old = slot.take();
        *slot = Some(value);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    pub fn remove(&mut self, coord: Coord) -> Option<V> {
        let slot = self.slot_mut(coord);
        let old = slot.take();
        if old.is_some() {
            self.len -= 1;
        }
        old
    }

    pub fn clear(&mut self) {
        for slot in self.slots.iter_mut() {
            *slot = None;
        }
        self.len = 0;
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl<V> CoordMap<V> {
    #[inline]
    pub fn new() -> Self {
        let slots = (0..Self::N).map(|_| None).collect::<Box<[_]>>();
        CoordMap { slots, len: 0 }
    }
}

impl<V> Default for CoordMap<V> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Entry API
// ---------------------------------------------------------------------------

impl<V> CoordMap<V> {
    pub fn entry(&mut self, coord: Coord) -> Entry<'_, V> {
        if self.contains_key(coord) {
            Entry::Occupied(OccupiedEntry { map: self, coord })
        } else {
            Entry::Vacant(VacantEntry { map: self, coord })
        }
    }
}

pub enum Entry<'a, V> {
    Occupied(OccupiedEntry<'a, V>),
    Vacant(VacantEntry<'a, V>),
}

pub struct OccupiedEntry<'a, V> {
    pub(super) map: &'a mut CoordMap<V>,
    pub(super) coord: Coord,
}

impl<'a, V> OccupiedEntry<'a, V> {
    pub fn key(&self) -> Coord {
        self.coord
    }

    /// Returns a reference to the stored value.
    ///
    /// # Safety
    ///
    /// `OccupiedEntry` guarantees the slot is occupied: the entry was created
    /// only after `contains_key` returned `true`, and the mutable borrow on
    /// the map prevents any concurrent removal.
    pub fn get(&self) -> &V {
        // SAFETY: the slot is verified occupied at entry creation.
        unsafe { self.map.get(self.coord).unwrap_unchecked() }
    }

    /// Returns a mutable reference to the stored value.
    pub fn get_mut(&mut self) -> &mut V {
        // SAFETY: same occupancy invariant as `get`.
        unsafe { self.map.get_mut(self.coord).unwrap_unchecked() }
    }

    /// Inserts a new value, returning the old one.
    pub fn insert(&mut self, value: V) -> V {
        // SAFETY: same occupancy invariant.
        unsafe { self.map.insert(self.coord, value).unwrap_unchecked() }
    }

    /// Removes and returns the value.
    pub fn remove_entry(self) -> V {
        // SAFETY: same occupancy invariant.
        unsafe { self.map.remove(self.coord).unwrap_unchecked() }
    }
}

pub struct VacantEntry<'a, V> {
    pub(super) map: &'a mut CoordMap<V>,
    pub(super) coord: Coord,
}

impl<'a, V> VacantEntry<'a, V> {
    pub fn key(&self) -> Coord {
        self.coord
    }

    pub fn into_key(self) -> Coord {
        self.coord
    }

    /// Inserts a value and returns a mutable reference to it.
    pub fn insert(self, value: V) -> &'a mut V {
        // Vacant → guaranteed no old value to discard.
        let _ = self.map.insert(self.coord, value);
        // SAFETY: we just inserted the value above.
        unsafe { self.map.get_mut(self.coord).unwrap_unchecked() }
    }
}

impl<'a, V> Entry<'a, V> {
    pub fn key(&self) -> Coord {
        match self {
            Entry::Occupied(e) => e.key(),
            Entry::Vacant(e) => e.key(),
        }
    }

    pub fn or_insert(self, default: V) -> &'a mut V {
        self.or_insert_with(|| default)
    }

    pub fn or_insert_with<F: FnOnce() -> V>(self, f: F) -> &'a mut V {
        match self {
            Entry::Occupied(e) => unsafe { e.map.get_mut(e.coord).unwrap_unchecked() },
            Entry::Vacant(e) => e.insert(f()),
        }
    }

    pub fn or_insert_with_key<F: FnOnce(Coord) -> V>(self, f: F) -> &'a mut V {
        match self {
            Entry::Occupied(e) => unsafe { e.map.get_mut(e.coord).unwrap_unchecked() },
            Entry::Vacant(e) => {
                let v = f(e.coord);
                e.insert(v)
            }
        }
    }

    pub fn and_modify<F: FnOnce(&mut V)>(mut self, f: F) -> Self {
        if let Entry::Occupied(ref mut e) = self {
            f(e.get_mut());
        }
        self
    }
}

// ---------------------------------------------------------------------------
// Iteration
// ---------------------------------------------------------------------------

pub struct Iter<'a, V> {
    slots: core::slice::Iter<'a, Option<V>>,
    idx: u16,
}

impl<'a, V> Iterator for Iter<'a, V> {
    type Item = (Coord, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        for slot in self.slots.by_ref() {
            let coord = Coord::new(self.idx).unwrap();
            self.idx += 1;
            if let Some(val) = slot.as_ref() {
                return Some((coord, val));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.slots.len()))
    }
}

pub struct IterMut<'a, V> {
    slots: core::slice::IterMut<'a, Option<V>>,
    idx: u16,
}

impl<'a, V> Iterator for IterMut<'a, V> {
    type Item = (Coord, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        for slot in self.slots.by_ref() {
            let coord = Coord::new(self.idx).unwrap();
            self.idx += 1;
            if let Some(val) = slot.as_mut() {
                return Some((coord, val));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.slots.len()))
    }
}

pub struct IntoIter<V> {
    slots: alloc::vec::IntoIter<Option<V>>,
    idx: u16,
}

impl<V> Iterator for IntoIter<V> {
    type Item = (Coord, V);

    fn next(&mut self) -> Option<Self::Item> {
        for slot in self.slots.by_ref() {
            let coord = Coord::new(self.idx).unwrap();
            self.idx += 1;
            if let Some(val) = slot {
                return Some((coord, val));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.slots.len()))
    }
}

pub struct Drain<'a, V> {
    slots: core::slice::IterMut<'a, Option<V>>,
    map_len: &'a mut usize,
    idx: u16,
}

impl<'a, V> Iterator for Drain<'a, V> {
    type Item = (Coord, V);

    fn next(&mut self) -> Option<Self::Item> {
        for slot in self.slots.by_ref() {
            let coord = Coord::new(self.idx).unwrap();
            self.idx += 1;
            if let Some(val) = slot.take() {
                *self.map_len -= 1;
                return Some((coord, val));
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.slots.len()))
    }
}

impl<'a, V> Drop for Drain<'a, V> {
    fn drop(&mut self) {
        for slot in self.slots.by_ref() {
            if slot.take().is_some() {
                *self.map_len -= 1;
            }
        }
        // After draining all remaining entries, the map must be empty.
        debug_assert_eq!(*self.map_len, 0);
    }
}

// ---------------------------------------------------------------------------
// Iterator constructors
// ---------------------------------------------------------------------------

impl<V> CoordMap<V> {
    pub fn iter(&self) -> Iter<'_, V> {
        Iter {
            slots: self.slots.iter(),
            idx: 0,
        }
    }
    pub fn iter_mut(&mut self) -> IterMut<'_, V> {
        IterMut {
            slots: self.slots.iter_mut(),
            idx: 0,
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = Coord> + '_ {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> + '_ {
        self.iter_mut().map(|(_, v)| v)
    }

    pub fn drain(&mut self) -> Drain<'_, V> {
        Drain {
            slots: self.slots.iter_mut(),
            map_len: &mut self.len,
            idx: 0,
        }
    }

    pub fn retain<F: FnMut(Coord, &mut V) -> bool>(&mut self, mut f: F) {
        let mut idx = 0u16;
        self.slots.iter_mut().for_each(|slot| {
            let coord = Coord::new(idx).unwrap();
            idx += 1;
            if let Some(val) = slot.as_mut() {
                if !f(coord, val) {
                    *slot = None;
                    self.len -= 1;
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// FromIterator
// ---------------------------------------------------------------------------

impl<V> FromIterator<(Coord, V)> for CoordMap<V> {
    fn from_iter<I: IntoIterator<Item = (Coord, V)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (coord, value) in iter {
            map.insert(coord, value);
        }
        map
    }
}

// ---------------------------------------------------------------------------
// IntoIterator
// ---------------------------------------------------------------------------

impl<V> IntoIterator for CoordMap<V> {
    type Item = (Coord, V);
    type IntoIter = IntoIter<V>;

    fn into_iter(self) -> IntoIter<V> {
        let vec: alloc::vec::Vec<Option<V>> = self.slots.into_vec();
        IntoIter {
            slots: vec.into_iter(),
            idx: 0,
        }
    }
}

impl<'a, V> IntoIterator for &'a CoordMap<V> {
    type Item = (Coord, &'a V);
    type IntoIter = Iter<'a, V>;

    fn into_iter(self) -> Iter<'a, V> {
        self.iter()
    }
}

impl<'a, V> IntoIterator for &'a mut CoordMap<V> {
    type Item = (Coord, &'a mut V);
    type IntoIter = IterMut<'a, V>;

    fn into_iter(self) -> IterMut<'a, V> {
        self.iter_mut()
    }
}

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

impl<V> core::ops::Index<Coord> for CoordMap<V> {
    type Output = V;

    fn index(&self, coord: Coord) -> &V {
        self.get(coord).expect("CoordMap::index: key not present")
    }
}

impl<V> core::ops::IndexMut<Coord> for CoordMap<V> {
    fn index_mut(&mut self, coord: Coord) -> &mut V {
        self.get_mut(coord)
            .expect("CoordMap::index_mut: key not present")
    }
}

// ---------------------------------------------------------------------------
// Equality
// ---------------------------------------------------------------------------

impl<V: PartialEq> PartialEq for CoordMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.slots == other.slots
    }
}

impl<V: PartialEq> Eq for CoordMap<V> {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn new_map_is_empty() {
        let map: CoordMap<u32> = CoordMap::new();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 11172);
    }

    #[test]
    fn insert_and_get() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        assert_eq!(map.insert(c, 42), None);
        assert_eq!(map.get(c), Some(&42));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn insert_overwrite() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.insert(c, 1);
        assert_eq!(map.insert(c, 2), Some(1));
        assert_eq!(map.get(c), Some(&2));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn remove() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.insert(c, 42);
        assert_eq!(map.remove(c), Some(42));
        assert_eq!(map.get(c), None);
        assert!(map.is_empty());
    }

    #[test]
    fn contains_key() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        assert!(!map.contains_key(c));
        map.insert(c, ());
        assert!(map.contains_key(c));
    }

    #[test]
    fn slot_independent() {
        let mut map = CoordMap::new();
        let a = Coord::new(0).unwrap();
        let b = Coord::new(11171).unwrap();
        map.insert(a, "first");
        map.insert(b, "last");
        assert_eq!(map.get(a), Some(&"first"));
        assert_eq!(map.get(b), Some(&"last"));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn clear() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(0).unwrap(), 1);
        map.insert(Coord::new(100).unwrap(), 2);
        map.clear();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn iter_empty() {
        let map: CoordMap<u32> = CoordMap::new();
        assert_eq!(map.iter().count(), 0);
    }

    #[test]
    fn iter_non_empty() {
        let mut map = CoordMap::new();
        let c1 = Coord::new(0).unwrap();
        let c2 = Coord::new(9999).unwrap();
        map.insert(c1, 10);
        map.insert(c2, 20);
        let entries: Vec<_> = map.iter().collect();
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&(c1, &10)));
        assert!(entries.contains(&(c2, &20)));
    }

    #[test]
    fn iter_mut() {
        let mut map = CoordMap::new();
        let c = Coord::new(5).unwrap();
        map.insert(c, 1);
        for (_, v) in map.iter_mut() {
            *v += 1;
        }
        assert_eq!(map.get(c), Some(&2));
    }

    #[test]
    fn into_iter_consuming() {
        let mut map = CoordMap::new();
        let c = Coord::new(42).unwrap();
        map.insert(c, "hello");
        let collected: Vec<_> = map.into_iter().collect();
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].0, c);
        assert_eq!(collected[0].1, "hello");
    }

    #[test]
    fn into_iter_ref() {
        let mut map = CoordMap::new();
        let c = Coord::new(7).unwrap();
        map.insert(c, 99);
        let collected: Vec<_> = (&map).into_iter().collect();
        assert_eq!(collected[0], (c, &99));
    }

    #[test]
    fn into_iter_mut_ref() {
        let mut map = CoordMap::new();
        let c = Coord::new(7).unwrap();
        map.insert(c, 99);
        for (_, v) in &mut map {
            *v = 0;
        }
        assert_eq!(map.get(c), Some(&0));
    }

    #[test]
    fn keys_values() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(0).unwrap(), "a");
        map.insert(Coord::new(1).unwrap(), "b");
        assert_eq!(map.keys().count(), 2);
        assert_eq!(map.values().cloned().collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn drain_empties_map() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(0).unwrap(), 1);
        map.insert(Coord::new(1).unwrap(), 2);
        let drained: Vec<_> = map.drain().collect();
        assert_eq!(drained.len(), 2);
        assert!(map.is_empty());
    }

    #[test]
    fn retain() {
        let mut map = CoordMap::new();
        for i in 0..10u16 {
            map.insert(Coord::new(i).unwrap(), i);
        }
        map.retain(|_, v| *v % 2 == 0);
        assert_eq!(map.len(), 5);
        for i in 0..10u16 {
            let c = Coord::new(i).unwrap();
            assert_eq!(map.get(c).is_some(), i % 2 == 0);
        }
    }

    #[test]
    fn from_iterator() {
        let coords: Vec<_> = (0..5u16)
            .map(|i| (Coord::new(i).unwrap(), i * 10))
            .collect();
        let map: CoordMap<u16> = coords.into_iter().collect();
        assert_eq!(map.len(), 5);
        assert_eq!(map.get(Coord::new(3).unwrap()), Some(&30));
    }

    #[test]
    fn entry_or_insert() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.entry(c).or_insert(42);
        assert_eq!(map.get(c), Some(&42));
        map.entry(c).or_insert(99);
        assert_eq!(map.get(c), Some(&42));
    }

    #[test]
    fn entry_or_insert_with_key() {
        let mut map = CoordMap::new();
        let c = Coord::new(5).unwrap();
        map.entry(c).or_insert_with_key(|k| k.index() as u32);
        assert_eq!(map.get(c), Some(&5));
    }

    #[test]
    fn entry_and_modify() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.insert(c, 1);
        map.entry(c).and_modify(|v| *v += 1).or_insert(0);
        assert_eq!(map.get(c), Some(&2));
    }

    #[test]
    fn entry_vacant_insert() {
        let mut map = CoordMap::new();
        let c = Coord::new(42).unwrap();
        if let Entry::Vacant(e) = map.entry(c) {
            e.insert("hello");
        } else {
            panic!("should be vacant");
        }
        assert_eq!(map.get(c), Some(&"hello"));
    }

    #[test]
    fn entry_occupied_remove() {
        let mut map = CoordMap::new();
        let c = Coord::new(7).unwrap();
        map.insert(c, "x");
        if let Entry::Occupied(e) = map.entry(c) {
            assert_eq!(e.remove_entry(), "x");
        } else {
            panic!("should be occupied");
        }
        assert!(!map.contains_key(c));
    }

    #[test]
    fn index_trait() {
        let mut map = CoordMap::new();
        let c = Coord::new(3).unwrap();
        map.insert(c, 10);
        assert_eq!(map[c], 10);
        map[c] = 20;
        assert_eq!(map[c], 20);
    }

    #[test]
    #[should_panic(expected = "key not present")]
    fn index_panics_on_missing() {
        let map: CoordMap<u32> = CoordMap::new();
        let _ = &map[Coord::new(0).unwrap()];
    }

    #[test]
    fn eq() {
        let mut a = CoordMap::new();
        let mut b = CoordMap::new();
        a.insert(Coord::new(0).unwrap(), 1);
        b.insert(Coord::new(0).unwrap(), 1);
        assert_eq!(a, b);
        b.insert(Coord::new(1).unwrap(), 2);
        assert_ne!(a, b);
    }

    // =========================================================================
    // HashMap 1:1 replacement scenario tests
    // =========================================================================

    #[test]
    fn get_key_value_returns_coord() {
        let mut map = CoordMap::new();
        let c = Coord::new(7).unwrap();
        map.insert(c, 42);
        let (k, v) = map.get_key_value(c).unwrap();
        assert_eq!(k, c);
        assert_eq!(*v, 42);
    }

    #[test]
    fn get_key_value_missing() {
        let map: CoordMap<u32> = CoordMap::new();
        assert_eq!(map.get_key_value(Coord::new(0).unwrap()), None);
    }

    #[test]
    fn insert_then_get_then_remove_then_get() {
        let mut map = CoordMap::new();
        let c = Coord::new(100).unwrap();
        assert_eq!(map.insert(c, 1), None);
        assert_eq!(map.get(c), Some(&1));
        assert_eq!(map.remove(c), Some(1));
        assert_eq!(map.get(c), None);
        assert!(map.is_empty());
    }

    #[test]
    fn insert_duplicate_coord_tracks_len() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.insert(c, 1);
        assert_eq!(map.len(), 1);
        map.insert(c, 2);
        assert_eq!(map.len(), 1); // overwrite does not increase len
    }

    #[test]
    fn fill_and_empty_cycle() {
        let mut map = CoordMap::new();
        // Fill
        for i in 0u16..11172 {
            map.insert(Coord::new(i).unwrap(), i);
        }
        assert_eq!(map.len(), 11172);
        assert!(!map.is_empty());
        // Empty via drain
        let count = map.drain().count();
        assert_eq!(count, 11172);
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        // Refill after drain
        for i in 0u16..100 {
            map.insert(Coord::new(i).unwrap(), i);
        }
        assert_eq!(map.len(), 100);
    }

    #[test]
    fn entry_or_insert_idiom_increment() {
        // HashMap pattern: *map.entry(k).or_insert(0) += 1
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        for _ in 0..5 {
            *map.entry(c).or_insert(0) += 1;
        }
        assert_eq!(map.get(c), Some(&5));
    }

    #[test]
    fn entry_and_modify_chain() {
        // HashMap pattern: map.entry(k).and_modify(|v| *v += 1).or_insert(1)
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.entry(c).and_modify(|v| *v += 1).or_insert(1);
        assert_eq!(map.get(c), Some(&1));
        map.entry(c).and_modify(|v| *v += 1).or_insert(1);
        assert_eq!(map.get(c), Some(&2));
    }

    #[test]
    fn entry_match_occupied() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.insert(c, "hello");
        match map.entry(c) {
            Entry::Occupied(e) => {
                assert_eq!(e.key(), c);
                assert_eq!(*e.get(), "hello");
            }
            Entry::Vacant(_) => panic!("should be occupied"),
        }
    }

    #[test]
    fn entry_match_vacant() {
        let mut map = CoordMap::new();
        let c = Coord::new(42).unwrap();
        match map.entry(c) {
            Entry::Occupied(_) => panic!("should be vacant"),
            Entry::Vacant(e) => {
                assert_eq!(e.key(), c);
                e.insert("world");
            }
        }
        assert_eq!(map.get(c), Some(&"world"));
    }

    #[test]
    fn collect_roundtrip() {
        let data: Vec<_> = (0..50u16)
            .map(|i| (Coord::new(i).unwrap(), i as u64))
            .collect();
        let map: CoordMap<u64> = data.clone().into_iter().collect();
        assert_eq!(map.len(), 50);
        let collected_back: Vec<_> = map.into_iter().collect();
        assert_eq!(collected_back.len(), 50);
        // Order is deterministic (coordinate order) but keys are unique
        let mut sorted = data;
        sorted.sort_by_key(|(k, _)| *k);
        for ((k1, v1), (k2, v2)) in sorted.iter().zip(collected_back.iter()) {
            assert_eq!(k1, k2);
            assert_eq!(v1, v2);
        }
    }

    #[test]
    fn for_loop_borrowed() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(0).unwrap(), 10);
        map.insert(Coord::new(1).unwrap(), 20);
        let mut sum = 0u32;
        for (_, v) in &map {
            sum += *v;
        }
        assert_eq!(sum, 30);
        // Map is still usable after borrow
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn for_loop_mut_borrowed() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(0).unwrap(), 1);
        for (_, v) in &mut map {
            *v += 1;
        }
        assert_eq!(map.get(Coord::new(0).unwrap()), Some(&2));
    }

    #[test]
    fn into_iter_for_loop() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(5).unwrap(), "a");
        map.insert(Coord::new(10).unwrap(), "b");
        let mut collected = Vec::new();
        for (k, v) in map {
            collected.push((k, v));
        }
        assert_eq!(collected.len(), 2);
    }

    #[test]
    fn index_read_write() {
        let mut map = CoordMap::new();
        let c = Coord::new(7).unwrap();
        map.insert(c, 100);
        assert_eq!(map[c], 100);
        map[c] = 200;
        assert_eq!(map[c], 200);
    }

    #[test]
    #[should_panic]
    fn index_panics_vacant() {
        let map: CoordMap<i32> = CoordMap::new();
        let _ = &map[Coord::new(0).unwrap()];
    }

    #[test]
    fn retain_all_true() {
        let mut map = CoordMap::new();
        for i in 0u16..100 {
            map.insert(Coord::new(i).unwrap(), i);
        }
        map.retain(|_, _| true);
        assert_eq!(map.len(), 100);
    }

    #[test]
    fn retain_all_false() {
        let mut map = CoordMap::new();
        for i in 0u16..100 {
            map.insert(Coord::new(i).unwrap(), i);
        }
        map.retain(|_, _| false);
        assert!(map.is_empty());
    }

    #[test]
    fn retain_by_coord() {
        let mut map = CoordMap::new();
        for i in 0u16..11172 {
            map.insert(Coord::new(i).unwrap(), i);
        }
        // Retain only first half
        map.retain(|k, _| k.index() < 5586);
        assert_eq!(map.len(), 5586);
        assert!(map.contains_key(Coord::new(0).unwrap()));
        assert!(!map.contains_key(Coord::new(5586).unwrap()));
    }

    #[test]
    fn default_is_empty() {
        let map: CoordMap<String> = Default::default();
        assert!(map.is_empty());
    }

    #[test]
    fn clone_independent() {
        let mut a = CoordMap::new();
        a.insert(Coord::new(0).unwrap(), 42);
        let mut b = a.clone();
        b.insert(Coord::new(1).unwrap(), 99);
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 2);
        assert_eq!(a.get(Coord::new(0).unwrap()), Some(&42));
        assert_eq!(b.get(Coord::new(0).unwrap()), Some(&42));
    }

    #[test]
    fn debug_format() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(0).unwrap(), 1);
        let s = format!("{:?}", map);
        assert!(s.contains("CoordMap"));
    }

    #[test]
    fn many_inserts_no_collisions() {
        let mut map = CoordMap::new();
        for i in 0u16..11172 {
            let prev = map.insert(Coord::new(i).unwrap(), i);
            assert!(prev.is_none(), "collision at index {}", i);
        }
        assert_eq!(map.len(), 11172);
    }

    #[test]
    fn overwrite_all_entries() {
        let mut map = CoordMap::new();
        for i in 0u16..11172 {
            map.insert(Coord::new(i).unwrap(), 0u32);
        }
        for i in 0u16..11172 {
            let prev = map.insert(Coord::new(i).unwrap(), i as u32);
            assert_eq!(prev, Some(0));
        }
        assert_eq!(map.len(), 11172);
    }

    #[test]
    fn remove_all_entries() {
        let mut map = CoordMap::new();
        for i in 0u16..11172 {
            map.insert(Coord::new(i).unwrap(), i);
        }
        for i in 0u16..11172 {
            let v = map.remove(Coord::new(i).unwrap());
            assert_eq!(v, Some(i));
        }
        assert!(map.is_empty());
    }

    #[test]
    fn keys_iterator_order() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(5).unwrap(), "a");
        map.insert(Coord::new(3).unwrap(), "b");
        map.insert(Coord::new(7).unwrap(), "c");
        let keys: Vec<_> = map.keys().collect();
        // Keys come in coordinate order (3, 5, 7), not insertion order
        assert_eq!(
            keys,
            vec![
                Coord::new(3).unwrap(),
                Coord::new(5).unwrap(),
                Coord::new(7).unwrap(),
            ]
        );
    }

    #[test]
    fn values_iterator_order() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(5).unwrap(), "c");
        map.insert(Coord::new(3).unwrap(), "a");
        map.insert(Coord::new(7).unwrap(), "e");
        let values: Vec<_> = map.values().copied().collect();
        assert_eq!(values, vec!["a", "c", "e"]);
    }

    #[test]
    fn drain_then_insert() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(0).unwrap(), 1);
        map.drain();
        assert!(map.is_empty());
        map.insert(Coord::new(0).unwrap(), 2);
        assert_eq!(map.get(Coord::new(0).unwrap()), Some(&2));
    }

    #[test]
    fn clear_then_insert() {
        let mut map = CoordMap::new();
        map.insert(Coord::new(0).unwrap(), 1);
        map.clear();
        assert!(map.is_empty());
        map.insert(Coord::new(0).unwrap(), 2);
        assert_eq!(map.get(Coord::new(0).unwrap()), Some(&2));
    }

    #[test]
    fn entry_take_ownership() {
        let mut map = CoordMap::new();
        let c = Coord::new(42).unwrap();
        map.insert(c, "owned");
        if let Entry::Occupied(e) = map.entry(c) {
            let v = e.remove_entry();
            assert_eq!(v, "owned");
        }
        assert!(!map.contains_key(c));
    }

    #[test]
    fn entry_insert_if_vacant_else_update() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        // insert if vacant
        map.entry(c).and_modify(|v| *v += 1).or_insert(0);
        assert_eq!(map[c], 0);
        // update if occupied
        map.entry(c).and_modify(|v| *v += 1).or_insert(0);
        assert_eq!(map[c], 1);
    }

    #[test]
    fn large_value_type() {
        // Ensure CoordMap works with large value types (e.g., arrays)
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.insert(c, [0u8; 1024]);
        assert!(map.contains_key(c));
        let v = map.get(c).unwrap();
        assert_eq!(v.len(), 1024);
    }

    #[test]
    fn string_values() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.insert(c, "hello".to_string());
        assert_eq!(map.get(c).map(|s| s.as_str()), Some("hello"));
        map.entry(c)
            .and_modify(|s| s.push_str(" world"))
            .or_insert_with(String::new);
        assert_eq!(map.get(c).map(|s| s.as_str()), Some("hello world"));
    }

    #[test]
    fn option_value() {
        let mut map = CoordMap::new();
        let c = Coord::new(0).unwrap();
        map.insert(c, Some(42));
        assert_eq!(map.get(c), Some(&Some(42)));
        map.insert(c, None);
        // This is a valid value — CoordMap stores Option<V>, not nested
        assert_eq!(map.get(c), Some(&None));
    }

    #[test]
    fn eq_different_lengths() {
        let mut a = CoordMap::new();
        let mut b = CoordMap::new();
        a.insert(Coord::new(0).unwrap(), 1);
        b.insert(Coord::new(0).unwrap(), 1);
        assert_eq!(a, b);
        b.insert(Coord::new(1).unwrap(), 2);
        assert_ne!(a, b);
        a.insert(Coord::new(1).unwrap(), 2);
        assert_eq!(a, b);
    }

    #[test]
    fn eq_different_values_same_key() {
        let mut a = CoordMap::new();
        let mut b = CoordMap::new();
        a.insert(Coord::new(0).unwrap(), 1);
        b.insert(Coord::new(0).unwrap(), 99);
        assert_ne!(a, b);
    }
}
