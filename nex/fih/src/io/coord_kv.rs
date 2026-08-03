// ── CoordKvIo: FileIo surface over the materialized CoordKV ──────────
//
// Bridges the flat key-space IO surface (FileIo) to chton's materialized
// CoordKV (MaterialKv). Each path maps to a deterministic N-byte key
// (SHA-256 prefix, so any path length fits); the value carries the path
// so prefix listing can recover it. This lets FihStorage run over the
// materialized CoordKV without changing FIH code.
//
// The record boundary here is the seam for a future codec layer: the
// value encoding is local to this adapter.

use std::sync::Mutex;

use crate::io::file_io::{BufferIo, FileIo, IoFuture};
use sha2::Digest;
use tagma_kv::coord_gen::CoordKey;

use chton::kv::MaterialKv;

/// Encode a path and content into a value: `[u32le path_len][path][content]`.
fn encode_value(path: &str, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + path.len() + content.len());
    out.extend_from_slice(&(path.len() as u32).to_le_bytes());
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(content);
    out
}

/// Decode a value back into `(path, content)`.
fn decode_value(value: &[u8]) -> Result<(String, Vec<u8>), String> {
    if value.len() < 4 {
        return Err("coord-kv value too short".into());
    }
    let path_len = u32::from_le_bytes(value[..4].try_into().unwrap()) as usize;
    let path_bytes = value
        .get(4..4 + path_len)
        .ok_or_else(|| "coord-kv value path truncated".to_string())?;
    let path = std::str::from_utf8(path_bytes)
        .map_err(|e| format!("coord-kv value path not utf-8: {e}"))?
        .to_string();
    Ok((path, value[4 + path_len..].to_vec()))
}

/// The key for a path: the first N bytes of SHA-256. Deterministic, and
/// collisions are negligible for N >= 16.
fn key_of<const N: usize>(path: &str) -> CoordKey<N> {
    let digest = sha2::Sha256::digest(path.as_bytes());
    let mut bytes = [0u8; N];
    bytes.copy_from_slice(&digest[..N]);
    CoordKey::new(bytes)
}

/// A FileIo backend over a materialized CoordKV.
///
/// The kv is interior-mutable because `FileIo` methods take `&self` while
/// `MaterialKv` writes need `&mut self`.
pub struct CoordKvIo<const N: usize> {
    kv: Mutex<MaterialKv<N>>,
}

impl<const N: usize> CoordKvIo<N> {
    pub fn new(kv: MaterialKv<N>) -> Self {
        Self { kv: Mutex::new(kv) }
    }
}

impl<const N: usize> BufferIo for CoordKvIo<N> {
    fn is_buffered(&self) -> bool {
        self.kv.lock().unwrap().is_buffered()
    }

    fn flush<'a>(&'a self) -> IoFuture<'a, ()> {
        let kv = &self.kv;
        Box::pin(async move { kv.lock().unwrap().flush().map_err(|e| e.to_string()) })
    }
}

impl<const N: usize> FileIo for CoordKvIo<N> {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let kv = &self.kv;
        Box::pin(async move {
            let guard = kv.lock().unwrap();
            let key = key_of::<N>(path);
            match guard.get_path(&key.to_coord_path()) {
                Ok(Some(value)) => Ok(Some(decode_value(&value)?.1)),
                Ok(None) => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let kv = &self.kv;
        Box::pin(async move {
            let mut guard = kv.lock().unwrap();
            let key = key_of::<N>(path);
            guard
                .put_path(&key.to_coord_path(), &encode_value(path, data))
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let kv = &self.kv;
        Box::pin(async move {
            let guard = kv.lock().unwrap();
            let entries = guard.iter().map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for (_, value) in entries {
                if let Ok((path, _)) = decode_value(&value) {
                    if path.starts_with(prefix) {
                        out.push(path);
                    }
                }
            }
            Ok(out)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let kv = &self.kv;
        Box::pin(async move {
            let mut guard = kv.lock().unwrap();
            let key = key_of::<N>(path);
            guard
                .remove_path(&key.to_coord_path())
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }
}
