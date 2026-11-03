// ── An optional ASCII rendering of a coordinate ────────────────────────
//
// The native alphabet is the coordinate space itself: one Hangul syllable per
// axis value. A consumer that cannot print or file those characters can render
// the same digits as fixed-width base-36 text instead. This module is that
// rendering and its inverse, and nothing in the record layer calls it.
//
// The rendering preserves order and the prefix structure. It is not a canonical
// identifier: `CoordId::resolve` reads a 60-character ASCII string as a label
// and derives a different record, so a caller that accepts one form has to
// route it through `decode` first.

use alloc::string::String;
use tagma_core::{Coord, CoordPath};

use crate::fih::CoordId;

/// Characters one axis value occupies in the ASCII rendering.
pub const DIGITS_PER_AXIS: usize = 3;

/// The alphabet in ascending code point order, which is also the digit order.
///
/// Ascending order is what makes sorting the text sort the coordinate, and the
/// fixed width is what makes a prefix of the text a prefix of the axes. Case is
/// not accepted on the way back, because two spellings of one value would cost
/// both properties.
const ALPHABET: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// The number of values one digit carries, taken from the alphabet so the
/// arithmetic cannot drift from the characters it writes.
const RADIX: u32 = ALPHABET.len() as u32;

/// Render a coordinate as fixed-width base-36 ASCII.
///
/// Each axis value is below 11,172, which three base-36 digits carry (46,656),
/// so the text is `DIGITS_PER_AXIS` characters per axis and the axis count
/// decides the length. The text is `N * 3` bytes, the same length as the native
/// form at any depth, because a Hangul syllable is three UTF-8 bytes.
///
/// The cost is two divisions per axis and no allocation per axis beyond the
/// string itself.
pub fn encode<const N: usize>(id: &CoordId<N>) -> String {
    let mut out = String::with_capacity(N * DIGITS_PER_AXIS);
    for coord in id.0.iter() {
        let value = coord.index() as u32;
        let high = value / (RADIX * RADIX);
        let rest = value % (RADIX * RADIX);
        out.push(ALPHABET[high as usize] as char);
        out.push(ALPHABET[(rest / RADIX) as usize] as char);
        out.push(ALPHABET[(rest % RADIX) as usize] as char);
    }
    out
}

/// Read the coordinate an ASCII rendering addresses.
///
/// A wrong length, a character outside the alphabet, or an axis value above the
/// coordinate space is `None`. The surplus of the alphabet is refused: three
/// digits carry 46,656 values and the space holds 11,172.
///
/// A system that folds case cannot carry the rendering, because folding would
/// map two texts to one coordinate. The native form has no such problem.
pub fn decode<const N: usize>(text: &str) -> Option<CoordId<N>> {
    let bytes = text.as_bytes();
    if bytes.len() != N * DIGITS_PER_AXIS {
        return None;
    }
    let mut coords = [Coord::new(0)?; N];
    for (axis, chunk) in bytes.as_chunks::<DIGITS_PER_AXIS>().0.iter().enumerate() {
        let mut value: u32 = 0;
        for &byte in chunk {
            value = value * RADIX + digit(byte)?;
        }
        coords[axis] = Coord::new(value as u16)?;
    }
    Some(CoordId(CoordPath::new(coords)))
}

fn digit(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u32),
        b'A'..=b'Z' => Some((byte - b'A' + 10) as u32),
        _ => None,
    }
}
