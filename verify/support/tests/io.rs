// The FileIo contract this stand-in claims.
//
// The verification gates assert against this harness, so a change that breaks it has to fail
// here as a contract break rather than later as a confusing scenario failure. The README
// fixes the canonical semantics the storage core relies on: listing a missing prefix is an
// empty enumeration, listed keys include their prefix so they round-trip through read, and
// deleting a missing key is a no-op. The paged enumeration adds the cursor rules the walk
// depends on, and the refusal switch is the medium a worn part presents.

use chton::io::FileIo;
use critical_section::RawRestoreState;
use nexus_verify_support::{FlatIo, Traffic, block_on};

// FlatIo uses Cell2, which needs a critical-section implementation to link in the test
// binary (the library build has none; an MCU firmware provides one from the HAL). A no-op is
// correct here because every test owns its medium and runs on one thread.
struct TestCs;
critical_section::set_impl!(TestCs);

unsafe impl critical_section::Impl for TestCs {
    unsafe fn acquire() -> RawRestoreState {
        false
    }
    unsafe fn release(_restore_state: RawRestoreState) {}
}

fn write(io: &FlatIo, path: &str, data: &[u8]) {
    block_on(io.write(path, data)).expect("the medium accepts the write");
}

fn read(io: &FlatIo, path: &str) -> Option<Vec<u8>> {
    block_on(io.read(path)).expect("the medium answers a read")
}

fn list(io: &FlatIo, prefix: &str) -> Vec<String> {
    block_on(io.list(prefix)).expect("the medium lists")
}

fn page(
    io: &FlatIo,
    prefix: &str,
    cursor: Option<&[u8]>,
    max: usize,
) -> (Vec<String>, Option<Vec<u8>>) {
    block_on(io.list_page(prefix, cursor, max)).expect("the medium lists a page")
}

/// Every key the paged enumeration yields, in yield order, and the number of calls it took.
fn enumerate(io: &FlatIo, prefix: &str, max: usize) -> (Vec<String>, usize) {
    let mut keys = Vec::new();
    let mut cursor: Option<Vec<u8>> = None;
    let mut pages = 0;
    loop {
        let (one_page, next) = page(io, prefix, cursor.as_deref(), max);
        pages += 1;
        keys.extend(one_page);
        match next {
            Some(token) => cursor = Some(token),
            None => return (keys, pages),
        }
    }
}

#[test]
fn write_and_read_round_trip() {
    let io = FlatIo::new();
    write(&io, "facts/a", b"hello");
    assert_eq!(read(&io, "facts/a").as_deref(), Some(&b"hello"[..]));
    // A path that was never written reads as None.
    assert_eq!(read(&io, "facts/missing"), None);
}

#[test]
fn a_write_over_an_existing_key_replaces_its_bytes() {
    let io = FlatIo::new();
    write(&io, "facts/a", b"one");
    write(&io, "facts/a", b"two");
    assert_eq!(read(&io, "facts/a").as_deref(), Some(&b"two"[..]));
    // Replacing a key does not add a second entry.
    assert_eq!(list(&io, "facts/").len(), 1);
}

#[test]
fn delete_removes_a_key_and_a_missing_key_is_a_no_op() {
    let io = FlatIo::new();
    write(&io, "facts/a", b"x");
    block_on(io.delete("facts/a")).expect("delete");
    assert_eq!(read(&io, "facts/a"), None);
    // Deleting a missing path is a no-op, twice over.
    block_on(io.delete("facts/missing")).expect("delete missing");
    block_on(io.delete("facts/a")).expect("delete removed");
}

#[test]
fn listing_filters_by_prefix_and_an_empty_prefix_lists_everything() {
    let io = FlatIo::new();
    write(&io, "facts/a", b"1");
    write(&io, "facts/b", b"2");
    write(&io, "hints/c", b"3");

    let facts = list(&io, "facts/");
    assert_eq!(facts.len(), 2);
    assert!(facts.contains(&String::from("facts/a")));
    assert!(facts.contains(&String::from("facts/b")));

    assert_eq!(list(&io, "").len(), 3);
}

#[test]
fn a_missing_prefix_lists_as_an_empty_enumeration() {
    let io = FlatIo::new();
    write(&io, "facts/a", b"1");
    assert!(list(&io, "absent/").is_empty());
}

#[test]
fn every_listed_key_round_trips_through_read() {
    let io = FlatIo::new();
    write(&io, "facts/a", b"1");
    write(&io, "facts/b", b"2");
    write(&io, "hints/c", b"3");
    for key in list(&io, "") {
        assert!(
            read(&io, &key).is_some(),
            "a listed key did not read: {key}"
        );
    }
}

