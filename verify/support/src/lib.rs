// Shared support software for the nexus verification topics (issue #181).
//
// Provides two pieces the storage core needs but does not itself own:
//   - FlatIo: a no_std flat key-space FileIo stand-in for the launcher
//     bridge. On the real MCU the launcher presents FAT32 sector IO as
//     this same flat key-space surface; the verification harness uses an
//     in-memory Vec so the storage-core logic is exercised on target
//     without the memory cost of a materialization backend.
//
//     A scenario that holds what a read path returned has not held what it
//     cost, so the stand-in carries the two properties the scenarios assert
//     on: a refusal switch, because a part at the end of its life refuses a
//     write, and counters, because the difference between the walk and the
//     framing walk is the payload reads rather than the records. Its
//     `list_page` resumes from a cursor, so a walk takes the paged path the
//     launcher presents rather than the default that hands over every key at
//     once (issues #203, #209, #213).
//   - block_on: drives the async storage surface with a no-op waker.
//     The storage IO backends are synchronous under the hood, so a poll
//     loop suffices; a real MCU launcher may use embassy instead.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use chton::cell::Cell2;
use chton::io::{FileIo, IoFuture};

/// What a store asked the medium for.
///
/// A scenario holds what a read path returned by looking at the records it handed
/// over. The cost of that path is this instead: the walk and the framing walk reach
/// the same records and differ in the reads, so a count of records cannot tell them
/// apart and a count of reads can.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Traffic {
    /// Every key the store read.
    pub reads: u32,
    /// The enumeration calls, which is where the page bound shows.
    pub pages: u32,
}

impl Traffic {
    /// The traffic between `before` and this reading.
    pub fn since(self, before: Traffic) -> Traffic {
        Traffic {
            reads: self.reads.saturating_sub(before.reads),
            pages: self.pages.saturating_sub(before.pages),
        }
    }
}

/// The entries a flat key-space holds: a key and the bytes at it, in write order.
type Entries = Vec<(Vec<u8>, Vec<u8>)>;

/// Minimal flat key-space IO. O(n) lookups are fine for a verification
/// workload; the point is a lean, no_std FileIo that mirrors the surface
/// the launcher bridge presents on the MCU.
///
/// Cloning shares the entries, so a verification scenario can close a store
/// and open a second one over the same medium. A clone is the same medium, not a
/// copy of it, which is what lets one session write a volume and the next read it.
#[derive(Clone)]
pub struct FlatIo {
    entries: Arc<Cell2<Entries>>,
    /// Set to refuse every write, which is a part at the end of its life.
    refuse: Arc<Cell2<bool>>,
    traffic: Arc<Cell2<Traffic>>,
}

impl Default for FlatIo {
    fn default() -> Self {
        Self {
            entries: Arc::new(Cell2::new(Vec::new())),
            refuse: Arc::new(Cell2::new(false)),
            traffic: Arc::new(Cell2::new(Traffic::default())),
        }
    }
}

impl FlatIo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Refuse every write from here on, which is what a part at the end of its life
    /// does.
    ///
    /// A refused write is not lost with the medium: the storage core keeps it pending
    /// and still answers for it, so a scenario can hold that a worn part does not turn
    /// a record the session holds into one that is gone.
    pub fn refuse_writes(&self, refuse: bool) {
        *self.refuse.borrow_mut() = refuse;
    }

    /// What the medium has been asked for so far.
    pub fn traffic(&self) -> Traffic {
        *self.traffic.borrow()
    }
}

impl FileIo for FlatIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let entries = &self.entries;
        let traffic = &self.traffic;
        Box::pin(async move {
            traffic.borrow_mut().reads += 1;
            let guard = entries.borrow();
            Ok(guard
                .iter()
                .find(|(p, _)| p.as_slice() == path.as_bytes())
                .map(|(_, d)| d.clone()))
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let entries = &self.entries;
        let refuse = &self.refuse;
        Box::pin(async move {
            if *refuse.borrow() {
                return Err(format!("the medium refused the write: {path}"));
            }
            let mut guard = entries.borrow_mut();
            let key = path.as_bytes().to_vec();
            match guard.iter_mut().find(|(p, _)| *p == key) {
                Some((_, d)) => {
                    d.clear();
                    d.extend_from_slice(data);
                }
                None => guard.push((key, data.to_vec())),
            }
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let entries = &self.entries;
        Box::pin(async move {
            let guard = entries.borrow();
            Ok(guard
                .iter()
                .filter(|(p, _)| p.starts_with(prefix.as_bytes()))
                .map(|(p, _)| String::from_utf8_lossy(p).into_owned())
                .collect())
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let entries = &self.entries;
        Box::pin(async move {
            let mut guard = entries.borrow_mut();
            let key = path.as_bytes();
            guard.retain(|(p, _)| p.as_slice() != key);
            Ok(())
        })
    }

    /// Keys under the prefix one page at a time, in key order.
    ///
    /// The trait's default hands over every key in one call, which leaves the walk
    /// holding the volume's key list and hides the page the walk exists for. This
    /// resumes from the cursor, so the gate takes the paged path a launcher presents
    /// and the walk's peak is a page rather than the enumeration.
    fn list_page<'a>(
        &'a self,
        prefix: &'a str,
        cursor: Option<&'a [u8]>,
        max: usize,
    ) -> IoFuture<'a, (Vec<String>, Option<Vec<u8>>)> {
        let entries = &self.entries;
        let traffic = &self.traffic;
        Box::pin(async move {
            traffic.borrow_mut().pages += 1;
            let guard = entries.borrow();
            let mut keys: Vec<String> = guard
                .iter()
                .filter(|(p, _)| p.starts_with(prefix.as_bytes()))
                .map(|(p, _)| String::from_utf8_lossy(p).into_owned())
                .collect();
            keys.sort_unstable();

            // The cursor is the last key of the previous page, which is a key order this
            // medium can resume from. The trait promises no order, so any stable one does.
            let start = match cursor {
                Some(token) => {
                    let last = String::from_utf8_lossy(token);
                    keys.partition_point(|key| key.as_str() <= last.as_ref())
                }
                None => 0,
            };

            // A bound of zero is read as no bound: the default ignores it rather than
            // dropping keys it has no way to hand over later, and so does this.
            let take = if max == 0 { keys.len() } else { max };
            let page: Vec<String> = keys[start..].iter().take(take).cloned().collect();
            let next = start + page.len();
            let token = if next < keys.len() {
                page.last().map(|key| key.as_bytes().to_vec())
            } else {
                None
            };
            Ok((page, token))
        })
    }
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(|_| noop_raw_waker(), |_| {}, |_| {}, |_| {});

fn noop_raw_waker() -> RawWaker {
    RawWaker::new(core::ptr::null(), &VTABLE)
}

/// Drive a future to completion with a no-op waker. Suitable only for
/// futures whose IO never yields; the storage backends used by the
/// verification crates are synchronous under the hood.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = Box::pin(fut);
    let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
    let mut cx = Context::from_waker(&waker);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => {}
        }
    }
}
