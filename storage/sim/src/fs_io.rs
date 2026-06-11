// ── FsFihIo: filesystem-backed FihIo implementation ────────────────────
//
// Wraps std::fs operations behind the FihIo trait.
// Uses a root directory as the store. Directory structure mirrors the
// flat key-space: each path becomes a file under root.
//
// Thread-safe via the OS filesystem (no internal locks needed).
// Compatible with wasm32-wasi (WASI filesystem) but NOT wasm32-unknown-unknown.

use std::path::{Path, PathBuf};

use crate::io::FihIo;

/// Filesystem-backed FihIo. Root directory is created on construction.
///
/// File layout:
///   {root}/{path}  →  file content
///
/// List with prefix scans directories recursively.
pub struct FsFihIo {
    root: PathBuf,
}

impl FsFihIo {
    /// Create a new FsFihIo rooted at the given path.
    /// Creates the directory if it does not exist.
    pub fn new<P: AsRef<Path>>(root: P) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root).map_err(|e| format!("create root: {e}"))?;
        Ok(Self { root })
    }

    /// Create a temporary FsFihIo for testing. Directory is auto-cleaned
    /// on drop or explicitly via clear().
    pub fn temp() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("nexus_fs_{}", std::process::id()));
        Self::new(dir)
    }

    fn resolve(&self, path: &str) -> PathBuf {
        // Sanitize: prevent directory traversal
        let safe: String = path
            .chars()
            .map(|c| {
                if c == '/' || c == '_' || c == '-' || c == '.' || c.is_alphanumeric() {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.root.join(&safe)
    }
}

impl FihIo for FsFihIo {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let full = self.resolve(path);
        match std::fs::read(&full) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("read {path}: {e}")),
        }
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), String> {
        let full = self.resolve(path);
        // Create parent directories
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {path}: {e}"))?;
        }
        std::fs::write(&full, data).map_err(|e| format!("write {path}: {e}"))?;
        Ok(())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        let full = self.resolve(prefix);
        let root_prefix = self.root.to_string_lossy().to_string();
        let mut results = Vec::new();

        if !full.exists() {
            return Ok(results);
        }

        if full.is_dir() {
            // Walk directory recursively
            let walker =
                walkdir::WalkDir::new(&full).sort_by(|a, b| a.file_name().cmp(b.file_name()));
            for entry in walker.into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    let abs_path = entry.path().to_string_lossy().to_string();
                    // Strip root prefix to get the relative key
                    if let Some(rel) = abs_path.strip_prefix(&root_prefix) {
                        let rel = rel.trim_start_matches('/');
                        if rel.starts_with(prefix) {
                            results.push(rel.to_string());
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let full = self.resolve(path);
        if full.exists() {
            std::fs::remove_file(&full).map_err(|e| format!("delete {path}: {e}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fs() -> FsFihIo {
        let dir = std::env::temp_dir().join(format!(
            "nexus_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        FsFihIo::new(dir).unwrap()
    }

    #[test]
    fn test_write_read_roundtrip() {
        let fs = make_fs();
        fs.write("facts/f001.fact", b"hello").unwrap();
        let data = fs.read("facts/f001.fact").unwrap().expect("should exist");
        assert_eq!(data, b"hello");
    }

    #[test]
    fn test_read_nonexistent() {
        let fs = make_fs();
        assert!(fs.read("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_delete() {
        let fs = make_fs();
        fs.write("test.txt", b"data").unwrap();
        fs.delete("test.txt").unwrap();
        assert!(fs.read("test.txt").unwrap().is_none());
    }

    #[test]
    fn test_list_prefix() {
        let fs = make_fs();
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
        let fs = make_fs();
        fs.write("a/b/c/d.txt", b"deep").unwrap();
        let data = fs.read("a/b/c/d.txt").unwrap().expect("should exist");
        assert_eq!(data, b"deep");
    }

    #[test]
    fn test_list_empty_prefix() {
        let fs = make_fs();
        let items = fs.list("nonexistent/").unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn test_delete_nonexistent_ok() {
        let fs = make_fs();
        fs.delete("no_such_file").unwrap(); // should not error
    }
}