#[test]
fn refusing_writes_fails_them_and_leaves_the_medium_empty() {
    let io = FlatIo::new();
    io.refuse_writes(true);
    let outcome = block_on(io.write("facts/a", b"x"));
    assert!(outcome.is_err(), "a refused write returned {outcome:?}");
    assert!(list(&io, "").is_empty(), "the medium kept a refused write");
}

#[test]
fn clearing_the_refusal_accepts_writes_again() {
    let io = FlatIo::new();
    io.refuse_writes(true);
    assert!(block_on(io.write("facts/a", b"x")).is_err());
    io.refuse_writes(false);
    write(&io, "facts/a", b"x");
    assert_eq!(read(&io, "facts/a").as_deref(), Some(&b"x"[..]));
}

#[test]
fn a_page_is_bounded_by_max_and_reports_more_to_come() {
    let io = FlatIo::new();
    for index in 0..5 {
        write(&io, &format!("facts/{index}"), b"x");
    }
    let (one_page, next) = page(&io, "facts/", None, 2);
    assert_eq!(one_page.len(), 2, "the channel honoured the bound");
    assert!(next.is_some(), "a bounded page reports a continuation");
}

#[test]
fn a_paged_enumeration_yields_every_key_exactly_once() {
    let io = FlatIo::new();
    for index in 0..40 {
        write(&io, &format!("facts/{index:02}"), b"x");
    }

    let (paged, pages) = enumerate(&io, "facts/", 16);
    assert_eq!(pages, 3, "forty keys at sixteen a page take three pages");
    assert_eq!(paged.len(), 40, "the pages cover the volume: {paged:?}");

    let mut seen = paged;
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 40, "a key was yielded twice");

    let mut listed = list(&io, "facts/");
    listed.sort_unstable();
    assert_eq!(seen, listed, "the pages yield exactly the listed keys");
}

#[test]
fn a_bound_of_zero_returns_every_key_in_one_page() {
    let io = FlatIo::new();
    for index in 0..5 {
        write(&io, &format!("facts/{index}"), b"x");
    }
    // The trait's default ignores the bound rather than dropping keys it could not hand
    // over later, and this channel reads zero the same way.
    let (one_page, next) = page(&io, "facts/", None, 0);
    assert_eq!(one_page.len(), 5);
    assert!(next.is_none());
}

#[test]
fn a_cursor_at_the_last_key_ends_the_enumeration() {
    let io = FlatIo::new();
    for index in 0..3 {
        write(&io, &format!("facts/{index}"), b"x");
    }
    let mut keys = list(&io, "facts/");
    keys.sort_unstable();
    let last = keys.last().expect("the medium holds keys");

    let (one_page, next) = page(&io, "facts/", Some(last.as_bytes()), 16);
    assert!(
        one_page.is_empty(),
        "a cursor at the end yielded {one_page:?}"
    );
    assert!(next.is_none());
}

#[test]
fn a_missing_prefix_ends_after_an_empty_page() {
    let io = FlatIo::new();
    let (one_page, next) = page(&io, "absent/", None, 16);
    assert!(one_page.is_empty());
    assert!(next.is_none());
}

#[test]
fn traffic_counts_reads_and_pages() {
    let io = FlatIo::new();
    write(&io, "facts/a", b"1");

    let _ = read(&io, "facts/a");
    let _ = read(&io, "facts/missing");
    let _ = page(&io, "facts/", None, 16);

    let traffic = io.traffic();
    assert_eq!(traffic.reads, 2, "a hit and a miss are both reads");
    assert_eq!(traffic.pages, 1);

    // A full listing is not the paged enumeration, so it does not count as a page.
    assert_eq!(list(&io, "").len(), 1);
    assert_eq!(io.traffic().pages, 1);
}

#[test]
fn traffic_since_reports_what_happened_between_two_readings() {
    let io = FlatIo::new();
    write(&io, "facts/a", b"1");

    let before = io.traffic();
    let _ = read(&io, "facts/a");
    let _ = page(&io, "facts/", None, 16);
    assert_eq!(io.traffic().since(before), Traffic { reads: 1, pages: 1 });

    // A reading taken before more traffic than the current one reports nothing rather than
    // wrapping a counter.
    let ahead = Traffic {
        reads: 10,
        pages: 2,
    };
    assert_eq!(Traffic::default().since(ahead), Traffic::default());
}

/// A future that returns `Pending` once, so the driver's poll loop is exercised.
struct PendingOnce(bool);

impl core::future::Future for PendingOnce {
    type Output = u8;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<u8> {
        if self.0 {
            core::task::Poll::Ready(7)
        } else {
            self.0 = true;
            core::task::Poll::Pending
        }
    }
}

#[test]
fn block_on_polls_again_after_pending() {
    assert_eq!(block_on(PendingOnce(false)), 7);
}
