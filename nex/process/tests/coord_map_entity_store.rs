// MapEntityStore: CoordMapStore-backed EntityStore durability.
//
// The store is an EntityStore whose backing is chton's materialized
// CoordMap. These tests verify the trait surface (roundtrip, replace)
// and the durability contract (buffered state, flush, reopen, read)
// over both file origins.

#[cfg(unix)]
use chton::origin::MappedFileOrigin;
use chton::origin::{FileOrigin, MemoryOrigin};
use futures_executor::block_on;
use nex_fih::{EntityStore, MapEntityStore};

fn temp_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "nex-coordmap-entity-{label}-{}.bin",
        std::process::id()
    ))
}

#[test]
fn map_entity_store_roundtrip_via_trait() {
    block_on(async {
        let store = MapEntityStore::<6, String>::new(Box::new(MemoryOrigin::new()), 256);
        assert!(!store.is_buffered());

        assert_eq!(store.insert("alpha".into(), "one".into()).await, None);
        assert!(store.is_buffered(), "a write leaves the store buffered");
        assert_eq!(store.get("alpha").await.as_deref(), Some("one"));
        assert!(store.contains_key("alpha").await);
        assert_eq!(store.len().await, 1);

        // Overwrite returns the previous value.
        assert_eq!(
            store.insert("alpha".into(), "uno".into()).await.as_deref(),
            Some("one")
        );
        assert_eq!(store.get("alpha").await.as_deref(), Some("uno"));

        let mut values = store.values().await;
        values.sort();
        assert_eq!(values, vec!["uno".to_string()]);

        assert_eq!(store.remove("alpha").await.as_deref(), Some("uno"));
        assert_eq!(store.len().await, 0);
        assert!(!store.contains_key("alpha").await);
    });
}

#[test]
fn map_entity_store_persists_across_reopen() {
    let path = temp_path("reopen");
    block_on(async {
        {
            let store =
                MapEntityStore::<6, String>::new(Box::new(FileOrigin::open(&path).unwrap()), 256);
            store.insert("f_a".into(), "persisted-a".into()).await;
            store.insert("f_b".into(), "persisted-b".into()).await;
            assert!(store.is_buffered());
            store.flush().unwrap();
            assert!(!store.is_buffered());
        }
        {
            let store =
                MapEntityStore::<6, String>::load(Box::new(FileOrigin::open(&path).unwrap()), 256)
                    .unwrap();
            assert_eq!(store.len().await, 2);
            assert_eq!(store.get("f_a").await.as_deref(), Some("persisted-a"));
            assert_eq!(store.get("f_b").await.as_deref(), Some("persisted-b"));
        }
    });
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn map_entity_store_replace_from() {
    block_on(async {
        let store = MapEntityStore::<6, String>::new(Box::new(MemoryOrigin::new()), 256);
        store.insert("a".into(), "1".into()).await;
        store.insert("b".into(), "2".into()).await;
        store.replace_from(vec![("c".into(), "3".into())]).await;
        assert_eq!(store.len().await, 1);
        let mut values = store.values().await;
        values.sort();
        assert_eq!(values, vec!["3".to_string()]);
    });
}

#[cfg(unix)]
#[test]
fn map_entity_store_over_mapped_file_origin() {
    let path = temp_path("mapped");
    block_on(async {
        {
            let store = MapEntityStore::<6, String>::new(
                Box::new(MappedFileOrigin::open(&path).unwrap()),
                256,
            );
            store.insert("f_m".into(), "mapped".into()).await;
            store.flush().unwrap();
        }
        {
            let store = MapEntityStore::<6, String>::load(
                Box::new(MappedFileOrigin::open(&path).unwrap()),
                256,
            )
            .unwrap();
            assert_eq!(store.get("f_m").await.as_deref(), Some("mapped"));
        }
    });
    std::fs::remove_file(&path).unwrap();
}
