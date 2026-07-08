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
    pub fn new(initial: u8, medial: u8, final_: u8) -> Option<Self> {
        if initial >= N_INIT || medial >= N_MED || final_ >= N_FIN {
            return None;
        }
        let cp = BASE + (initial as u16) * M1 + (medial as u16) * M2 + final_ as u16;
        Some(Self(cp))
    }

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

    pub const fn code_point(&self) -> u16 { self.0 }

    pub fn decompose(&self) -> (u8, u8, u8) {
        let offset = self.0 - BASE;
        let initial = (offset / M1) as u8;
        let rem = offset % M1;
        let medial = (rem / M2) as u8;
        let final_ = (rem % M2) as u8;
        (initial, medial, final_)
    }

    pub fn validate(cp: u16) -> bool {
        Self::from_code_point(cp).is_some()
    }

    pub fn hamming_distance(&self, other: &Self) -> (u8, u8, u8) {
        let (ai, am, af) = self.decompose();
        let (bi, bm, bf) = other.decompose();
        (ai.abs_diff(bi), am.abs_diff(bm), af.abs_diff(bf))
    }

    pub fn to_char(&self) -> char {
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
    fn from(c: TagmaCoord) -> Self { c.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_decompose_roundtrip() {
        for i in 0..19 {
            for m in 0..21 {
                for f in 0..28 {
                    let coord = TagmaCoord::new(i, m, f).unwrap();
                    assert_eq!(coord.decompose(), (i, m, f));
                }
            }
        }
    }

    #[test]
    fn boundary_values() {
        let first = TagmaCoord::new(0, 0, 0).unwrap();
        assert_eq!(first.code_point(), 0xAC00);
        assert_eq!(first.to_char(), '\u{AC00}');

        let last = TagmaCoord::new(18, 20, 27).unwrap();
        assert_eq!(last.code_point(), 0xD7A3);
        assert_eq!(last.to_char(), '\u{D7A3}');
    }

    #[test]
    fn invalid_indices() {
        assert!(TagmaCoord::new(19, 0, 0).is_none());
        assert!(TagmaCoord::new(0, 21, 0).is_none());
        assert!(TagmaCoord::new(0, 0, 28).is_none());
    }

    #[test]
    fn from_code_point() {
        assert_eq!(TagmaCoord::from_code_point(0xAC00).unwrap().decompose(), (0, 0, 0));
        assert_eq!(TagmaCoord::from_code_point(0xAC01).unwrap().decompose(), (0, 0, 1));
        assert!(TagmaCoord::from_code_point(0xD7A4).is_none());
        assert!(TagmaCoord::from_code_point(0xD7AF).is_none());
    }

    #[test]
    fn out_of_range() {
        assert!(TagmaCoord::from_code_point(0xABFF).is_none());
        assert!(TagmaCoord::from_code_point(0xD7B0).is_none());
    }

    #[test]
    fn hamming_distance() {
        let a = TagmaCoord::new(0, 0, 0).unwrap();
        let b = TagmaCoord::new(0, 0, 1).unwrap();
        assert_eq!(a.hamming_distance(&b), (0, 0, 1));
        let c = TagmaCoord::new(5, 3, 7).unwrap();
        let d = TagmaCoord::new(2, 8, 7).unwrap();
        assert_eq!(c.hamming_distance(&d), (3, 5, 0));
    }

    #[test]
    fn count_11k_valid() {
        let mut count = 0;
        for cp in 0xAC00..=0xD7A3 {
            if TagmaCoord::from_code_point(cp).is_some() {
                count += 1;
            }
        }
        assert_eq!(count, 11_172);
    }
}
