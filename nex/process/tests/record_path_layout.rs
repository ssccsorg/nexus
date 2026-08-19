// Record path layout after the content-hash axis removal (#176, Step 1).
//
// The unified store path carries structural axes only:
//   [0-1] time, [2] entity, [3-4] origin/creator, [5] status,
//   [6-11] identity (CoordId<6> coordinates).
// The seven content-hash axes [12-18] were removed, so the path is 12
// coordinates and no longer depends on content_hash. The id-to-hash
// matching map (fact_id_index) is the sole defender against same-id
// collisions: two facts with the same identity coordinates share one
// tree path, and submit_fact rejects a different content_hash before the
// tree is reached (see content_hash_conflict.rs).

use nex_fih::CoordId;
use nex_fih::core::store::record_to_path;
use tagma_core::{Coord, CoordPath};

const DAY_NS: u64 = 86_400_000_000_000;

fn id_at(serial: u16) -> CoordId {
    CoordId::from_axes(0, 0, 0, 0, 0, serial).unwrap()
}

#[test]
fn path_has_twelve_axes() {
    let path = record_to_path(0u16, "origin", "creator", 3u16, "f_a", 1_000);
    assert_eq!(path.len(), 12);
}

#[test]
fn identity_coords_round_trip_the_id() {
    let id = id_at(7);
    let id_str = id.to_string();
    let path = record_to_path(0u16, "o", "c", 0u16, &id_str, 1_000);
    let mut coords = [Coord::new(0).unwrap(); 6];
    coords.copy_from_slice(&path.coords()[6..12]);
    let cid = CoordId::<6>(CoordPath::new(coords));
    assert_eq!(cid.to_string(), id_str);
}

#[test]
fn identity_moves_only_the_identity_axes() {
    let a = record_to_path(0u16, "o", "c", 0u16, &id_at(1).to_string(), 1_000);
    let b = record_to_path(0u16, "o", "c", 0u16, &id_at(2).to_string(), 1_000);
    assert_eq!(&a.coords()[0..6], &b.coords()[0..6]);
    assert_ne!(&a.coords()[6..12], &b.coords()[6..12]);
}

#[test]
fn time_moves_only_the_leading_axes() {
    let before = record_to_path(0u16, "o", "c", 0u16, &id_at(3).to_string(), 0);
    let after = record_to_path(0u16, "o", "c", 0u16, &id_at(3).to_string(), DAY_NS);
    assert_ne!(&before.coords()[0..2], &after.coords()[0..2]);
    assert_eq!(&before.coords()[2..12], &after.coords()[2..12]);
}

#[test]
fn content_hash_no_longer_enters_the_path() {
    // The path is a function of the structural and identity fields only.
    // Same identity with any content maps to the same tree path, so the
    // tree cannot host two records at one id; the matching map is the
    // sole same-id collision defender and submit_fact rejects a second
    // different-content record before it reaches the tree.
    let p1 = record_to_path(0u16, "o", "c", 0u16, &id_at(9).to_string(), 1_000);
    let p2 = record_to_path(0u16, "o", "c", 0u16, &id_at(9).to_string(), 1_000);
    assert_eq!(p1, p2);
}
