// Fixed geometry of the product space.
//
// The space is the Cartesian product of three axes of equal, fixed radix:
// work (the Fact domain), intent, and hint (the contract). A point is one
// combination, addressed by a packed index. Fixed radix is what lets a
// conjunction be a word-wise AND rather than a walk over records.
//
// AXIS_BITS is the knob. Raising it grows the space, the word count, and the
// posting table together; it does not change the query cost's independence
// from the record count.

/// Bits per axis value index. A value is an index in `0..AXIS_CARD`.
pub const AXIS_BITS: usize = 6;

/// Distinct values per axis.
pub const AXIS_CARD: usize = 1 << AXIS_BITS;

/// Number of axes: work, intent, hint.
pub const AXES: usize = 3;

/// Bits needed to address the whole product space.
pub const COORD_BITS: usize = AXIS_BITS * AXES;

/// Number of coordinates in the product space.
pub const COORD_SPACE: usize = 1 << COORD_BITS;

/// Words per bitset over the coordinate space.
pub const WORDS: usize = COORD_SPACE / u64::BITS as usize;
