//! read_state and read_state_filtered must agree on identical states,
//! including record order (issue #173 acceptance). Both enumerate the
//! application-layer record maps in id-sorted order and materialize the
//! same content.

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead,
    Content, CoordId, Fact, FihStorage, Hint, Intent, StateFilter,
};
use nexus_storage_sim::SimIo;

#[test]
fn test_read_state_and_filtered_agree_in_order() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "agree");

        for id in ["z", "a", "m", "b"] {
            store
                .submit_fact(&Fact::with_id(
                    CoordId::resolve(id),
                    "t".into(),
                    Content::from(format!("content {id}")),
                    "t".into(),
                ))
                .await
                .unwrap();
        }
        for id in ["i_b", "i_a"] {
            store
                .submit_intent(&Intent {
                    id: CoordId::resolve(id),
                    from_facts: vec![CoordId::resolve("b")],
                    description: format!("desc {id}"),
                    creator: "t".into(),
                    worker: None,
                    to_fact_id: None,
                    last_heartbeat_at: None,
                    created_at: None,
                    is_concluded: false,
                    concluded_at: None,
                })
                .await
                .unwrap();
        }
        for id in ["h_z", "h_a"] {
            store
                .submit_hint(&Hint {
                    id: CoordId::resolve(id),
                    content: format!("hint {id}"),
                    creator: "t".into(),
                })
                .await
                .unwrap();
        }

        let full = store.read_state().await;
        let filtered = store.read_state_filtered(&StateFilter::default()).await;

        assert_eq!(full.facts.len(), filtered.facts.len());
        assert_eq!(full.intents.len(), filtered.intents.len());
        assert_eq!(full.hints.len(), filtered.hints.len());

        // Identical id order: both enumerate id-sorted.
        let full_fact_ids: Vec<String> = full.facts.iter().map(|f| f.id.to_string()).collect();
        let filtered_fact_ids: Vec<String> =
            filtered.facts.iter().map(|f| f.id.to_string()).collect();
        assert_eq!(full_fact_ids, filtered_fact_ids);
        let mut sorted = full_fact_ids.clone();
        sorted.sort();
        assert_eq!(full_fact_ids, sorted, "facts must be id-sorted");

        let full_intent_ids: Vec<String> = full.intents.iter().map(|i| i.id.to_string()).collect();
        let filtered_intent_ids: Vec<String> =
            filtered.intents.iter().map(|i| i.id.to_string()).collect();
        assert_eq!(full_intent_ids, filtered_intent_ids);

        let full_hint_ids: Vec<String> = full.hints.iter().map(|h| h.id.to_string()).collect();
        let filtered_hint_ids: Vec<String> =
            filtered.hints.iter().map(|h| h.id.to_string()).collect();
        assert_eq!(full_hint_ids, filtered_hint_ids);

        // Identical content and hashes.
        for (f1, f2) in full.facts.iter().zip(filtered.facts.iter()) {
            assert_eq!(f1.content.data, f2.content.data);
            assert_eq!(f1.content.mime_type, f2.content.mime_type);
            assert_eq!(f1.content_hash, f2.content_hash);
        }
        for (i1, i2) in full.intents.iter().zip(filtered.intents.iter()) {
            assert_eq!(i1.description, i2.description);
            assert_eq!(i1.is_concluded, i2.is_concluded);
        }
    });
}

#[test]
fn test_read_state_struct_without_content() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "light");
        store
            .submit_fact(&Fact::with_id(
                CoordId::resolve("f1"),
                "t".into(),
                Content::from("payload"),
                "t".into(),
            ))
            .await
            .unwrap();
        store
            .submit_intent(&Intent {
                id: CoordId::resolve("i1"),
                from_facts: vec![CoordId::resolve("f1")],
                description: "desc".into(),
                creator: "t".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            })
            .await
            .unwrap();

        let structure = store.read_state_struct().await;
        assert_eq!(structure.facts.len(), 1);
        assert_eq!(structure.facts[0].id, CoordId::resolve("f1"));
        assert!(
            structure.facts[0].content.data.is_empty(),
            "structure read must not materialize fact content"
        );
        assert_eq!(structure.intents.len(), 1);
        assert!(
            structure.intents[0].description.is_empty(),
            "structure read must not materialize intent descriptions"
        );
    });
}
