/// A 16-bit value guaranteed to be a valid Hangul syllable coordinate.
///
/// `Coord` wraps a `u16` in the range `0..11172`, corresponding to
/// Unicode code points U+AC00..U+D7AF.  Every valid value is simultaneously
/// a Unicode address, a 3-axis coordinate (initial, medial, final), and
/// a Hangul syllable.
///
/// # Layout
///
/// | Bit     | Field               |
/// |---------|---------------------|
/// | 15      | reserved (zero)     |
/// | 14:10   | initial (choseong)  |
/// | 9:5     | medial  (jungseong) |
/// | 4:0     | final   (jongseong) |
///
/// # Composition formula
///
/// ```text
/// C(i, m, f) = 0xAC00 + 588·i + 28·m + f
/// ```
use alloc::string::{String, ToString};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Coord(u16);

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

impl Coord {
    /// Number of valid syllable blocks (19 × 21 × 28).
    pub const N_VALID: usize = 11_172;

    /// Number of representable 16-bit states.
    pub const N_TOTAL: usize = 65_536;

    /// Number of structurally invalid states (N_TOTAL − N_VALID).
    pub const N_INVALID: usize = Self::N_TOTAL - Self::N_VALID;

    /// Base code point (U+AC00).
    pub const BASE: u16 = 0xAC00;

    /// Last code point of the Unicode Hangul Syllables block (U+D7AF).
    /// Note: U+D7A4..U+D7AF are filler positions; the last valid syllable
    /// is at U+D7A3 (offset 11171).
    pub const LAST: u16 = 0xD7AF;

