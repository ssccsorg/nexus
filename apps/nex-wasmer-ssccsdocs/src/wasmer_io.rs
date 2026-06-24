// WasmerIo: filesystem-backed AsyncFileIo for WASIX targets.
//
// WASIX (wasm32-wasix) provides full std::fs access.
// This is equivalent to nex::io::FsIo but does not depend on
// the target_arch = "wasm32" gate in the nex crate.

use std::path::{Path, PathBuf};

use nex::io::{AsyncFileIo, IoFuture};

/// Filesystem-backed IO for WASIX. Root directory is created on construction.
pub struct WasmerIo {
    root: PathBuf,
}

impl WasmerIo {
    pub fn new<P: AsRef<Path>>(root: P) -> Result<Self, String> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root).map_err(|e| format!("create root: {e}"))?;
        Ok(Self { root })
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        for c in path.chars() {
            if c != '/' && c != '_' && c != '-' && c != '.' && !c.is_alphanumeric() {
                return Err(format!(
                    "invalid character '{}' in path '{}': only alphanumeric, /, _, -, . allowed",
                    c, path
                ));
            }
        }
        Ok(self.root.join(path))
    }
}

impl AsyncFileIo for WasmerIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            let full = self.resolve(path)?;
            match std::fs::read(&full) {
                Ok(data) => Ok(Some(data)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("read {path}: {e}")),
            }
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        Box::pin(async move {
            let full = self.resolve(path)?;
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {path}: {e}"))?;
            }
            std::fs::write(&full, data).map_err(|e| format!("write {path}: {e}"))?;
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        Box::pin(async move {
            let full = self.resolve(prefix)?;
            let root_prefix = self.root.to_string_lossy().to_string();
            let mut results = Vec::new();

            if !full.exists() {
                return Ok(results);
            }

            if full.is_dir() {
                let walker =
                    walkdir::WalkDir::new(&full).sort_by(|a, b| a.file_name().cmp(b.file_name()));
                for entry in walker.into_iter().filter_map(|e| e.ok()) {
                    if entry.file_type().is_file() {
                        let abs_path = entry.path().to_string_lossy().to_string();
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
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        Box::pin(async move {
            let full = self.resolve(path)?;
            if full.exists() {
                std::fs::remove_file(&full).map_err(|e| format!("delete {path}: {e}"))?;
            }
            Ok(())
        })
    }
}
