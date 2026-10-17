// The model's own surface: identifiers, hashes, content, and errors.
//
// These are the types every layer above matches on, and the parts a record layer does not
// exercise for them are the parts a consumer does: hashing an id, encoding and decoding
// it as text, building and parsing a hash, and rendering content by its mime type.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use fih_model::{BlackboardError, Content, CoordId, FihHash};

#[test]
fn a_coord_id_hashes_by_its_indices() {
    let id = CoordId::<6>::from_axes(10, 20, 0, 4, 5, 6).expect("the axes are in range");

    let mut expected = DefaultHasher::new();
    for coord in id.0.iter() {
        coord.index().hash(&mut expected);
    }
    let mut actual = DefaultHasher::new();
    id.hash(&mut actual);
    assert_eq!(actual.finish(), expected.finish());
}

#[test]
fn a_content_hash_covers_every_index_and_is_stable() {
    let id = CoordId::<6>::from_axes(10, 20, 0, 4, 5, 6).expect("the axes are in range");
    let other = CoordId::<6>::from_axes(10, 20, 0, 4, 5, 7).expect("the axes are in range");

    assert_eq!(id.to_content_hash(), id.to_content_hash());
    assert_ne!(id.to_content_hash(), other.to_content_hash());
}

#[test]
fn the_six_axis_surface_builds_and_reads_back() {
    let zero = CoordId::<6>::new(0);
    assert_eq!(zero.axis(0).index(), 0);

    // The counter fills the least significant coordinate first.
    let one_place = CoordId::<6>::new(11172);
    assert_eq!(one_place.axis(0).index(), 0);
    assert_eq!(one_place.axis(1).index(), 1);
    assert_eq!(one_place.coord_at(1).index(), 1);

    let explicit =
        CoordId::<6>::from_indices([1, 2, 3, 4, 5, 6]).expect("the indices are in range");
    assert_eq!(explicit.axis(2).index(), 3);
    assert!(
        CoordId::<6>::from_indices([11172, 0, 0, 0, 0, 0]).is_none(),
        "an index outside the alphabet is refused"
    );

    let timestamp = CoordId::<6>::with_timestamp(2 * 86_400_000_000_000 + 5, 1, 2, 3, 4);
    assert_eq!(timestamp.time_hi(), 2);
    assert_eq!(timestamp.axis(1).index(), 5);
    assert_eq!(timestamp.entity_type(), 1);
}

#[test]
fn a_six_axis_id_round_trips_through_text_and_serde() {
    let id = CoordId::<6>::from_axes(1, 2, 3, 4, 5, 6).expect("the axes are in range");
    let text = id.to_string();
    assert_eq!(text.chars().count(), 6);

    let json = serde_json::to_string(&id).expect("the id serializes");
    assert_eq!(json, format!("\"{text}\""));
    assert_eq!(
        serde_json::from_str::<CoordId<6>>(&json).expect("the id decodes"),
        id
    );

    let error = serde_json::from_str::<CoordId<6>>("\"abc\"")
        .expect_err("a short id is refused")
        .to_string();
    assert!(error.contains("expected 6 chars"), "{error}");
    let error = serde_json::from_str::<CoordId<6>>("\"abcdef\"")
        .expect_err("a non-coordinate character is refused")
        .to_string();
    assert!(error.contains("not a valid coordinate"), "{error}");
}

#[test]
fn a_twenty_axis_id_round_trips_and_refuses_malformed_input() {
    let id = CoordId::<20>::from_string(&"가".repeat(20)).expect("the canonical id parses");
    let text = id.to_string();
    assert_eq!(text.chars().count(), 20);

    let json = serde_json::to_string(&id).expect("the id serializes");
    assert_eq!(
        serde_json::from_str::<CoordId<20>>(&json).expect("the id decodes"),
        id
    );

    let error = serde_json::from_str::<CoordId<20>>("\"short\"")
        .expect_err("a short id is refused")
        .to_string();
    assert!(error.contains("expected 20 chars"), "{error}");
    let error = serde_json::from_str::<CoordId<20>>(&format!("\"{}\"", "a".repeat(20)))
        .expect_err("a non-coordinate character is refused")
        .to_string();
    assert!(error.contains("not a valid coordinate"), "{error}");
}

