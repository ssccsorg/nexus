// SimIo in-memory IO tests.
// Tests: write/read roundtrip, read nonexistent, delete, list prefix,
// failure injection, clear.

use nexus_storage_sim::{SimIo, SyncFileIo};

#[test]
fn test_write_read_roundtrip() {
    let io = SimIo::new();
    let blocking = SyncFileIo::new(io);
    blocking.write("facts/f_test.fact", b"hello").unwrap();
    let data = blocking
        .read("facts/f_test.fact")
        .unwrap()
        .expect("should exist");
    assert_eq!(data, b"hello");
}

#[test]
fn test_read_nonexistent() {
    let io = SimIo::new();
    let blocking = SyncFileIo::new(io);
    assert!(blocking.read("nonexistent").unwrap().is_none());
}

#[test]
fn test_delete() {
    let io = SimIo::new();
    let blocking = SyncFileIo::new(io);
    blocking.write("facts/f_test.fact", b"data").unwrap();
    blocking.delete("facts/f_test.fact").unwrap();
    assert!(blocking.read("facts/f_test.fact").unwrap().is_none());
}

#[test]
fn test_list_prefix() {
    let io = SimIo::new();
    let blocking = SyncFileIo::new(io);
    blocking.write("facts/f_a.fact", b"a").unwrap();
    blocking.write("facts/f_b.fact", b"b").unwrap();
    blocking.write("blob/hash.bin", b"c").unwrap();
    let facts = blocking.list("facts/").unwrap();
    assert_eq!(facts.len(), 2);
    assert!(facts.contains(&"facts/f_a.fact".to_string()));
    assert!(facts.contains(&"facts/f_b.fact".to_string()));
}

#[test]
fn test_failure_injection() {
    let io = SimIo::new().with_failure_rate(1.0); // 100% fail
    let blocking = SyncFileIo::new(io);
    assert!(blocking.write("x", b"data").is_err());
}

#[test]
fn test_clear() {
    let io = SimIo::new();
    let blocking = SyncFileIo::new(io.clone());
    blocking.write("test", b"data").unwrap();
    assert_eq!(io.len(), 1);
    io.clear();
    assert_eq!(io.len(), 0);
}
