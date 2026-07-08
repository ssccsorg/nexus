use nex_tagma::TagmaCoord;

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

#[test]
fn validate_function() {
    assert!(TagmaCoord::validate(0xAC00));
    assert!(TagmaCoord::validate(0xD7A3));
    assert!(!TagmaCoord::validate(0xD7A4));
    assert!(!TagmaCoord::validate(0xD7AF));
    assert!(!TagmaCoord::validate(0xABFF));
    assert!(!TagmaCoord::validate(0xD7B0));
}

#[test]
fn display_format() {
    let coord = TagmaCoord::new(0, 0, 0).unwrap();
    let s = coord.to_string();
    assert!(s.contains("U+AC00"));
    assert!(s.contains("i=0"));
    assert!(s.contains("m=0"));
    assert!(s.contains("f=0"));

    let coord = TagmaCoord::new(5, 10, 15).unwrap();
    let s = coord.to_string();
    assert!(s.contains("i=5"));
    assert!(s.contains("m=10"));
    assert!(s.contains("f=15"));
}

#[test]
fn from_trait_u16() {
    let coord = TagmaCoord::new(0, 0, 0).unwrap();
    let cp: u16 = coord.into();
    assert_eq!(cp, 0xAC00);

    let coord = TagmaCoord::new(18, 20, 27).unwrap();
    let cp: u16 = coord.into();
    assert_eq!(cp, 0xD7A3);
}
