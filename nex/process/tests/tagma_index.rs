// ── Tagma index tests (FihCoord removed — intersect helpers remain) ─────

use nex::storage::core::index::{intersect_2, intersect_3};

// ── intersect helpers ────────────────────────────────────────────────

#[test]
fn intersect_2_basic() {
    let a = vec![1, 2, 3, 4];
    let b = vec![3, 4, 5, 6];
    let mut result = intersect_2(&a, &b);
    result.sort();
    assert_eq!(result, vec![3, 4]);
}

#[test]
fn intersect_2_empty() {
    assert!(intersect_2(&[], &[1, 2, 3]).is_empty());
    assert!(intersect_2(&[1, 2, 3], &[]).is_empty());
}

#[test]
fn intersect_2_no_overlap() {
    assert!(intersect_2(&[1, 2], &[3, 4]).is_empty());
}

#[test]
fn intersect_3_basic() {
    let a = vec![1, 2, 3, 4];
    let b = vec![2, 3, 4, 5];
    let c = vec![3, 4, 5, 6];
    let mut result = intersect_3(&a, &b, &c);
    result.sort();
    assert_eq!(result, vec![3, 4]);
}

#[test]
fn intersect_3_empty() {
    assert!(intersect_3(&[], &[1, 2], &[2, 3]).is_empty());
}

#[test]
fn intersect_3_no_overlap() {
    assert!(intersect_3(&[1], &[2], &[3]).is_empty());
}
