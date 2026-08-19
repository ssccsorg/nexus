// Record path layout after the L2 restructure (#176, Step 2).
//
// The unified filter index path carries structural axes only:
//   [0-1] time, [2] entity, [3-4] origin/creator, [5] status.
// The path carries no id and no content hash: the tree value is the set
// of record ids at the structural path, so memory is bounded by axis
// cardinality, not record count. Record bodies live in the application
// layer, and the id-to-hash matching map (fact_id_index) is the sole
// defender against same-id collisions.

use nex_fih::core::store::structural_path;

const DAY_NS: u64 = 86_400_000_000_000;

#[test]
fn path_has_six_axes() {
    let path = structural_path(0u16, "origin", "creator", 3u16, 1_000);
    assert_eq!(path.len(), 6);
}

#[test]
fn time_moves_only_the_leading_axes() {
    let before = structural_path(0u16, "o", "c", 0u16, 0);
    let after = structural_path(0u16, "o", "c", 0u16, DAY_NS);
    assert_ne!(&before.coords()[0..2], &after.coords()[0..2]);
    assert_eq!(&before.coords()[2..6], &after.coords()[2..6]);
}

#[test]
fn entity_moves_only_axis_two() {
    let fact = structural_path(0u16, "o", "c", 0u16, 1_000);
    let intent = structural_path(1u16, "o", "c", 0u16, 1_000);
    assert_ne!(fact.coords()[2], intent.coords()[2]);
    assert_eq!(&fact.coords()[0..2], &intent.coords()[0..2]);
    assert_eq!(&fact.coords()[3..6], &intent.coords()[3..6]);
}

#[test]
fn status_moves_only_axis_five() {
    let submitted = structural_path(1u16, "o", "c", 0u16, 1_000);
    let concluded = structural_path(1u16, "o", "c", 2u16, 1_000);
    assert_ne!(submitted.coords()[5], concluded.coords()[5]);
    assert_eq!(&submitted.coords()[0..5], &concluded.coords()[0..5]);
}

#[test]
fn ids_do_not_enter_the_path() {
    // Two records that differ only in id map to the same structural path:
    // the tree cannot distinguish them, so the id set at the path holds
    // both and the matching map is the sole same-id collision defender.
    let p1 = structural_path(0u16, "o", "c", 0u16, 1_000);
    let p2 = structural_path(0u16, "o", "c", 0u16, 1_000);
    assert_eq!(p1, p2);
}
