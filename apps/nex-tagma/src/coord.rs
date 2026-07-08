use std::fmt;

/// A 16-bit Tagma coordinate representing a Hangul syllable.
///
/// Defined by the formula:
///   code_point = 0xAC00 + (initial x 588) + (medial x 28) + final
///
/// Where initial (choseong): 0-18, medial (jungseong): 0-21, final (jongseong): 0-27.
/// Total: 19 x 21 x 28 = 11,172 valid coordinates in a 16-bit space of 65,536.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TagmaCoord(u16);

const BASE: u16 = 0xAC00;
const N_INIT: u8 = 19;
const N_MED: u8 = 21;
const N_FIN: u8 = 28;
const M1: u16 = 588;
const M2: u16 = 28;

impl TagmaCoord {
    /// Create a Tagma coordinate from initial, medial, and final indices.
    /// Returns `None` if any index is out of range.
    pub fn new(initial: u8, medial: u8, final_: u8) -> Option<Self> {
        if initial >= N_INIT || medial >= N_MED || final_ >= N_FIN {
            return None;
        }
        let cp = BASE + (initial as u16) * M1 + (medial as u16) * M2 + final_ as u16;
        Some(Self(cp))
    }

    /// Create a Tagma coordinate from a Unicode code point.
    /// Returns `None` if the code point is outside the Hangul syllable block
    /// or falls in a filler position (within the block but not producible by the formula).
    pub fn from_code_point(cp: u16) -> Option<Self> {
        if !(0xAC00..=0xD7AF).contains(&cp) {
            return None;
        }
        let offset = cp - BASE;
        let initial = (offset / M1) as u8;
        let rem = offset % M1;
        let medial = (rem / M2) as u8;
        let final_ = (rem % M2) as u8;
        if initial >= N_INIT || medial >= N_MED || final_ >= N_FIN {
            return None;
        }
        Some(Self(cp))
    }

    /// The raw 16-bit Unicode code point value.
    pub const fn code_point(&self) -> u16 {
        self.0
    }

    /// Decompose into (initial, medial, final) axis values.
    pub fn decompose(&self) -> (u8, u8, u8) {
        let offset = self.0 - BASE;
        let initial = (offset / M1) as u8;
        let rem = offset % M1;
        let medial = (rem / M2) as u8;
        let final_ = (rem % M2) as u8;
        (initial, medial, final_)
    }

    /// Returns true if the given code point is a valid Tagma coordinate.
    pub fn validate(cp: u16) -> bool {
        Self::from_code_point(cp).is_some()
    }

    /// Map this coordinate to a dense linear index 0..11171.
    /// This enables array-based storage with zero hash tables.
    pub fn to_dense_index(self) -> usize {
        let (i, m, f) = self.decompose();
        (i as usize) * 588 + (m as usize) * 28 + (f as usize)
    }

    /// Field-wise Hamming distance between two coordinates.
    /// Returns (distance_initial, distance_medial, distance_final).
    pub fn hamming_distance(&self, other: &Self) -> (u8, u8, u8) {
        let (ai, am, af) = self.decompose();
        let (bi, bm, bf) = other.decompose();
        (ai.abs_diff(bi), am.abs_diff(bm), af.abs_diff(bf))
    }

    /// The Hangul syllable character for this coordinate.
    pub fn to_char(self) -> char {
        // self.0 is always a valid Unicode Hangul syllable code point
        // by construction, so unwrap_or is defense-in-depth only.
        char::from_u32(self.0 as u32).unwrap_or('\u{FFFD}')
    }
}

impl fmt::Display for TagmaCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (i, m, fn_) = self.decompose();
        write!(f, "{} (U+{:04X}, i={i}, m={m}, f={fn_})", self.to_char(), self.0)
    }
}

impl From<TagmaCoord> for u16 {
    fn from(c: TagmaCoord) -> Self {
        c.0
    }
}
