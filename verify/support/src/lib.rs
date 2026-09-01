// Shared support software for the nexus verification topics (issue #181).
//
// Provides two pieces the storage core needs but does not itself own:
//   - FlatIo: a no_std flat key-space FileIo stand-in for the launcher
//     bridge. On the real MCU the launcher presents FAT32 sector IO as
//     this same flat key-space surface; the verification harness uses an
//     in-memory Vec so the storage-core logic is exercised on target
//     without the memory cost of a materialization backend.
//   - block_on: drives the async storage surface with a no-op waker.
//     The storage IO backends are synchronous under the hood, so a poll
//     loop suffices; a real MCU launcher may use embassy instead.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use chton::cell::Cell2;
use chton::io::{FileIo, IoFuture};

/// Minimal flat key-space IO. O(n) lookups are fine for a verification
/// workload; the point is a lean, no_std FileIo that mirrors the surface
/// the launcher bridge presents on the MCU.
pub struct FlatIo {
    entries: Cell2<Vec<(Vec<u8>, Vec<u8>)>>,
}

impl Default for FlatIo {
    fn default() -> Self {
        Self {
            entries: Cell2::new(Vec::new()),
        }
    }
}

impl FlatIo {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FileIo for FlatIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let entries = &self.entries;
        Box::pin(async move {
            let guard = entries.borrow();
            Ok(guard
                .iter()
                .find(|(p, _)| p.as_slice() == path.as_bytes())
                .map(|(_, d)| d.clone()))
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let entries = &self.entries;
        Box::pin(async move {
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

#[cfg(test)]
mod tests {
    use super::*;
    use critical_section::RawRestoreState;

    // FlatIo uses Cell2, which needs a critical-section implementation to
    // link in the test binary (the library build has none; an MCU
    // firmware provides one from the HAL). A no-op is correct for the
    // single-threaded test harness.
    struct TestCs;
    critical_section::set_impl!(TestCs);

    unsafe impl critical_section::Impl for TestCs {
        unsafe fn acquire() -> RawRestoreState {
            false
        }
        unsafe fn release(_restore_state: RawRestoreState) {}
    }

    #[test]
    fn write_read_round_trip() {
        let io = FlatIo::new();
        block_on(io.write("facts/a", b"hello")).unwrap();
        let got = block_on(io.read("facts/a")).unwrap();
        assert_eq!(got.as_deref(), Some(&b"hello"[..]));
        // A path that was never written reads as None.
        assert_eq!(block_on(io.read("facts/missing")).unwrap(), None);
    }

    #[test]
    fn write_overwrites_existing_path() {
        let io = FlatIo::new();
        block_on(io.write("facts/a", b"one")).unwrap();
        block_on(io.write("facts/a", b"two")).unwrap();
        let got = block_on(io.read("facts/a")).unwrap();
        assert_eq!(got.as_deref(), Some(&b"two"[..]));
    }

    #[test]
    fn delete_removes_path_and_is_idempotent() {
        let io = FlatIo::new();
        block_on(io.write("facts/a", b"x")).unwrap();
        block_on(io.delete("facts/a")).unwrap();
        assert_eq!(block_on(io.read("facts/a")).unwrap(), None);
        // Deleting a missing path is a no-op.
        block_on(io.delete("facts/missing")).unwrap();
        block_on(io.delete("facts/a")).unwrap();
    }

    #[test]
    fn list_filters_by_prefix() {
        let io = FlatIo::new();
        block_on(io.write("facts/a", b"1")).unwrap();
        block_on(io.write("facts/b", b"2")).unwrap();
        block_on(io.write("hints/c", b"3")).unwrap();
        let facts = block_on(io.list("facts/")).unwrap();
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&String::from("facts/a")));
        assert!(facts.contains(&String::from("facts/b")));
        // The empty prefix lists everything.
        let all = block_on(io.list("")).unwrap();
        assert_eq!(all.len(), 3);
    }
}
