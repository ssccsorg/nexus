// Filesystem-backed IO tests. Only available on non-wasm32 targets.
// Tests: write/read roundtrip, read nonexistent, delete, list prefix,
// deep path creates dirs, list empty prefix, delete nonexistent, invalid path.

#![cfg(not(target_arch = "wasm32"))]

use nexus_storage_sim::SyncFileIo;
use nexus_storage_sim::fs_io::FsIo;

fn make_fs_blocking() -> (SyncFileIo<FsIo>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let fs = FsIo::new(dir.path()).unwrap();
    (SyncFileIo::new(fs), dir)
}

#[test]
fn test_write_read_roundtrip() {
    let (fs, _dir) = make_fs_blocking();
    fs.write("facts/f001.fact", b"hello").unwrap();
    let data = fs.read("facts/f001.fact").unwrap().expect("should exist");
    assert_eq!(data, b"hello");
}

#[test]
fn test_read_nonexistent() {
    let (fs, _dir) = make_fs_blocking();
    assert!(fs.read("nonexistent").unwrap().is_none());
}

#[test]
fn test_delete() {
    let (fs, _dir) = make_fs_blocking();
    fs.write("test.txt", b"data").unwrap();
    fs.delete("test.txt").unwrap();
    assert!(fs.read("test.txt").unwrap().is_none());
}

#[test]
fn test_list_prefix() {
    let (fs, _dir) = make_fs_blocking();
    fs.write("facts/f_a.fact", b"a").unwrap();
    fs.write("facts/f_b.fact", b"b").unwrap();
    fs.write("blob/hash.bin", b"c").unwrap();
    let facts = fs.list("facts/").unwrap();
    assert_eq!(facts.len(), 2);
    assert!(facts.contains(&"facts/f_a.fact".to_string()));
    assert!(facts.contains(&"facts/f_b.fact".to_string()));
}

#[test]
fn test_deep_path_creates_dirs() {
    let (fs, _dir) = make_fs_blocking();
    fs.write("a/b/c/d.txt", b"deep").unwrap();
    let data = fs.read("a/b/c/d.txt").unwrap().expect("should exist");
    assert_eq!(data, b"deep");
}

#[test]
fn test_list_empty_prefix() {
    let (fs, _dir) = make_fs_blocking();
    let items = fs.list("nonexistent/").unwrap();
    assert!(items.is_empty());
}

#[test]
fn test_delete_nonexistent_ok() {
    let (fs, _dir) = make_fs_blocking();
    fs.delete("no_such_file").unwrap();
}

#[test]
fn test_invalid_path_rejected() {
    let (fs, _dir) = make_fs_blocking();
    let result = fs.write("facts/foo@bar.fact", b"data");
    assert!(result.is_err(), "path with @ must be rejected");
    assert!(
        result.unwrap_err().contains("invalid character"),
        "error must mention invalid character"
    );
}
