//! Every record's content materializes at scale.
//!
//! A read resolves its content from the medium and this node keeps no cache, so a store
//! holding more records than a bounded cache would hold answers the same bytes. The
//! assertion is the content, which is what a reader holds a node to.

use futures_executor::block_on;
use nex_fih::FihId20 as _;
use nex_fih::{AsyncFactCapable, Content, CoordId, Fact};
use nexus_storage_ref::{RefFihStorage};
use nex_io::SimIo;

/// More records than a bounded cache would hold, so nothing is served from one.
const N_FACTS: u32 = 10_100;

#[test]
fn every_record_reads_its_content_back() {
    block_on(async {
        let store = RefFihStorage::new(SimIo::new(), "cache");

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

        // The first and the last record both read their content back from the medium.
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

        // A second read answers the same, which a cache would also have to do.
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
