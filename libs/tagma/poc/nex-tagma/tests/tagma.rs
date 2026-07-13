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
    assert_eq!(
        TagmaCoord::from_code_point(0xAC00).unwrap().decompose(),
        (0, 0, 0)
    );
    assert_eq!(
        TagmaCoord::from_code_point(0xAC01).unwrap().decompose(),
        (0, 0, 1)
    );
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
fn dense_index_roundtrip() {
    let mut seen = std::collections::HashSet::new();
    for i in 0..19 {
        for m in 0..21 {
            for f in 0..28 {
                let coord = TagmaCoord::new(i, m, f).unwrap();
                let idx = coord.to_dense_index();
                assert!(idx < 11172, "index {idx} out of range");
                assert!(seen.insert(idx), "duplicate index {idx} at ({i},{m},{f})");
            }
        }
    }
    assert_eq!(seen.len(), 11172);
}

#[test]
fn dense_index_zero() {
    let coord = TagmaCoord::new(0, 0, 0).unwrap();
    assert_eq!(coord.to_dense_index(), 0);
}

#[test]
fn dense_index_max() {
    let coord = TagmaCoord::new(18, 20, 27).unwrap();
    assert_eq!(coord.to_dense_index(), 11171);
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

#[test]
fn hamming_distance_max() {
    let a = TagmaCoord::new(0, 0, 0).unwrap();
    let b = TagmaCoord::new(18, 20, 27).unwrap();
    assert_eq!(a.hamming_distance(&b), (18, 20, 27));
}

#[test]
fn hamming_distance_self() {
    let a = TagmaCoord::new(5, 10, 15).unwrap();
    assert_eq!(a.hamming_distance(&a), (0, 0, 0));
}

#[test]
fn parse_val_single_char() {
    use std::process::Command;
    let output = Command::new(env!("CARGO_BIN_EXE_nex-tagma"))
        .args(["check", "가"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("valid"));
    assert!(out.contains("U+AC00"));
}

#[test]
fn parse_val_hex() {
    use std::process::Command;
    let output = Command::new(env!("CARGO_BIN_EXE_nex-tagma"))
        .args(["check", "AC01"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("valid"));
}

#[test]
fn parse_val_hex_prefix() {
    use std::process::Command;
    let output = Command::new(env!("CARGO_BIN_EXE_nex-tagma"))
        .args(["check", "0xD7A3"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("힣"));
}

#[test]
fn parse_val_invalid() {
    use std::process::Command;
    let output = Command::new(env!("CARGO_BIN_EXE_nex-tagma"))
        .args(["check", "invalid"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn bench_runs() {
    // Verify the benchmark produces plausible output
    use std::process::Command;
    let output = Command::new(env!("CARGO_BIN_EXE_nex-tagma"))
        .args(["bench"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let out = String::from_utf8_lossy(&output.stdout);
    // Verify all speedup lines are present
    assert!(out.contains("1-syll:"), "missing 1-syll speedup");
    assert!(out.contains("6-syll:"), "missing 6-syll speedup");
    assert!(out.contains("19-syll:"), "missing 19-syll speedup");

    // Parse speedup for 19-syll (same address space as SHA256)
    if let Some(line) = out.lines().find(|l| l.contains("19-syll:")) {
        // line format: "  19-syll:  6x  (space: ...)"
        let after_colon = line.split(':').nth(1).unwrap_or("");
        let num_str = after_colon.split('x').next().unwrap_or("").trim();
        let val: f64 = num_str.parse().unwrap_or(0.0);
        assert!(val > 1.0, "19-syll speedup {val}x should be > 1x");
    }
}
