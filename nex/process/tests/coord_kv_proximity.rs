// ── Phase 2 (#166): search.json scenario over the materialized CoordKV ──
//
// Documents are indexed into a coordinate space (each axis is a
// vocabulary term, the coordinate is the term count), and a CoordCubeKV
// proximity query retrieves the matching entries. The scenario is
// verified over both file origins (FileOrigin and MappedFileOrigin) and
// across a flush + reopen, so the coordinate index is durable.

use chton::kv::MaterialKv;
use chton::origin::FileOrigin;
#[cfg(unix)]
use chton::origin::MappedFileOrigin;
use tagma_core::{Coord, CoordPath};
use tagma_kv::CoordKV;
use tagma_kv::coord_cube_kv::CoordCubeKV;

/// Vocabulary axes of the document space. Axis i holds the count of
/// vocabulary[i] in a document, so documents sharing terms are nearby
/// in coordinate space.
const VOCABULARY: [&str; 6] = ["storage", "semantic", "fact", "index", "search", "vector"];

/// A search.json-style document.
struct Doc {
    id: &'static str,
    title: &'static str,
    text: &'static str,
}

/// Documents for the scenario. The term-count vectors are separated so
/// that proximity results are unambiguous: the storage cluster sits near
/// the origin, the semantic and search clusters are far from it.
const DOCS: &[Doc] = &[
    Doc {
        id: "doc-storage",
        title: "Storage architecture",
        text: "the storage storage storage layer keeps storage records",
    },
    Doc {
        id: "doc-storage-facts",
        title: "Storage facts",
        text: "storage facts storage index",
    },
    Doc {
        id: "doc-semantic",
        title: "Semantic vector search",
        text: "semantic semantic vector search",
    },
    Doc {
        id: "doc-search-index",
        title: "Search index",
        text: "search index search search",
    },
];

/// Map a document to its coordinate path: axis i counts vocabulary[i]
/// occurrences in the lowercased title and text.
fn doc_path(doc: &Doc) -> CoordPath<6> {
    let lower = format!("{} {}", doc.title, doc.text).to_lowercase();
    let mut coords = [Coord::new(0).unwrap(); 6];
    for (i, term) in VOCABULARY.iter().enumerate() {
        let count = lower.matches(term).count() as u16;
        coords[i] = Coord::new(count.min(11171)).unwrap();
    }
    CoordPath::new(coords)
}

/// Index the documents into `kv` at their coordinate paths. The value is
/// the document id, the opaque record at the codec seam.
fn index_docs(kv: &mut MaterialKv<6>, docs: &[Doc]) {
    for doc in docs {
        let previous = kv.put_path(&doc_path(doc), doc.id.as_bytes()).unwrap();
        assert!(previous.is_none(), "duplicate path for {}", doc.id);
    }
}

/// Collect the ids stored in proximity or bounding-box results, sorted.
fn collect_ids(results: Vec<(CoordPath<6>, Vec<u8>)>) -> Vec<String> {
    let mut ids: Vec<String> = results
        .into_iter()
        .map(|(_, value)| String::from_utf8(value).expect("value is a doc id"))
        .collect();
    ids.sort();
    ids
}

/// The center of the storage cluster: doc-storage-facts at (3,0,2,1,0,0).
fn storage_center() -> CoordPath<6> {
    doc_path(&DOCS[1])
}

/// A temp file path unique to this test process.
fn temp_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "nex-coordkv-prox-{label}-{}.bin",
        std::process::id()
    ))
}

#[test]
fn proximity_search_retrieves_matching_entries() {
    let path = temp_path("query");
    {
        let mut kv = MaterialKv::<6>::new(Box::new(FileOrigin::open(&path).unwrap()), 256);
        index_docs(&mut kv, DOCS);
        assert_eq!(kv.len(), DOCS.len());

        // Radius 1 from the storage center finds only the center doc.
        let r1 = kv.proximity::<6, 1>(&storage_center(), 1);
        assert_eq!(collect_ids(r1), vec!["doc-storage-facts"]);

        // Radius 2 reaches the storage-heavy document, still not the
        // semantic or search clusters (distances 3 and 4).
        let r2 = kv.proximity::<6, 1>(&storage_center(), 2);
        assert_eq!(collect_ids(r2), vec!["doc-storage", "doc-storage-facts"]);

        // Bounding box over the storage cluster: the same two entries.
        let ranges = [(2u16, 6u16), (0, 0), (0, 3), (0, 2), (0, 0), (0, 0)];
        let bb = kv.bounding_box_range(&ranges);
        assert_eq!(collect_ids(bb), vec!["doc-storage", "doc-storage-facts"]);
    }
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn proximity_search_persists_across_file_reopen() {
    let path = temp_path("reopen");
    {
        let mut kv = MaterialKv::<6>::new(Box::new(FileOrigin::open(&path).unwrap()), 256);
        index_docs(&mut kv, DOCS);
        kv.flush().unwrap();
    }
    {
        let kv = MaterialKv::<6>::load(Box::new(FileOrigin::open(&path).unwrap()), 256).unwrap();
        assert_eq!(kv.len(), DOCS.len());
        let results = kv.proximity::<6, 1>(&storage_center(), 2);
        assert_eq!(
            collect_ids(results),
            vec!["doc-storage", "doc-storage-facts"]
        );
    }
    std::fs::remove_file(&path).unwrap();
}

#[cfg(unix)]
#[test]
fn proximity_search_over_mapped_file_origin() {
    let path = temp_path("mapped");
    {
        let mut kv = MaterialKv::<6>::new(Box::new(MappedFileOrigin::open(&path).unwrap()), 256);
        index_docs(&mut kv, DOCS);
        kv.flush().unwrap();
    }
    {
        let kv =
            MaterialKv::<6>::load(Box::new(MappedFileOrigin::open(&path).unwrap()), 256).unwrap();
        let results = kv.proximity::<6, 1>(&storage_center(), 2);
        assert_eq!(
            collect_ids(results),
            vec!["doc-storage", "doc-storage-facts"]
        );
    }
    std::fs::remove_file(&path).unwrap();
}
