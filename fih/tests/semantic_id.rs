// Semantic id layer contract tests.
//
// The semantic layer derives full-injective CoordId<20> addresses:
// content_id (SHA-256 over content hash + entity + origin + creator,
// encoded injectively into 20 base-11172 coordinates), from_label
// (content-addressed labels), resolve (canonical pass-through or label
// derivation), and the canonical-only from_string parser. These tests
// pin determinism, distinctness, and canonical form. Id coordinates are
// opaque since the migration: the old axis-layout contract is gone.

use fih_model::{Content, CoordId, Fact, FihHash};
use sha2::{Digest, Sha256};

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
