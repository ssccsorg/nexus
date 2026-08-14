// Semantic id layer contract tests.
//
// The semantic layer maps domain meaning onto CoordId axes before
// insertion: content_id (entity/origin/creator/content axes), from_label
// (content-addressed labels), resolve (canonical pass-through or label
// derivation), and the canonical-only from_string parser. These tests
// pin determinism, distinctness, axis structure, and canonical form.

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
fn content_id_axis_structure() {
    // Entity kind occupies axis 2; origin and creator fingerprints
    // occupy axes 3 and 4.
    let fact = CoordId::content_id(0, "origin", "creator", &hash(b"x"));
    let intent = CoordId::content_id(1, "origin", "creator", &hash(b"x"));
    assert_eq!(fact.coord_at(2).index(), 0, "fact entity axis is 0");
    assert_eq!(intent.coord_at(2).index(), 1, "intent entity axis is 1");
    assert_eq!(fact.coord_at(3), intent.coord_at(3), "same origin axis");
    assert_eq!(fact.coord_at(4), intent.coord_at(4), "same creator axis");
    let other = CoordId::content_id(0, "other", "creator", &hash(b"x"));
    assert_ne!(fact.coord_at(3), other.coord_at(3), "origin axis differs");
}

#[test]
fn content_id_is_canonical() {
    let id = CoordId::content_id(0, "origin", "creator", &hash(b"x"));
    let s = id.to_string();
    assert_eq!(s.chars().count(), 6, "canonical id is 6 Hangul characters");
    assert_eq!(
        CoordId::<6>::from_string(&s),
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
    assert_eq!(s.chars().count(), 6);
    assert_eq!(CoordId::<6>::from_string(&s), Some(a));
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
        CoordId::<6>::from_string(&canonical).unwrap()
    );
}

#[test]
fn from_string_is_canonical_only() {
    let canonical = CoordId::new(42);
    assert_eq!(
        CoordId::<6>::from_string(&canonical.to_string()),
        Some(canonical)
    );
    assert_eq!(CoordId::<6>::from_string("not-hangul-id"), None);
    assert_eq!(CoordId::<6>::from_string(""), None);
    assert_eq!(
        CoordId::<6>::from_string("가나다라마"),
        None,
        "wrong length is not canonical"
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
    assert_eq!(f1.id.to_string().chars().count(), 6, "fact id is canonical");
}
