// The ASCII rendering's cost, counted rather than timed.
//
// `encode` returns a string, so one allocation is its contract and no more: a
// second would mean the buffer was grown or an intermediate was built. `decode`
// returns a coordinate and allocates nothing, because a caller that reads a key
// per record cannot afford one, and the earlier reader built a Vec of chars.
//
// The counter is process-wide, so this file holds one test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use fih_model::CoordId;
use fih_model::ascii;

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn id_with(axes: [u16; 20]) -> CoordId {
    let text: String = axes
        .iter()
        .map(|value| char::from_u32(0xAC00 + u32::from(*value)).expect("a coordinate"))
        .collect();
    CoordId::from_string(&text).expect("a canonical id")
}

#[test]
fn the_rendering_costs_one_small_string_and_its_reader_costs_nothing() {
    let mut axes = [0u16; 20];
    for (axis, value) in axes.iter_mut().enumerate() {
        *value = (axis as u16 * 613) % 11_172;
    }
    let id = id_with(axes);

    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let text = std::hint::black_box(ascii::encode(&id));
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(
        after - before,
        1,
        "encode should allocate its string and nothing else"
    );
    assert_eq!(text.len(), 20 * ascii::DIGITS_PER_AXIS);

    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let read = std::hint::black_box(ascii::decode::<20>(&text));
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(after, before, "decode allocated; a key is read per record");
    assert_eq!(read, Some(id), "the rendering did not round trip");

    // The counter is the instrument, so it is checked with an allocation that is
    // meant to be counted. Without this a build where the hook stopped being
    // applied would pass every measurement above.
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let probe = std::hint::black_box(String::from("probe"));
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert!(after > before, "the counting allocator is not measuring");
    drop(probe);
}
