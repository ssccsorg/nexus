pub const BASE: u32 = 0xAC00;
pub const N_SYLLABLES: u32 = 11172;

/// Encode a 16-bit value into two Hangul syllables (base-11172 representation).
pub fn encode_u16(v: u16) -> [char; 2] {
    let hi = (v as u32 / N_SYLLABLES) % N_SYLLABLES;
    let lo = (v as u32) % N_SYLLABLES;
    [
        char::from_u32(BASE + hi).expect("valid Hangul syllable range"),
        char::from_u32(BASE + lo).expect("valid Hangul syllable range"),
    ]
}

/// Encode a byte slice into a Hangul string, 2 bytes per syllable pair.
pub fn encode_bytes(bytes: &[u8]) -> String {
    let n_pairs = bytes.len().div_ceil(2);
    let mut out = String::with_capacity(n_pairs * 2 * 3);
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

/// Decode a pair of Hangul syllables back to a 16-bit value.
pub fn decode_pair(c0: char, c1: char) -> Option<u16> {
    let u0 = c0 as u32;
    let u1 = c1 as u32;
    if !(BASE..BASE + N_SYLLABLES).contains(&u0)
        || !(BASE..BASE + N_SYLLABLES).contains(&u1)
    {
        return None;
    }
    let hi = u0 - BASE;
    let lo = u1 - BASE;
    Some((hi * N_SYLLABLES + lo) as u16)
}

/// Decode a Hangul string back to bytes, 2 syllables per u16 pair.
pub fn decode_bytes(s: &str) -> Option<Vec<u8>> {
    let mut chars = s.chars();
    let mut out = Vec::with_capacity(s.len() * 2);
    while let Some(c0) = chars.next() {
        let c1 = chars.next()?;
        let v = decode_pair(c0, c1)?;
        out.extend_from_slice(&v.to_le_bytes());
    }
    Some(out)
}

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
}
