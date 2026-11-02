// The optional ASCII rendering of a coordinate: round trip, the two properties
// it buys, and what it refuses.

use fih_model::CoordId;
use fih_model::ascii;

fn coordinate(index: u16) -> char {
    char::from_u32(0xAC00 + index as u32).expect("a coordinate character")
}

fn id_with(first: u16, second: u16) -> CoordId {
    let mut axes = [0u16; 20];
    axes[0] = first;
    axes[1] = second;
    let text: String = axes.iter().map(|i| coordinate(*i)).collect();
    CoordId::from_string(&text).expect("a canonical id")
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
fn a_lowercase_character_is_refused() {
    let mut text = ascii::encode(&id_with(10, 0));
    text.replace_range(1..2, "a");
    assert_eq!(ascii::decode::<20>(&text), None);
}

#[test]
fn a_wrong_length_is_refused() {
    let text = ascii::encode(&id_with(1, 1));
    assert_eq!(ascii::decode::<20>(&text[..text.len() - 1]), None);
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
