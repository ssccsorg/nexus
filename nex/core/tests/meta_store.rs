// The metadata store contract: the two methods a backend owns and the two defaults the
// blanket impl supplies.
//
// `MetaStore` carries scalars such as a cursor or a snapshot pointer. `KeyValueStore`
// exists so a legacy caller compiles against it, and its defaults are deliberately empty
// and inert rather than pretending to store anything. Both are public contracts, so both
// are pinned here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nex_core::storage::meta_store::{KeyValueStore, MetaStore};

/// A backend that owns nothing but the two required methods.
#[derive(Clone, Default)]
struct MemoryMeta(Arc<Mutex<HashMap<String, String>>>);

impl MetaStore for MemoryMeta {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }
}

#[test]
fn a_meta_store_gets_and_sets_a_scalar() {
    let store = MemoryMeta::default();

    assert_eq!(store.get("cursor").expect("the lookup succeeds"), None);
    store.set("cursor", "42").expect("the write succeeds");
    assert_eq!(
        store.get("cursor").expect("the lookup succeeds").as_deref(),
        Some("42")
    );
}

#[test]
fn any_meta_store_is_a_key_value_store_whose_defaults_are_inert() {
    let store = MemoryMeta::default();
    store.set("cursor", "42").expect("the write succeeds");

    // The defaults exist for compatibility, and a caller that relies on them gets an
    // empty listing and a delete that removes nothing.
    assert!(
        KeyValueStore::list(&store, "cursor")
            .expect("the listing succeeds")
            .is_empty()
    );
    KeyValueStore::delete(&store, "cursor").expect("the delete succeeds");
    assert_eq!(
        store.get("cursor").expect("the lookup succeeds").as_deref(),
        Some("42"),
        "the default delete is a no-op"
    );
}
