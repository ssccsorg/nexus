// The optional ASCII rendering of a coordinate: round trip, the two properties
// it buys, the exact digits it writes, and what it refuses.

use fih_model::CoordId;
use fih_model::ascii;

fn coordinate(index: u16) -> char {
    char::from_u32(0xAC00 + index as u32).expect("a coordinate character")
}

fn id_from<const N: usize>(axes: [u16; N]) -> CoordId<N> {
    let text: String = axes.iter().map(|value| coordinate(*value)).collect();
    CoordId::from_string(&text).expect("a canonical id")
}

fn id_with(first: u16, second: u16) -> CoordId {
    let mut axes = [0u16; 20];
    axes[0] = first;
    axes[1] = second;
    id_from(axes)
}

fn first_window(value: u16) -> String {
    let mut axes = [0u16; 20];
    axes[0] = value;
    ascii::encode(&id_from(axes))[..ascii::DIGITS_PER_AXIS].to_string()
}

/// A deterministic sample, so the middle of a digit range is visited too.
fn sampled(seed: u64) -> CoordId {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut axes = [0u16; 20];
    for axis in axes.iter_mut() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *axis = ((state >> 33) % 11_172) as u16;
    }
    id_from(axes)
}

#[test]
fn a_coordinate_round_trips_through_the_rendering() {
    for (first, second) in [(0, 0), (1, 1), (11_171, 11_171), (8_100, 1_234)] {
        let id = id_with(first, second);
        let text = ascii::encode(&id);
        assert_eq!(ascii::decode::<20>(&text), Some(id));
    }
}

#[test]
fn the_rendering_is_three_characters_per_axis() {
    let text = ascii::encode(&id_with(0, 0));
    assert_eq!(text.chars().count(), ascii::DIGITS_PER_AXIS * 20);
}

#[test]
fn the_rendering_is_the_native_form_s_byte_length() {
    let id = id_with(1, 2);
    assert_eq!(ascii::encode(&id).len(), id.to_string().len());

    let small = id_from([1u16, 2, 3, 4, 5, 6]);
    assert_eq!(ascii::encode(&small).len(), small.to_string().len());
}

#[test]
fn a_prefix_of_the_text_is_a_prefix_of_the_axes() {
    let left = ascii::encode(&id_with(300, 7));
    let right = ascii::encode(&id_with(300, 9));
    assert_eq!(&left[..3], &right[..3]);
    assert_ne!(&left[3..6], &right[3..6]);
}

#[test]
fn the_text_order_matches_the_coordinate_order() {
    let mut texts: Vec<String> = (0..8).map(|i| ascii::encode(&id_with(i, 0))).collect();
    let sorted = texts.clone();
    texts.sort();
    assert_eq!(texts, sorted);
}

#[test]
fn an_uppercase_character_is_refused() {
    let mut text = ascii::encode(&id_from([11_171u16; 20]));
    assert_eq!(&text[..3], "8mb");
    text.replace_range(1..2, "M");
    assert_eq!(ascii::decode::<20>(&text), None);
}

#[test]
fn a_wrong_length_is_refused() {
    let text = ascii::encode(&id_with(1, 1));
    assert_eq!(ascii::decode::<20>(&text[..text.len() - 1]), None);
    assert_eq!(ascii::decode::<20>(&(text + "0")), None);
    assert_eq!(ascii::decode::<20>(""), None);
}

#[test]
fn the_surplus_of_the_alphabet_is_refused() {
    let text = "ZZZ".repeat(20);
    assert_eq!(ascii::decode::<20>(&text), None);
}

#[test]
fn the_native_form_is_not_read_as_a_rendering() {
    let id = id_with(5, 6);
    assert_eq!(ascii::decode::<20>(&id.to_string()), None);
}

#[test]
fn resolve_reads_the_rendering_as_a_label() {
    let id = id_with(7, 8);
    let text = ascii::encode(&id);
    assert_ne!(CoordId::resolve(&text), id);
}