    const N_INIT: usize = 19;
    const N_MED: usize = 21;
    const N_FIN: usize = 28;
    const STRIDE_MED: usize = Self::N_FIN; // 28
    const STRIDE_INIT: usize = Self::N_MED * Self::N_FIN; // 588
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl Coord {
    /// Creates a `Coord` from a raw 16-bit value.
    ///
    /// Returns `None` if the value is outside the valid range [0, 11171].
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::new(0).unwrap();
    /// assert_eq!(c.to_char(), '가');
    ///
    /// let invalid = Coord::new(11172);
    /// assert!(invalid.is_none());
    /// ```
    #[inline]
    pub const fn new(value: u16) -> Option<Self> {
        if (value as usize) < Self::N_VALID {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Creates a `Coord` from its three structural axes.
    ///
    /// Returns `None` if any axis is out of bounds.
    /// Valid ranges: initial 0..19, medial 0..21, final 0..28.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let ga = Coord::from_axes(0, 0, 0).unwrap();
    /// assert_eq!(ga.to_char(), '가');
    ///
    /// let invalid = Coord::from_axes(19, 0, 0);
    /// assert!(invalid.is_none());
    /// ```
    #[inline]
    pub const fn from_axes(initial: u8, medial: u8, final_: u8) -> Option<Self> {
        let i = initial as usize;
        let m = medial as usize;
        let f = final_ as usize;
        if i < Self::N_INIT && m < Self::N_MED && f < Self::N_FIN {
            Some(Self(
                (i * Self::STRIDE_INIT + m * Self::STRIDE_MED + f) as u16,
            ))
        } else {
            None
        }
    }

    /// Creates a `Coord` from a Unicode code point.
    ///
    /// Returns `None` if the code point is outside the Hangul syllable block
    /// (U+AC00..U+D7AF) or falls on a filler position within it
    /// (U+D7A4..U+D7AF, 12 reserved code points that lack structural
    /// validity).
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::from_code_point(0xAC00).unwrap();
    /// assert_eq!(c.to_char(), '가');
    ///
    /// let invalid = Coord::from_code_point(0x0041);
    /// assert!(invalid.is_none());
    ///
    /// let filler = Coord::from_code_point(0xD7A4);
    /// assert!(filler.is_none());
    /// ```
    #[inline]
    pub const fn from_code_point(cp: u16) -> Option<Self> {
        if cp >= Self::BASE && cp <= Self::LAST {
            let offset = cp - Self::BASE;
            if (offset as usize) < Self::N_VALID {
                Some(Self(offset))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Creates a `Coord` from a `char`.
    ///
    /// Returns `None` if the character is not a valid Hangul syllable.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::from_char('한').unwrap();
    /// assert_eq!(c.to_code_point(), 0xD55C);
    ///
    /// let non_hangul = Coord::from_char('A');
    /// assert!(non_hangul.is_none());
    /// ```
    #[inline]
    pub const fn from_char(ch: char) -> Option<Self> {
        Self::from_code_point(ch as u16)
    }
}

// ---------------------------------------------------------------------------
// Decomposition
// ---------------------------------------------------------------------------

impl Coord {
    /// Returns the raw 0-based index (0..11171).
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::from_char('가').unwrap();
    /// assert_eq!(c.index(), 0);
    ///
    /// let last = Coord::from_char('힣').unwrap();
    /// assert_eq!(last.index(), 11171);
    /// ```
    #[inline]
    pub const fn index(self) -> u16 {
        self.0
    }

    /// Returns the Unicode code point (U+AC00..U+D7AF).
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::new(0).unwrap();
    /// assert_eq!(c.to_code_point(), 0xAC00);
    /// ```
    #[inline]
    pub const fn to_code_point(self) -> u16 {
        Self::BASE + self.0
    }

    /// Returns the `char` corresponding to this coordinate.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::new(0).unwrap();
    /// assert_eq!(c.to_char(), '가');
    /// ```
    #[inline]
    pub const fn to_char(self) -> char {
        // SAFETY: self.0 is guaranteed < 11172, so BASE + self.0 is in U+AC00..U+D7AF,
        // which is a valid Unicode scalar value.
        unsafe { char::from_u32_unchecked(self.to_code_point() as u32) }
    }

    /// Decomposes this coordinate into its three structural axes:
    /// `(initial, medial, final)`.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::from_char('한').unwrap();
    /// assert_eq!(c.to_axes(), (18, 0, 4));
    /// ```
    #[inline]
    pub const fn to_axes(self) -> (u8, u8, u8) {
        let v = self.0 as usize;
        let initial = (v / Self::STRIDE_INIT) as u8;
        let rem = v % Self::STRIDE_INIT;
        let medial = (rem / Self::STRIDE_MED) as u8;
        let final_ = (rem % Self::STRIDE_MED) as u8;
        (initial, medial, final_)
    }

    /// Returns the Hangul syllable as a UTF-8 string.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::new(0).unwrap();
    /// assert_eq!(c.to_hangul_string(), "가");
    /// ```
    pub fn to_hangul_string(self) -> String {
        self.to_char().to_string()
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

impl core::fmt::Display for Coord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_char())
    }
}

// ---------------------------------------------------------------------------
// Hamming distance
// ---------------------------------------------------------------------------

impl Coord {
    /// Computes the field-wise Hamming distance between two coordinates.
    ///
    /// Returns `(d_initial, d_medial, d_final)`, each in 0..=max(axis).
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let a = Coord::from_axes(0, 0, 0).unwrap();
    /// let b = Coord::from_axes(3, 5, 7).unwrap();
    /// assert_eq!(a.hamming_distance(b), (3, 5, 7));
    ///
    /// assert_eq!(a.hamming_distance(a), (0, 0, 0));
    /// ```
    #[inline]
    pub const fn hamming_distance(self, other: Self) -> (u8, u8, u8) {
        let (ai, am, af) = self.to_axes();
        let (bi, bm, bf) = other.to_axes();
        (abs_diff(ai, bi), abs_diff(am, bm), abs_diff(af, bf))
    }
}

const fn abs_diff(a: u8, b: u8) -> u8 {
    a.abs_diff(b)
}

// ---------------------------------------------------------------------------
// Serialisation helpers (no_std compatible)
// ---------------------------------------------------------------------------

impl Coord {
    /// Returns the little-endian bytes of the raw index.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::new(0x2BA3).unwrap();
    /// // 0x2BA3 = 11171 = last syllable 힣
    /// assert_eq!(c.to_le_bytes(), [0xA3, 0x2B]);
    /// ```
    #[inline]
    pub const fn to_le_bytes(self) -> [u8; 2] {
        self.0.to_le_bytes()
    }

    /// Returns the big-endian bytes of the raw index.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::new(0x2BA3).unwrap();
    /// // 0x2BA3 = 11171 = last syllable 힣
    /// assert_eq!(c.to_be_bytes(), [0x2B, 0xA3]);
    /// ```
    #[inline]
    pub const fn to_be_bytes(self) -> [u8; 2] {
        self.0.to_be_bytes()
    }

    /// Creates a `Coord` from little-endian bytes.
    ///
    /// Returns `None` if the decoded value is invalid.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::from_le_bytes([0x00, 0x00]).unwrap();
    /// assert_eq!(c.to_char(), '가');
    ///
    /// let invalid = Coord::from_le_bytes([0x04, 0x2C]);
    /// // 0x2C04 = 11268, out of range
    /// assert!(invalid.is_none());
    /// ```
    #[inline]
    pub const fn from_le_bytes(bytes: [u8; 2]) -> Option<Self> {
        Self::new(u16::from_le_bytes(bytes))
    }

    /// Creates a `Coord` from big-endian bytes.
    ///
    /// Returns `None` if the decoded value is invalid.
    ///
    /// ```
    /// use tagma_core::Coord;
    ///
    /// let c = Coord::from_be_bytes([0x00, 0x00]).unwrap();
    /// assert_eq!(c.to_char(), '가');
    ///
    /// let invalid = Coord::from_be_bytes([0x2C, 0x04]);
    /// // 0x2C04 = 11268, out of range
    /// assert!(invalid.is_none());
    /// ```
    #[inline]
    pub const fn from_be_bytes(bytes: [u8; 2]) -> Option<Self> {
        Self::new(u16::from_be_bytes(bytes))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_11172_coords_are_valid() {
        for i in 0..11172u16 {
            assert!(Coord::new(i).is_some());
        }
        assert!(Coord::new(11172).is_none());
    }

    #[test]
    fn roundtrip_axes() {
        for i in 0..19 {
            for m in 0..21 {
                for f in 0..28 {
                    let c = Coord::from_axes(i, m, f).unwrap();
                    assert_eq!((i, m, f), c.to_axes());
                }
            }
        }
    }

    #[test]
    fn roundtrip_code_point() {
        for raw in [0u16, 1, 11171, 4444, 8888] {
            let c = Coord::new(raw).unwrap();
            let cp = c.to_code_point();
            let back = Coord::from_code_point(cp).unwrap();
            assert_eq!(c, back);
        }
    }

    #[test]
    fn char_roundtrip() {
        let c = Coord::from_axes(0, 0, 0).unwrap();
        assert_eq!(c.to_char(), '가');
        assert_eq!(c.to_hangul_string(), "가");

        let last = Coord::new(11171).unwrap();
        assert_eq!(last.to_char(), '힣');
    }

    #[test]
    fn hamming_distance_same() {
        let a = Coord::new(0).unwrap();
        assert_eq!(a.hamming_distance(a), (0, 0, 0));
    }

    #[test]
    fn hamming_distance_different() {
        let a = Coord::from_axes(0, 0, 0).unwrap();
        let b = Coord::from_axes(3, 5, 7).unwrap();
        assert_eq!(a.hamming_distance(b), (3, 5, 7));
    }

    #[test]
    fn coordinate_formula_smoke() {
        // 가 (U+AC00) = initial 0, medial 0, final 0
        let ga = Coord::from_char('가').unwrap();
        assert_eq!(ga.to_axes(), (0, 0, 0));

        // 힣 (U+D7A3) = initial 18, medial 20, final 27
        let hih = Coord::from_char('힣').unwrap();
        assert_eq!(hih.to_axes(), (18, 20, 27));
        assert_eq!(hih.index(), 11171);
    }
}
