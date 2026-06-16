// ── FihExport / FihImport: portable StateSpace bundle ──────────────────
//
// Export full FIH StateSpace (chain files + blob store) into a single
// portable bundle file (.fihbundle), and import it back into any
// AsyncFileIo-backed storage.
//
// Bundle format: a single postcard-serialized Bundle struct:
//   magic(8B) | postcard(Bundle { header, facts, intents, hints, blobs })
//
// postcard already handles length-delimited arrays internally
// (u32 LE prefix per Vec), so no manual framing is needed.
//
// Implementations:
//   - FsIo-based: export local FsIo → file, import file → FsIo
//   - SimIo-based: export/import in-memory (for testing)
//   - CfFihIo: (future) export/import over R2

use super::record::{ContentMeta, FactRecord, HintRecord, IntentRecord};
use crate::io::file_io::{AsyncFileIo, SyncFileIo};

/// Magic bytes for .fihbundle format identification.
const BUNDLE_MAGIC: &[u8; 8] = b"FIHBUNDL";

/// Complete bundle: all StateSpace records in one postcard-serialized struct.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
struct Bundle {
    version: u16,
    facts: Vec<FactRecord>,
    intents: Vec<IntentRecord>,
    hints: Vec<HintRecord>,
    blobs: Vec<BlobEntry>,
}

/// Container for a blob entry (content + metadata).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct BlobEntry {
    hash: String,
    data: Vec<u8>,
    meta: ContentMeta,
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

    // Collect blob entries (pair .bin + .bin.meta).
    // Use a HashSet to find metadata regardless of list ordering,
    // avoiding fragility across different IO backends.
    let blob_keys = sync.list("blob/")?;
    let meta_keys: std::collections::HashSet<String> = blob_keys
        .iter()
        .filter(|k| k.ends_with(".bin.meta"))
        .cloned()
        .collect();
    let mut blobs: Vec<BlobEntry> = Vec::new();
    for key in &blob_keys {
        if key.ends_with(".bin") && !key.ends_with(".bin.meta") {
            let hash = key
                .strip_prefix("blob/")
                .unwrap()
                .strip_suffix(".bin")
                .unwrap()
                .to_string();
            let data = sync.read(key)?.unwrap_or_default();
            let meta_key = format!("blob/{}.bin.meta", hash);
            let meta = if meta_keys.contains(&meta_key) {
                if let Some(mbytes) = sync.read(&meta_key)? {
                    postcard::from_bytes::<ContentMeta>(&mbytes).unwrap_or(ContentMeta {
                        mime_type: "application/octet-stream".into(),
                        size: data.len() as u64,
                    })
                } else {
                    ContentMeta {
                        mime_type: "application/octet-stream".into(),
                        size: data.len() as u64,
                    }
                }
            } else {
                ContentMeta {
                    mime_type: "application/octet-stream".into(),
                    size: data.len() as u64,
                }
            };
            blobs.push(BlobEntry { hash, data, meta });
        }
    }

    // Serialize entire bundle as a single postcard struct
    let bundle_struct = Bundle {
        version: 1,
        facts,
        intents,
        hints,
        blobs,
    };
    let mut bundle = Vec::with_capacity(8 + 1024);
    bundle.extend_from_slice(BUNDLE_MAGIC);
    let payload = postcard::to_allocvec(&bundle_struct).map_err(|e| e.to_string())?;
    bundle.extend_from_slice(&payload);

    Ok(bundle)
}

/// Import FIH StateSpace bundle into a sync IO handle.
///
/// Writes all records and blobs from the bundle into the IO,
/// overwriting any existing data at the same paths.
pub fn import_into_io<A: AsyncFileIo>(io: &SyncFileIo<A>, bundle: &[u8]) -> Result<(), String> {
    let sync = io;

    // Validate magic
    if bundle.len() < 8 || &bundle[..8] != BUNDLE_MAGIC {
        return Err("invalid bundle: bad magic".into());
    }

    // Deserialize entire bundle
    let bundle_struct: Bundle =
        postcard::from_bytes(&bundle[8..]).map_err(|e| format!("invalid bundle: {e}"))?;

    if bundle_struct.version != 1 {
        return Err(format!(
            "unsupported bundle version: {}",
            bundle_struct.version
        ));
    }

    // Write facts
    for record in &bundle_struct.facts {
        let bytes = postcard::to_allocvec(record).map_err(|e| e.to_string())?;
        sync.write(&record.key(), &bytes)?;
    }

    // Write intents
    for record in &bundle_struct.intents {
        let bytes = postcard::to_allocvec(record).map_err(|e| e.to_string())?;
        sync.write(&record.key(), &bytes)?;
    }

    // Write hints
    for record in &bundle_struct.hints {
        let bytes = postcard::to_allocvec(record).map_err(|e| e.to_string())?;
        sync.write(&record.key(), &bytes)?;
    }

    // Write blobs
    for entry in &bundle_struct.blobs {
        sync.write(&format!("blob/{}.bin", entry.hash), &entry.data)?;
        let meta_bytes = postcard::to_allocvec(&entry.meta).map_err(|e| e.to_string())?;
        sync.write(&format!("blob/{}.bin.meta", entry.hash), &meta_bytes)?;
    }

    Ok(())
}