#[test]
fn a_fih_hash_is_built_from_fields_and_chains() {
    let a = FihHash::new(&["one", "two"], "fact");
    assert_eq!(a, FihHash::new(&["one", "two"], "fact"));
    assert_ne!(a, FihHash::new(&["one"], "fact"));
    assert_ne!(a, FihHash::new(&["one", "two"], "hint"));

    let b = FihHash::new(&["three"], "intent");
    let chained = FihHash::chain(&a, &b, &a);
    assert_eq!(chained, FihHash::chain(&a, &b, &a));
    assert_ne!(chained, FihHash::chain(&b, &a, &a));
}

#[test]
fn a_fih_hash_parses_hex_and_falls_back_to_hashing_the_input() {
    let hex = "0f".repeat(32);
    let parsed = FihHash::from_hex(&hex);
    assert_eq!(parsed.to_string(), hex, "64 hex characters are parsed");

    let mixed = format!("{hex} zzz");
    assert_eq!(
        FihHash::from_hex(&mixed).to_string(),
        hex,
        "non-hex characters are filtered before parsing"
    );

    let fallback = FihHash::from_hex("not a hash at all");
    assert_eq!(fallback.to_string().len(), 64);
    assert_ne!(fallback.to_string(), "not a hash at all");
}

#[test]
fn a_fih_hash_round_trips_through_serde_and_refuses_malformed_hex() {
    let hash = FihHash::new(&["content"], "fact");
    let json = serde_json::to_string(&hash).expect("the hash serializes");
    assert_eq!(
        serde_json::from_str::<FihHash>(&json).expect("the hash decodes"),
        hash
    );

    let error = serde_json::from_str::<FihHash>("\"abc\"")
        .expect_err("a short hash is refused")
        .to_string();
    assert!(error.contains("expected 64 hex"), "{error}");
}

#[test]
fn content_renders_by_its_mime_type() {
    let text = Content::from("hello");
    assert_eq!(text.as_str(), Some("hello"));
    assert_eq!(text.to_string(), "hello");

    let json = Content {
        mime_type: "application/json".into(),
        data: br#"{"a":1}"#.to_vec(),
    };
    assert_eq!(json.as_str(), Some(r#"{"a":1}"#));

    let binary = Content {
        mime_type: "application/octet-stream".into(),
        data: vec![1, 2, 3],
    };
    assert_eq!(binary.as_str(), None);
    assert_eq!(binary.to_string(), "<application/octet-stream: 3 bytes>");

    let invalid = Content {
        mime_type: "text/plain".into(),
        data: vec![0xff, 0xfe],
    };
    assert!(invalid.as_str().is_none());
    assert_eq!(invalid.to_string(), "<invalid utf-8 for text/plain>");
}

#[test]
fn every_blackboard_error_renders_its_category() {
    for (error, rendered) in [
        (BlackboardError::NotFound("x".into()), "not found: x"),
        (BlackboardError::Conflict("x".into()), "conflict: x"),
        (BlackboardError::Forbidden("x".into()), "forbidden: x"),
        (BlackboardError::BadRequest("x".into()), "bad request: x"),
        (BlackboardError::Internal("x".into()), "internal: x"),
    ] {
        assert_eq!(error.to_string(), rendered);
    }
}

#[test]
fn content_converts_from_owned_text_and_compares_to_it() {
    let owned: Content = String::from("owned").into();
    assert_eq!(owned.as_str(), Some("owned"));

    let text = Content::from("hello");
    assert!(text == "hello");
    assert!(text != "other");

    let binary = Content {
        mime_type: "application/octet-stream".into(),
        data: b"hello".to_vec(),
    };
    assert!(
        binary != "hello",
        "the comparison also checks the mime type"
    );
}