#[test]
fn the_digit_carries_render_exactly() {
    assert_eq!(first_window(0), "000");
    assert_eq!(first_window(35), "00z");
    assert_eq!(first_window(36), "010");
    assert_eq!(first_window(1_295), "0zz");
    assert_eq!(first_window(1_296), "100");
    assert_eq!(first_window(11_171), "8mb");
}

#[test]
fn an_axis_position_moves_only_its_own_window() {
    let plain = ascii::encode(&id_from([0u16; 20]));
    for axis in 0..20 {
        let mut axes = [0u16; 20];
        axes[axis] = 1;
        let rendered = ascii::encode(&id_from(axes));
        let start = axis * ascii::DIGITS_PER_AXIS;
        let end = start + ascii::DIGITS_PER_AXIS;
        assert_eq!(
            &rendered[..start],
            &plain[..start],
            "axis {axis} moved an earlier window"
        );
        assert_eq!(
            &rendered[start..end],
            "001",
            "axis {axis} did not write its own window"
        );
        assert_eq!(
            &rendered[end..],
            &plain[end..],
            "axis {axis} moved a later window"
        );
    }
}

#[test]
fn every_axis_at_the_maximum_round_trips() {
    let id = id_from([11_171u16; 20]);
    let text = ascii::encode(&id);
    assert_eq!(text, "8mb".repeat(20));
    assert_eq!(ascii::decode::<20>(&text), Some(id));
}

#[test]
fn a_thousand_sampled_coordinates_round_trip() {
    for seed in 0..1_000 {
        let id = sampled(seed);
        assert_eq!(
            ascii::decode::<20>(&ascii::encode(&id)),
            Some(id),
            "seed {seed}"
        );
    }
}

#[test]
fn the_rendering_and_the_native_form_sort_to_the_same_order() {
    let ids: Vec<CoordId> = (0..64).map(sampled).collect();

    let mut by_native: Vec<usize> = (0..ids.len()).collect();
    by_native.sort_by_key(|&at| ids[at].to_string());

    let mut by_rendering: Vec<usize> = (0..ids.len()).collect();
    by_rendering.sort_by_key(|&at| ascii::encode(&ids[at]));

    assert_eq!(by_native, by_rendering);
}

#[test]
fn encoding_a_decoded_text_reproduces_the_text() {
    for seed in 0..32 {
        let text = ascii::encode(&sampled(seed));
        let id = ascii::decode::<20>(&text).expect("a readable text");
        assert_eq!(ascii::encode(&id), text);
    }
}

#[test]
fn a_character_outside_the_alphabet_is_refused_in_every_window() {
    let text = ascii::encode(&id_with(1, 1));
    for at in [0, 3, 30, 59] {
        for bad in ["-", " ", "_", "A", "Z", "+"] {
            let mut shaken = text.clone();
            shaken.replace_range(at..at + 1, bad);
            assert_eq!(ascii::decode::<20>(&shaken), None, "{bad:?} at {at}");
        }
    }
}

#[test]
fn an_uppercase_copy_of_the_whole_text_is_refused() {
    let text = ascii::encode(&id_from([11_171u16; 20]));
    let uppered = text.to_uppercase();
    assert_ne!(uppered, text, "the sample holds no letter");
    assert_eq!(uppered.len(), text.len());
    assert_eq!(ascii::decode::<20>(&uppered), None);
}

#[test]
fn a_six_axis_coordinate_round_trips() {
    let id = id_from([1u16, 2, 35, 36, 1_296, 11_171]);
    let text = ascii::encode(&id);
    assert_eq!(text, "00100200z0101008mb");
    assert_eq!(ascii::decode::<6>(&text), Some(id));
}

#[test]
fn a_depth_the_text_does_not_fill_is_refused() {
    let deep = ascii::encode(&id_with(1, 1));
    assert_eq!(ascii::decode::<6>(&deep), None);

    let shallow = ascii::encode(&id_from([1u16, 2, 35, 36, 1_296, 11_171]));
    assert_eq!(ascii::decode::<20>(&shallow), None);
}
