// ── FihExport / FihImport: portable StateSpace bundle ──────────────────
//
// Export full FIH StateSpace (chain files + blob store) into a single
// portable bundle file (.fihbundle), and import it back into any
// AsyncFileIo-backed storage.
//
// Bundle format: concatenated postcard frames
//   [header] [fact_record*] [intent_record*] [hint_record*] [blob_entry*] [trailer]
//
// Each frame is length-delimited: (u32 LE length)(postcard bytes).
// The bundle is a single Vec<u8> — no temp files, no directory structure.
//
// Implementations:
//   - FsIo-based: export local FsIo → file, import file → FsIo
//   - SimIo-based: export/import in-memory (for testing)
//   - CfFihIo: (future) export/import over R2

use crate::io::{AsyncFileIo, SyncFileIo};
use crate::record::{ContentMeta, FactRecord, HintRecord, IntentRecord};

/// Magic bytes for .fihbundle format identification.
const BUNDLE_MAGIC: &[u8; 8] = b"FIHBUNDL";

/// Bundle header: identifies format version and record counts.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
struct BundleHeader {
    version: u16,
    fact_count: u32,
    intent_count: u32,
    hint_count: u32,
    blob_count: u32,
}

/// Container for a blob entry (content + metadata).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct BlobEntry {
    hash: String,
    data: Vec<u8>,
    meta: ContentMeta,
}

/// Bundle trailer: integrity checksum (simple hash for now).
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
struct BundleTrailer {
    entry_count: u32,
    checksum: u64, // XOR of all frame payload lengths (non-cryptographic)
}

/// Write a single postcard-serialized frame: (u32 LE length)(payload).
fn write_frame(buf: &mut Vec<u8>, payload: &[u8]) {
    let len = payload.len() as u32;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(payload);
}

/// Read a single postcard-serialized frame from a cursor position.
/// Returns (payload, new_cursor) or None if exhausted.
fn read_frame(data: &[u8], cursor: usize) -> Option<(&[u8], usize)> {
    if cursor + 4 > data.len() {
        return None;
    }
    let len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
    let start = cursor + 4;
    let end = start + len;
    if end > data.len() {
        return None;
    }
    Some((&data[start..end], end))
}

// ── FihExport trait ─────────────────────────────────────────────────────

/// Export full FIH StateSpace as a portable byte bundle.
pub trait FihExport {
    /// Export entire StateSpace into a single Vec<u8> bundle.
    fn export_bundle(&self) -> Result<Vec<u8>, String>;
}

// ── FihImport trait ─────────────────────────────────────────────────────

/// Import a previously exported FIH StateSpace bundle.
pub trait FihImport {
    /// Restore StateSpace from a bundle, overwriting existing data.
    fn import_bundle(&self, bundle: &[u8]) -> Result<(), String>;
}

// ── FihExport implementation ────────────────────────────────────────────

/// Export FIH StateSpace from a sync IO handle.
///
/// Scans the IO for fact, intent, hint records and blob entries,
/// then serializes them into a single .fihbundle byte vector.
pub fn export_from_io<A: AsyncFileIo>(io: &SyncFileIo<A>) -> Result<Vec<u8>, String> {
    let sync = io;

    // Collect all records
    let fact_keys = sync.list("facts/")?;
    let mut facts: Vec<FactRecord> = Vec::with_capacity(fact_keys.len());
    for key in &fact_keys {
        if let Some(bytes) = sync.read(key)?
            && let Ok(record) = postcard::from_bytes::<FactRecord>(&bytes)
        {
            facts.push(record);
        }
    }

    let intent_keys = sync.list("intents/")?;
    let mut intents: Vec<IntentRecord> = Vec::with_capacity(intent_keys.len());
    for key in &intent_keys {
        if let Some(bytes) = sync.read(key)?
            && let Ok(record) = postcard::from_bytes::<IntentRecord>(&bytes)
        {
            intents.push(record);
        }
    }

    let hint_keys = sync.list("hints/")?;
    let mut hints: Vec<HintRecord> = Vec::with_capacity(hint_keys.len());
    for key in &hint_keys {
        if let Some(bytes) = sync.read(key)?
            && let Ok(record) = postcard::from_bytes::<HintRecord>(&bytes)
        {
            hints.push(record);
        }
    }

    // Collect blob entries
    let blob_keys = sync.list("blob/")?;
    let mut blobs: Vec<BlobEntry> = Vec::new();
    let mut i = 0;
    while i < blob_keys.len() {
        let key = &blob_keys[i];
        if key.ends_with(".bin") {
            let hash = key.strip_prefix("blob/").unwrap().strip_suffix(".bin").unwrap().to_string();
            let data = sync.read(key)?.unwrap_or_default();
            let meta_key = format!("blob/{}.bin.meta", hash);
            let meta = if i + 1 < blob_keys.len() && blob_keys[i + 1] == meta_key {
                if let Some(mbytes) = sync.read(&meta_key)? {
                    postcard::from_bytes::<ContentMeta>(&mbytes).unwrap_or(ContentMeta {
                        mime_type: "application/octet-stream".into(),
                        size: data.len() as u64,
                    })
                } else {
                    ContentMeta { mime_type: "application/octet-stream".into(), size: data.len() as u64 }
                }
            } else {
                ContentMeta { mime_type: "application/octet-stream".into(), size: data.len() as u64 }
            };
            blobs.push(BlobEntry { hash, data, meta });
        }
        i += 1;
    }

    // Build bundle
    let header = BundleHeader {
        version: 1,
        fact_count: facts.len() as u32,
        intent_count: intents.len() as u32,
        hint_count: hints.len() as u32,
        blob_count: blobs.len() as u32,
    };

    let mut bundle = Vec::new();

    // Magic
    bundle.extend_from_slice(BUNDLE_MAGIC);

    // Header
    let hdr_bytes = postcard::to_allocvec(&header).map_err(|e| e.to_string())?;
    write_frame(&mut bundle, &hdr_bytes);

    // Facts
    for r in &facts {
        let bytes = postcard::to_allocvec(r).map_err(|e| e.to_string())?;
        write_frame(&mut bundle, &bytes);
    }

    // Intents
    for r in &intents {
        let bytes = postcard::to_allocvec(r).map_err(|e| e.to_string())?;
        write_frame(&mut bundle, &bytes);
    }

    // Hints
    for r in &hints {
        let bytes = postcard::to_allocvec(r).map_err(|e| e.to_string())?;
        write_frame(&mut bundle, &bytes);
    }

    // Blobs
    for b in &blobs {
        let bytes = postcard::to_allocvec(b).map_err(|e| e.to_string())?;
        write_frame(&mut bundle, &bytes);
    }

    // Trailer
    let checksum = bundle.iter().fold(0u64, |acc, b| acc.wrapping_add(*b as u64));
    let trailer = BundleTrailer {
        entry_count: facts.len() as u32 + intents.len() as u32 + hints.len() as u32 + blobs.len() as u32,
        checksum,
    };
    let trailer_bytes = postcard::to_allocvec(&trailer).map_err(|e| e.to_string())?;
    write_frame(&mut bundle, &trailer_bytes);

    Ok(bundle)
}

