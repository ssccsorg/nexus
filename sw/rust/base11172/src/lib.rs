//! Tagma native serialization format.
//!
//! Encodes arbitrary byte sequences into self-validating Hangul syllable
//! strings using the Tagma coordinate space as its alphabet.
//!
//! # Properties
//!
//! - **Self-validating**: invalid syllables are immediately detectable.
//! - **No special characters**: URL-safe, no escaping needed.
//! - **Deterministic**: encoding is a pure function of the input.
//! - **`no_std` compatible**: requires only `alloc`.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use tagma_core::TagmaCoord;

/// The number of distinct syllables used by this encoding (11172).
pub const N_SYLLABLES: u32 = TagmaCoord::N_VALID as u32;

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Encodes a `u16` into two Hangul syllables (base-11172 representation).
///
/// Each syllable carries log₂(11172) ≈ 13.45 bits of information,
/// so a pair covers 26.9 bits — sufficient for a full `u16`.
pub fn encode_u16(v: u16) -> [char; 2] {
    let hi = (v as u32) / N_SYLLABLES;
    let lo = (v as u32) % N_SYLLABLES;
    let c0 = TagmaCoord::new(hi as u16).unwrap_or_else(|| TagmaCoord::new(0).unwrap());
    let c1 = TagmaCoord::new(lo as u16).unwrap_or_else(|| TagmaCoord::new(0).unwrap());
    [c0.to_char(), c1.to_char()]
}

/// Encodes a byte slice into a Hangul string, 2 bytes per syllable pair.
pub fn encode_bytes(bytes: &[u8]) -> String {
    let n_pairs = bytes.len().div_ceil(2);
    let mut out = String::with_capacity(n_pairs * 2 * 3); // UTF-8: ≤3 bytes/char
    for chunk in bytes.chunks(2) {
        let v = if chunk.len() == 2 {
            u16::from_le_bytes([chunk[0], chunk[1]])
        } else {
            chunk[0] as u16
        };
        let [c0, c1] = encode_u16(v);
        out.push(c0);
        out.push(c1);
    }
    out
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Decodes a pair of Hangul syllables back to a `u16`.
///
/// Returns `None` if either character is outside the valid Hangul syllable
/// block (U+AC00..U+D7AF).
pub fn decode_pair(c0: char, c1: char) -> Option<u16> {
    let coord0 = TagmaCoord::from_char(c0)?;
    let coord1 = TagmaCoord::from_char(c1)?;
    Some((coord0.index() as u32 * N_SYLLABLES + coord1.index() as u32) as u16)
}

/// Decodes a Hangul string back to bytes, 2 syllables per `u16` pair.
///
/// Returns `None` if the string contains an odd number of syllables or any
/// invalid character.
pub fn decode_bytes(s: &str) -> Option<Vec<u8>> {
    let mut chars = s.chars();
    let mut out = Vec::with_capacity(s.len() * 2);
    while let Some(c0) = chars.next() {
        let c1 = chars.next()?; // odd count → None
        let v = decode_pair(c0, c1)?;
        out.extend_from_slice(&v.to_le_bytes());
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_u16() {
        for v in [0u16, 1, 11171, 12345, 32768, 65535] {
            let [c0, c1] = encode_u16(v);
            let decoded = decode_pair(c0, c1).unwrap();
            assert_eq!(v, decoded, "roundtrip failed for {v}");
        }
    }

    #[test]
    fn roundtrip_bytes() {
        let data = b"Hello, Hangul Base11172!";
        let encoded = encode_bytes(data);
        let decoded = decode_bytes(&encoded).unwrap();
        assert_eq!(&decoded[..data.len()], data);
    }

    #[test]
    fn binary_roundtrip() {
        let data: Vec<u8> = (0..255).collect();
        let encoded = encode_bytes(&data);
        let decoded = decode_bytes(&encoded).unwrap();
        assert_eq!(&decoded[..data.len()], &data[..]);
    }

    #[test]
    fn invalid_char_returns_none() {
        assert!(decode_pair('\u{0000}', '가').is_none());
        assert!(decode_pair('가', '\u{0000}').is_none());
        assert!(decode_pair('ힰ', '가').is_none()); // U+D7B0, just beyond block
        assert!(decode_pair('가', 'ힰ').is_none());
        assert!(decode_bytes("가").is_none()); // odd number of chars
    }

    #[test]
    fn valid_encode_uses_tagma_coord() {
        let [c0, c1] = encode_u16(0);
        assert_eq!(c0, '가'); // U+AC00 — first syllable
        assert_eq!(c1, '가');
    }
}
