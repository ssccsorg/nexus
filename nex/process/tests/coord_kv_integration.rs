// Flagship path: FihStorage over the materialized CoordKV (chton
// CoordKVStore) with a file origin. Insert -> flush -> reopen -> read:
// a fact written in the first session survives a reopened store.

use chton::io::CoordKVStoreIo;
use chton::kv::CoordKVStore;
use chton::origin::FileOrigin;
use futures_executor::block_on;
use nex_fih::io::file_io::BufferIo;
use nex_fih::{AsyncFactCapable, AsyncStorageRead, Content, CoordId, Fact, FihStorage};

fn fact(id: &str) -> Fact {
    Fact::with_id(
        CoordId::from_string(id),
        "test".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"flagship content".to_vec(),
        },
        "tester".into(),
    )
}

#[test]
fn fih_over_materialized_coordkv_persists_across_reopen() {
    block_on(async {
        let path = std::env::temp_dir().join(format!("nex-coordkv-{}.bin", std::process::id()));
        let path2 = path.clone();
        let fid = CoordId::from_string("f_persist").to_string();

        // First session: write a fact through FihStorage over CoordKVStoreIo.
        {
            let kv =
                CoordKVStore::<16>::load(Box::new(FileOrigin::open(&path).unwrap()), 4096).unwrap();
            let storage = FihStorage::new(CoordKVStoreIo::new(kv), "coordkv");
            storage.submit_fact(&fact("f_persist")).await.unwrap();
            storage.flush_pending().await.unwrap();
            // Persist the buffered kv header and the file bytes.
            assert!(storage.io.is_buffered());
            storage.io.flush().await.unwrap();
            assert!(!storage.io.is_buffered());
        }

        // Second session: reopen the file into a fresh kv, rebuild, read.
        {
            let kv =
                CoordKVStore::<16>::load(Box::new(FileOrigin::open(&path2).unwrap()), 4096).unwrap();
            let storage = FihStorage::new(CoordKVStoreIo::new(kv), "coordkv");
            storage.rebuild_cache().await.unwrap();
            let state = storage.read_state().await;
            assert!(
                state.facts.iter().any(|f| f.id.to_string() == fid),
                "fact should survive the materialized CoordKV reopen"
            );
        }
        std::fs::remove_file(&path).unwrap();
    });
}
