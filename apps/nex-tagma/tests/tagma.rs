use nex_tagma::Coord;
use std::process::Command;

fn nex_tagma_bin() -> &'static str {
    env!("CARGO_BIN_EXE_nex-tagma")
}

#[test]
fn compose_decompose_roundtrip() {
    for i in 0..19 {
        for m in 0..21 {
            for f in 0..28 {
                let coord = Coord::from_axes(i, m, f).unwrap();
                assert_eq!(coord.to_axes(), (i, m, f));
            }
        }
    }
}

#[test]
fn boundary_values() {
    let first = Coord::from_axes(0, 0, 0).unwrap();
    assert_eq!(first.to_code_point(), 0xAC00);
    assert_eq!(first.to_char(), '\u{AC00}');

    let last = Coord::from_axes(18, 20, 27).unwrap();
    assert_eq!(last.to_code_point(), 0xD7A3);
    assert_eq!(last.to_char(), '\u{D7A3}');
}

#[test]
fn invalid_indices() {
    assert!(Coord::from_axes(19, 0, 0).is_none());
    assert!(Coord::from_axes(0, 21, 0).is_none());
    assert!(Coord::from_axes(0, 0, 28).is_none());
}

#[test]
fn from_code_point() {
    assert_eq!(Coord::from_code_point(0xAC00).unwrap().to_axes(), (0, 0, 0));
    assert_eq!(Coord::from_code_point(0xAC01).unwrap().to_axes(), (0, 0, 1));
    // Filler positions (U+D7A4..U+D7AF) are within the Unicode block
    // and are accepted by from_code_point; they are rejected by new().
    assert!(Coord::from_code_point(0xD7A4).is_some());
    // Out-of-block values are always rejected.
    assert!(Coord::from_code_point(0xABFF).is_none());
    assert!(Coord::from_code_point(0xD7B0).is_none());
}

#[test]
fn hamming_distance() {
    let a = Coord::from_axes(0, 0, 0).unwrap();
    let b = Coord::from_axes(0, 0, 1).unwrap();
    assert_eq!(a.hamming_distance(b), (0, 0, 1));

    let c = Coord::from_axes(5, 3, 7).unwrap();
    let d = Coord::from_axes(2, 8, 7).unwrap();
    assert_eq!(c.hamming_distance(d), (3, 5, 0));
}

#[test]
fn count_11k_valid() {
    let mut count = 0;
    for cp in 0xAC00..=0xD7A3 {
        if Coord::from_code_point(cp).is_some() {
            count += 1;
        }
    }
    assert_eq!(count, 11_172);
}

#[test]
fn validate_function() {
    assert!(nex_tagma::validate(0xAC00));
    assert!(nex_tagma::validate(0xD7A3));
    // Filler positions are within the Unicode block, so from_code_point accepts them.
    assert!(nex_tagma::validate(0xD7A4));
    assert!(nex_tagma::validate(0xD7AF));
    assert!(!nex_tagma::validate(0xABFF));
    assert!(!nex_tagma::validate(0xD7B0));
}

#[test]
fn display_format() {
    let coord = Coord::from_axes(0, 0, 0).unwrap();
    assert_eq!(coord.to_string(), "가");

    let coord = Coord::from_axes(5, 10, 15).unwrap();
    let s = coord.to_string();
    assert_eq!(s.chars().count(), 1);
    assert!(s.chars().all(|c| c as u32 >= 0xAC00 && c as u32 <= 0xD7AF));
}

#[test]
fn dense_index_roundtrip() {
    let mut seen = std::collections::HashSet::new();
    for i in 0..19 {
        for m in 0..21 {
            for f in 0..28 {
                let coord = Coord::from_axes(i, m, f).unwrap();
                let idx = coord.index() as usize;
                assert!(idx < 11172, "index {idx} out of range");
                assert!(seen.insert(idx), "duplicate index {idx} at ({i},{m},{f})");
            }
        }
    }
    assert_eq!(seen.len(), 11172);
}

#[test]
fn dense_index_zero() {
    let coord = Coord::from_axes(0, 0, 0).unwrap();
    assert_eq!(coord.index() as usize, 0);
}

