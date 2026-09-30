// Fixed-width bitset over the coordinate space.
//
// Every set the board keeps (occupancy, and one posting per axis value) is a
// bitset over the same coordinate index space. That shared index space is the
// whole trick: a conjunctive query is the word-wise AND of the postings it
// names, so the evaluation is a fixed number of machine-word operations
// instead of a walk over the records.

use core::fmt;

use crate::layout::{COORD_SPACE, WORDS};

#[derive(Clone, PartialEq, Eq)]
pub struct Bits {
    words: [u64; WORDS],
}

impl Bits {
    pub const EMPTY: Bits = Bits {
        words: [0u64; WORDS],
    };

    pub fn new() -> Self {
        Self::EMPTY
    }

    #[inline]
    pub fn set(&mut self, index: usize) {
        debug_assert!(index < COORD_SPACE, "set index out of space");
        self.words[index >> 6] |= 1u64 << (index & 63);
    }

    #[inline]
    pub fn test(&self, index: usize) -> bool {
        debug_assert!(index < COORD_SPACE, "test index out of space");
        self.words[index >> 6] & (1u64 << (index & 63)) != 0
    }

    #[inline]
    pub fn and_assign(&mut self, other: &Bits) {
        for (a, b) in self.words.iter_mut().zip(other.words.iter()) {
            *a &= *b;
        }
    }

    #[inline]
    pub fn or_assign(&mut self, other: &Bits) {
        for (a, b) in self.words.iter_mut().zip(other.words.iter()) {
            *a |= *b;
        }
    }

    pub fn count(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    pub fn iter(&self) -> BitIter {
        BitIter {
            bits: self.clone(),
            word: 0,
        }
    }
}

impl Default for Bits {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl fmt::Debug for Bits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bits({} set)", self.count())
    }
}

/// An iterator over the set coordinate indices, in ascending order.
pub struct BitIter {
    bits: Bits,
    word: usize,
}

impl Iterator for BitIter {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        while self.word < WORDS {
            let word = self.bits.words[self.word];
            if word != 0 {
                let bit = word.trailing_zeros() as usize;
                // Clear the lowest set bit so the next call advances.
                self.bits.words[self.word] = word & (word - 1);
                return Some((self.word << 6) + bit);
            }
            self.word += 1;
        }
        None
    }
}
