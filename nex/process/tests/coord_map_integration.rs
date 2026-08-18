// Flagship path: FihStorage over the materialized CoordMap (chton
// CoordMapStore) with a file origin. Insert -> flush -> reopen -> read:
// a fact written in the first session survives a reopened store.

use chton::io::CoordMapStoreIo;
use chton::map::CoordMapStore;
use chton::origin::FileOrigin;
use futures_executor::block_on;
use nex_fih::io::file_io::BufferIo;
use nex_fih::{AsyncFactCapable, AsyncStorageRead, Content, CoordId, Fact, FihStorage};

fn fact(id: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        "test".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"flagship content".to_vec(),
        },
        "tester".into(),
    )
}

#[test]
fn fih_over_materialized_coordmap_persists_across_reopen() {
    block_on(async {
        let path = std::env::temp_dir().join(format!("nex-coordmap-{}.bin", std::process::id()));
        let path2 = path.clone();
        let fid = CoordId::resolve("f_persist").to_string();

        // First session: write a fact through FihStorage over CoordMapStoreIo.
        // Depth 79 holds the longest FIH path (blob meta paths reach 78
        // bytes) under the injective length-prefix key contract.
        {
            let map = CoordMapStore::<79>::load(Box::new(FileOrigin::open(&path).unwrap()), 4096)
                .unwrap();
            let storage = FihStorage::new(CoordMapStoreIo::new(map), "coordmap");
            storage.submit_fact(&fact("f_persist")).await.unwrap();
            storage.flush_pending().await.unwrap();
            // Persist the buffered map header and the file bytes.
            assert!(storage.io.is_buffered());
            storage.io.flush().await.unwrap();
            assert!(!storage.io.is_buffered());
        }

        // Second session: reopen the file into a fresh map, rebuild, read.
        {
            let map = CoordMapStore::<79>::load(Box::new(FileOrigin::open(&path2).unwrap()), 4096)
                .unwrap();
            let storage = FihStorage::new(CoordMapStoreIo::new(map), "coordmap");
            storage.rebuild_cache().await.unwrap();
            let state = storage.read_state().await;
            assert!(
                state.facts.iter().any(|f| f.id.to_string() == fid),
                "fact should survive the materialized CoordMap reopen"
            );
        }
        std::fs::remove_file(&path).unwrap();
    });
}
