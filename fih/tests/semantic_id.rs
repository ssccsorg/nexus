// Semantic id layer contract tests.
//
// The semantic layer derives full-injective CoordId<20> addresses:
// content_id (SHA-256 over content hash + entity + origin + creator,
// encoded injectively into 20 base-11172 coordinates), from_label
// (content-addressed labels), resolve (canonical pass-through or label
// derivation), and the canonical-only from_string parser. These tests
// pin determinism, distinctness, and canonical form. Id coordinates are
// opaque since the migration: the old axis-layout contract is gone.

use fih_model::fih::encode_hash_into_coords;
use fih_model::{Content, CoordId, Fact, FihHash};
use sha2::{Digest, Sha256};
use tagma_core::{Coord, CoordPath};

fn hash(data: &[u8]) -> FihHash {
    FihHash(Sha256::digest(data).into())
}

#[test]
fn content_id_is_deterministic_per_inputs() {
    let a = CoordId::content_id(0, "origin", "creator", &hash(b"content"));
    let b = CoordId::content_id(0, "origin", "creator", &hash(b"content"));
    assert_eq!(a, b, "same inputs must produce the same id");
}

#[test]
fn content_id_distinguishes_content() {
    let a = CoordId::content_id(0, "origin", "creator", &hash(b"alpha"));
    let b = CoordId::content_id(0, "origin", "creator", &hash(b"beta"));
    assert_ne!(a, b, "different content must produce different ids");
}

#[test]
fn content_id_distinguishes_origin_and_creator() {
    let a = CoordId::content_id(0, "origin-a", "creator", &hash(b"x"));
    let b = CoordId::content_id(0, "origin-b", "creator", &hash(b"x"));
    assert_ne!(a, b, "different origin must produce different ids");
    let c = CoordId::content_id(0, "origin-a", "creator-1", &hash(b"x"));
    let d = CoordId::content_id(0, "origin-a", "creator-2", &hash(b"x"));
    assert_ne!(c, d, "different creator must produce different ids");
}

#[test]
fn content_id_is_canonical() {
    let id = CoordId::content_id(0, "origin", "creator", &hash(b"x"));
    let s = id.to_string();
    assert_eq!(
        s.chars().count(),
        20,
        "canonical id is 20 Hangul characters"
    );
    assert_eq!(
        CoordId::<20>::from_string(&s),
        Some(id),
        "canonical string round-trips"
    );
}

#[test]
fn from_label_is_deterministic_and_canonical() {
    let a = CoordId::from_label("fixture-a");
    let b = CoordId::from_label("fixture-a");
    assert_eq!(a, b, "same label must produce the same id");
    assert_ne!(a, CoordId::from_label("fixture-b"));
    let s = a.to_string();
    assert_eq!(s.chars().count(), 20);
    assert_eq!(CoordId::<20>::from_string(&s), Some(a));
}

#[test]
fn resolve_accepts_canonical_and_label_equivalently() {
    let label = "some-label";
    let derived = CoordId::from_label(label);
    let canonical = derived.to_string();
    assert_eq!(
        CoordId::resolve(label),
        derived,
        "label resolves to its derivation"
    );
    assert_eq!(
        CoordId::resolve(&canonical),
        derived,
        "canonical form resolves identically"
    );
    assert_eq!(
        CoordId::resolve(&canonical),
        CoordId::<20>::from_string(&canonical).unwrap()
    );
}

#[test]
fn from_string_is_canonical_only() {
    let canonical = CoordId::from_label("fixture");
    let s = canonical.to_string();
    assert_eq!(s.chars().count(), 20);
    assert_eq!(
        CoordId::<20>::from_string(&s),
        Some(canonical),
        "canonical 20-char string round-trips"
    );
    assert_eq!(CoordId::<20>::from_string("not-hangul-id"), None);
    assert_eq!(CoordId::<20>::from_string(""), None);
    assert_eq!(
        CoordId::<20>::from_string("가나다라마바"),
        None,
        "a 6-Hangul string is a label at depth 20, not canonical"
    );
}

