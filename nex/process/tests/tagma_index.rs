// ── Tagma index tests (FihCoord removed — intersect helpers remain) ─────

use nex::storage::core::index::{OrderedIndex, intersect_2, intersect_3};

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

#[test]
fn intersect_3_walks_the_smallest_list_whichever_it_is() {
    // The helper picks the smallest list to iterate because that is the cost it pays.
    // Each arm has to select the same intersection.
    let a = vec![1, 2, 3, 4];
    let b = vec![2, 3];
    let c = vec![3, 4, 5];
    let mut middle = intersect_3(&a, &b, &c);
    middle.sort();
    assert_eq!(middle, vec![3]);

    let a = vec![1, 2, 3];
    let b = vec![2, 3, 4, 5];
    let c = vec![3];
    let mut last = intersect_3(&a, &b, &c);
    last.sort();
    assert_eq!(last, vec![3]);
}

// ── OrderedIndex ────────────────────────────────────────────────────────

#[test]
fn an_empty_index_answers_nothing() {
    let index: OrderedIndex<u64> = OrderedIndex::default();

    assert!(index.is_empty());
    assert_eq!(index.len(), 0);
    assert_eq!(index.first_key(), None);
    assert_eq!(index.last_key(), None);
    assert!(index.as_of(&10).is_empty());
    assert!(index.since(&0).is_empty());
    assert!(index.range(&0, &10).is_empty());
}

#[test]
fn an_ordered_index_filters_by_bound_and_range_in_insertion_order() {
    let mut index: OrderedIndex<u64> = OrderedIndex::new();
    for (key, id) in [(30, 3), (10, 1), (20, 2), (40, 4)] {
        index.record(key, id);
    }

    assert_eq!(index.len(), 4);
    assert_eq!(index.first_key(), Some(30));
    assert_eq!(index.last_key(), Some(40));
    // The index is append-only, so each answer keeps the order it was recorded in.
    assert_eq!(index.as_of(&20), vec![(10, 1), (20, 2)]);
    assert_eq!(index.since(&20), vec![(30, 3), (40, 4)]);
    assert_eq!(index.range(&15, &35), vec![(30, 3), (20, 2)]);

    index.clear();
    assert!(index.is_empty());
    assert_eq!(index.first_key(), None);
}