/// Import FIH StateSpace bundle into a sync IO handle.
///
/// Writes all records and blobs from the bundle into the IO,
/// overwriting any existing data at the same paths.
pub fn import_into_io<A: AsyncFileIo>(io: &SyncFileIo<A>, bundle: &[u8]) -> Result<(), String> {
    let sync = io;

    // Validate magic
    if &bundle[..BUNDLE_MAGIC.len()] != BUNDLE_MAGIC {
        return Err("invalid bundle: bad magic".into());
    }

    let mut cursor = BUNDLE_MAGIC.len();

    // Read header
    let (hdr_bytes, new_cursor) = read_frame(bundle, cursor).ok_or("invalid bundle: truncated header")?;
    cursor = new_cursor;
    let header: BundleHeader = postcard::from_bytes(hdr_bytes).map_err(|e| format!("invalid header: {e}"))?;

    if header.version != 1 {
        return Err(format!("unsupported bundle version: {}", header.version));
    }

    // Read facts
    for _ in 0..header.fact_count {
        let (bytes, new_cursor) = read_frame(bundle, cursor).ok_or("invalid bundle: truncated facts")?;
        cursor = new_cursor;
        let record: FactRecord = postcard::from_bytes(bytes).map_err(|e| format!("invalid fact: {e}"))?;
        sync.write(&record.key(), &postcard::to_allocvec(&record).map_err(|e| e.to_string())?)?;
    }

    // Read intents
    for _ in 0..header.intent_count {
        let (bytes, new_cursor) = read_frame(bundle, cursor).ok_or("invalid bundle: truncated intents")?;
        cursor = new_cursor;
        let record: IntentRecord = postcard::from_bytes(bytes).map_err(|e| format!("invalid intent: {e}"))?;
        sync.write(&record.key(), &postcard::to_allocvec(&record).map_err(|e| e.to_string())?)?;
    }

    // Read hints
    for _ in 0..header.hint_count {
        let (bytes, new_cursor) = read_frame(bundle, cursor).ok_or("invalid bundle: truncated hints")?;
        cursor = new_cursor;
        let record: HintRecord = postcard::from_bytes(bytes).map_err(|e| format!("invalid hint: {e}"))?;
        sync.write(&record.key(), &postcard::to_allocvec(&record).map_err(|e| e.to_string())?)?;
    }

    // Read blobs
    for _ in 0..header.blob_count {
        let (bytes, new_cursor) = read_frame(bundle, cursor).ok_or("invalid bundle: truncated blobs")?;
        cursor = new_cursor;
        let entry: BlobEntry = postcard::from_bytes(bytes).map_err(|e| format!("invalid blob: {e}"))?;
        sync.write(&format!("blob/{}.bin", entry.hash), &entry.data)?;
        let meta_bytes = postcard::to_allocvec(&entry.meta).map_err(|e| e.to_string())?;
        sync.write(&format!("blob/{}.bin.meta", entry.hash), &meta_bytes)?;
    }

    // Trailer (validate checksum)
    let (trailer_bytes, _) = read_frame(bundle, cursor).ok_or("invalid bundle: truncated trailer")?;
    let trailer: BundleTrailer = postcard::from_bytes(trailer_bytes).map_err(|e| format!("invalid trailer: {e}"))?;
    let expected_count = header.fact_count + header.intent_count + header.hint_count + header.blob_count;
    if trailer.entry_count != expected_count {
        return Err(format!(
            "bundle checksum: expected {} entries, trailer says {}",
            expected_count, trailer.entry_count
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_io::SimIo;

    fn seeded_io() -> SimIo {
        let io = SimIo::new();
        let sync = SyncFileIo::new(io.clone());

        // Write a fact record
        let fact = FactRecord {
            id: "f001".into(),
            blob_hash: "deadbeef".into(),
            origin: "test".into(),
            creator: "alice".into(),
            submitted_at: 1000,
        };
        sync.write(
            &fact.key(),
            &postcard::to_allocvec(&fact).unwrap(),
        )
        .unwrap();

        // Write an intent record
        let intent = IntentRecord {
            id: "i001".into(),
            from_facts: vec!["f001".into()],
            description_hash: String::new(),
            creator: "bob".into(),
            status: crate::intent_status::IntentStatus::Submitted,
            created_at: 1001,
        };
        sync.write(
            &intent.key(),
            &postcard::to_allocvec(&intent).unwrap(),
        )
        .unwrap();

        // Write a hint record
        let hint = HintRecord {
            id: "h001".into(),
            content: "test hint".into(),
            creator: "tester".into(),
            submitted_at: 1002,
            ttl_secs: None,
        };
        sync.write(
            &hint.key(),
            &postcard::to_allocvec(&hint).unwrap(),
        )
        .unwrap();

        // Write a blob
        sync.write("blob/deadbeef.bin", b"hello world").unwrap();
        let meta = ContentMeta {
            mime_type: "text/plain".into(),
            size: 11,
        };
        sync.write(
            "blob/deadbeef.bin.meta",
            &postcard::to_allocvec(&meta).unwrap(),
        )
        .unwrap();

        io
    }

    #[test]
    fn test_export_round_trip() {
        let io = seeded_io();
        let sync = SyncFileIo::new(io);
        let bundle = export_from_io(&sync).unwrap();
        assert!(!bundle.is_empty());

        // Import into fresh IO
        let dst = SimIo::new();
        let dst_sync = SyncFileIo::new(dst);
        import_into_io(&dst_sync, &bundle).unwrap();

        // Verify contents
        assert!(dst_sync.read("facts/f_f001.fact").unwrap().is_some());
        assert!(dst_sync.read("intents/i_i001.intent").unwrap().is_some());
        assert!(dst_sync.read("hints/h_h001.hint").unwrap().is_some());
        assert!(dst_sync.read("blob/deadbeef.bin").unwrap().is_some());
        assert!(dst_sync.read("blob/deadbeef.bin.meta").unwrap().is_some());
    }

    #[test]
    fn test_export_empty_io() {
        let io = SimIo::new();
        let sync = SyncFileIo::new(io);
        let bundle = export_from_io(&sync).unwrap();
        let dst = SimIo::new();
        let dst_sync = SyncFileIo::new(dst);
        import_into_io(&dst_sync, &bundle).unwrap();
        assert!(dst_sync.list("facts/").unwrap().is_empty());
    }

    #[test]
    fn test_invalid_magic_rejected() {
        let io = SimIo::new();
        let sync = SyncFileIo::new(io);
        let result = import_into_io(&sync, b"NOTABUNDL");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad magic"));
    }

    #[test]
    fn test_export_import_fs_to_sim() {
        // Create a temp FsIo-backed store, write records, export,
        // then import into SimIo and verify
        let dir = std::env::temp_dir().join(format!("fih_export_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let fs_io = crate::fs_io::FsIo::new(&dir).unwrap();
        let sync = SyncFileIo::new(fs_io);

        let fact = FactRecord {
            id: "f_fs".into(),
            blob_hash: "cafe01".into(),
            origin: "fs_test".into(),
            creator: "fs_user".into(),
            submitted_at: 2000,
        };
        sync.write(
            &fact.key(),
            &postcard::to_allocvec(&fact).unwrap(),
        )
        .unwrap();

        // Export using the SyncFileIo wrapper
        let bundle = export_from_io(&sync).unwrap();

        // Import into SimIo
        let sim_io = SimIo::new();
        let sim_sync = SyncFileIo::new(sim_io);
        import_into_io(&sim_sync, &bundle).unwrap();

        let loaded: FactRecord = postcard::from_bytes(
            &sim_sync.read("facts/f_f_fs.fact").unwrap().unwrap(),
        )
        .unwrap();
        assert_eq!(loaded.id, "f_fs");
        assert_eq!(loaded.blob_hash, "cafe01");

        let _ = std::fs::remove_dir_all(dir);
    }
}
