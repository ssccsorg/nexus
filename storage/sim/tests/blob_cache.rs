//! The content-addressed blob cache is bounded: when it exceeds the cap
//! the oldest entries are evicted and reloaded from io on a later read.
//! The current read always resolves its content, so eviction must never
//! starve the read that triggered it.

use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, AsyncStorageRead, Content, CoordId, Fact, FihStorage};
use nexus_storage_sim::SimIo;

/// Exceeds the cache cap (10_000) so the earliest blobs are evicted.
const N_FACTS: u32 = 10_100;

#[test]
fn test_blob_cache_evicts_and_reloads() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "cache");

        for i in 0..N_FACTS {
            store
                .submit_fact(&Fact::with_id(
                    CoordId::from_label(&format!("c{i}")),
                    "t".into(),
                    Content::from(format!("payload {i}")),
                    "t".into(),
                ))
                .await
                .unwrap();
        }

        let state = store.read_state().await;
        assert_eq!(state.facts.len(), N_FACTS as usize);

        // The earliest facts' blobs are evicted (FIFO) and reload from io;
        // the latest stay cached. Both must materialize correctly.
        let content_of = |id: &str| -> String {
            state
                .facts
                .iter()
                .find(|f| f.id.to_string() == CoordId::from_label(id).to_string())
                .unwrap_or_else(|| panic!("fact {id} missing from state"))
                .content
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(content_of("c0"), "payload 0", "evicted blob reloads");
        assert_eq!(
            content_of(&format!("c{}", N_FACTS - 1)),
            format!("payload {}", N_FACTS - 1)
        );

        // A second read serves the cached tail without io and stays correct.
        let state2 = store.read_state().await;
        assert_eq!(state2.facts.len(), N_FACTS as usize);
        assert_eq!(
            state2
                .facts
                .iter()
                .find(|f| f.id.to_string() == CoordId::from_label("c0").to_string())
                .unwrap()
                .content
                .as_str()
                .unwrap_or_default(),
            "payload 0"
        );
    });
}