#[test]
fn fact_new_is_content_addressed() {
    let f1 = Fact::new("origin".into(), Content::from("hello"), "creator".into());
    let f2 = Fact::new("origin".into(), Content::from("hello"), "creator".into());
    assert_eq!(
        f1.id, f2.id,
        "same content+origin+creator produces the same id"
    );
    let f3 = Fact::new("origin".into(), Content::from("world"), "creator".into());
    assert_ne!(f1.id, f3.id, "different content produces a different id");
    assert_eq!(
        f1.id.to_string().chars().count(),
        20,
        "fact id is canonical 20 Hangul"
    );
}

// ── Full-injective encoding (Step 4) ──────────────────────────────────

#[test]
fn encode_hash_is_injective() {
    // Deterministic distinct digests covering the edges: zero, max, the
    // top bit, one in the lowest byte, near-max, and a spread.
    let mut digests: Vec<[u8; 32]> = Vec::new();
    digests.push([0u8; 32]);
    digests.push([0xFFu8; 32]);
    let mut one = [0u8; 32];
    one[31] = 1;
    digests.push(one);
    let mut top_bit = [0u8; 32];
    top_bit[0] = 0x80;
    digests.push(top_bit);
    let mut near_max = [0xFFu8; 32];
    near_max[31] = 0xFE;
    digests.push(near_max);
    for i in 0..64u8 {
        let mut d = [0u8; 32];
        d[0] = i;
        d[31] = i.wrapping_mul(7).wrapping_add(3);
        digests.push(d);
    }

    let ids: Vec<CoordId> = digests
        .iter()
        .map(|d| CoordId(CoordPath::new(encode_hash_into_coords(d))))
        .collect();
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(
                ids[i], ids[j],
                "distinct 256-bit digests {i} and {j} must map to distinct ids"
            );
        }
    }
}

#[test]
fn encode_hash_uses_the_20th_digit_for_256_bit_values() {
    // The maximum 256-bit value needs the 20th digit: 19 coords carry
    // only 2^255.5, so the top bit would be lost. A non-zero leading
    // coord proves the full width is in use.
    let max = encode_hash_into_coords(&[0xFFu8; 32]);
    assert!(
        max[0].index() > 0,
        "leading coord must be non-zero for the max 256-bit value"
    );

    // Zero maps to the all-zero path (canonical 20 Hangul '가' chars).
    let zero = encode_hash_into_coords(&[0u8; 32]);
    assert!(zero.iter().all(|c| c.index() == 0));
    let id: CoordId = CoordId(CoordPath::new(zero));
    assert_eq!(id.to_string(), "가".repeat(20));
}

#[test]
fn coord_id_20_serde_round_trip() {
    let id = CoordId::from_label("serde-fixture");
    let json = serde_json::to_string(&id).unwrap();
    let back: CoordId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back, "CoordId<20> serde round-trip");

    let fact = Fact::new("origin".into(), Content::from("payload"), "creator".into());
    let fjson = serde_json::to_string(&fact).unwrap();
    let fback: Fact = serde_json::from_str(&fjson).unwrap();
    assert_eq!(fact.id, fback.id);
    assert_eq!(fact.content_hash, fback.content_hash);
    assert_eq!(fact.origin, fback.origin);
}

#[test]
fn six_syllable_strings_are_labels_after_the_depth_migration() {
    // A legacy 6-syllable canonical id is a label at depth 20: resolve
    // derives a different 20-syllable id instead of passing through.
    // This pins the breaking migration boundary of the CoordId<20>
    // switch: old persisted ids are not addressable by their old
    // spelling.
    let legacy = "가나다라마바";
    assert_eq!(
        CoordId::<20>::from_string(legacy),
        None,
        "6 syllables are not canonical at depth 20"
    );
    let resolved = CoordId::resolve(legacy);
    assert_eq!(resolved.to_string().chars().count(), 20);
    assert_ne!(resolved.to_string(), legacy);
}