#[test]
fn dense_index_max() {
    let coord = Coord::from_axes(18, 20, 27).unwrap();
    assert_eq!(coord.index() as usize, 11171);
}

#[test]
fn from_trait_u16() {
    let coord = Coord::from_axes(0, 0, 0).unwrap();
    assert_eq!(coord.index(), 0);

    let coord = Coord::from_axes(18, 20, 27).unwrap();
    assert_eq!(coord.index(), 11171);
}

#[test]
fn hamming_distance_max() {
    let a = Coord::from_axes(0, 0, 0).unwrap();
    let b = Coord::from_axes(18, 20, 27).unwrap();
    assert_eq!(a.hamming_distance(b), (18, 20, 27));
}

#[test]
fn hamming_distance_self() {
    let a = Coord::from_axes(5, 10, 15).unwrap();
    assert_eq!(a.hamming_distance(a), (0, 0, 0));
}

#[test]
fn parse_val_single_char() {
    let output = Command::new(nex_tagma_bin())
        .args(["check", "가"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("valid"));
    assert!(out.contains("U+AC00"));
}

#[test]
fn parse_val_hex() {
    let output = Command::new(nex_tagma_bin())
        .args(["check", "AC01"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("valid"));
}

#[test]
fn parse_val_hex_prefix() {
    let output = Command::new(nex_tagma_bin())
        .args(["check", "0xD7A3"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("힣"));
}

#[test]
fn parse_val_invalid() {
    let output = Command::new(nex_tagma_bin())
        .args(["check", "invalid"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn check_no_arg() {
    let output = Command::new(nex_tagma_bin())
        .args(["check"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("error") || err.contains("Usage"));
}

#[test]
fn compose_invalid_axes() {
    let output = Command::new(nex_tagma_bin())
        .args(["compose", "19", "0", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn compose_wrong_arg_type() {
    let output = Command::new(nex_tagma_bin())
        .args(["compose", "x", "0", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn decompose_invalid_input() {
    let output = Command::new(nex_tagma_bin())
        .args(["decompose", "invalid"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn decompose_out_of_range() {
    let output = Command::new(nex_tagma_bin())
        .args(["decompose", "FFFF"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn dist_one_invalid() {
    let output = Command::new(nex_tagma_bin())
        .args(["dist", "가", "FFFF"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn dist_both_invalid() {
    let output = Command::new(nex_tagma_bin())
        .args(["dist", "FFFF", "FFFE"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn dist_missing_args() {
    let output = Command::new(nex_tagma_bin())
        .args(["dist", "가"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn help_flag_exit_zero() {
    let output = Command::new(nex_tagma_bin())
        .args(["--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let out = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(out.contains("Usage") || err.contains("Usage"));
}

#[test]
fn no_args_exit_nonzero() {
    let output = Command::new(nex_tagma_bin()).output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn unknown_command() {
    let output = Command::new(nex_tagma_bin())
        .args(["unknown"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("unknown command"));
}

#[test]
fn to_code_point_standalone() {
    let c = Coord::from_axes(5, 10, 15).unwrap();
    assert_eq!(c.to_code_point(), 0xAC00 + 5 * 588 + 10 * 28 + 15);
}

#[test]
fn to_char_standalone() {
    let c = Coord::from_axes(0, 0, 0).unwrap();
    assert_eq!(c.to_char(), '가');
    let c = Coord::from_axes(18, 20, 27).unwrap();
    assert_eq!(c.to_char(), '힣');
}

#[test]
fn bench_runs() {
    let output = Command::new(nex_tagma_bin())
        .args(["bench"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("1-syll:"), "missing 1-syll speedup");
    assert!(out.contains("6-syll:"), "missing 6-syll speedup");
    assert!(out.contains("19-syll:"), "missing 19-syll speedup");

    if let Some(line) = out.lines().find(|l| l.contains("19-syll:")) {
        let after_colon = line.split(':').nth(1).unwrap_or("");
        let num_str = after_colon.split('x').next().unwrap_or("").trim();
        let val: f64 = num_str.parse().unwrap_or(0.0);
        assert!(val > 1.0, "19-syll speedup {val}x should be > 1x");
    }
}
