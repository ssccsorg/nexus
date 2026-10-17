// A conclusion belongs to the intent, so its address cannot depend on how the caller
// spelled the intent's name.
//
// Intent references come in two forms: the 20-Hangul canonical id and a label the engine
// resolves to it. `conclude_intent` reaches the same record through either, so it has to
// produce the same conclusion fact through either. A derivation from the caller's string
// makes one intent conclude at two addresses, which a replay cannot reproduce.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{AsyncFactCapable, AsyncIntentCapable, Content, CoordId, Fact, FihStorage, Intent};

#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl FileIo for MemoryIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move { Ok(map.lock().unwrap().get(path).cloned()) })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            map.lock().unwrap().insert(path.to_string(), data.to_vec());
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            let mut keys: Vec<String> = map
                .lock()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect();
            keys.sort();
            Ok(keys)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            map.lock().unwrap().remove(path);
            Ok(())
        })
    }
}

/// Conclude the same intent through `reference`, on a volume of its own.
fn conclude_through(reference: &str) -> Fact {
    let storage = FihStorage::new(MemoryIo::default(), "identity");
    let base = Fact::with_id(
        CoordId::resolve("f_base"),
        "origin/one".into(),
        Content::from("base"),
        "agent".into(),
    );
    block_on(storage.submit_fact(&base)).expect("the base fact is accepted");

    let intent = Intent::new(
        CoordId::resolve("i_ref"),
        vec![base.id],
        None,
        "an intent to conclude".into(),
        "agent".into(),
    );
    block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    block_on(storage.claim_intent("i_ref", "worker")).expect("the intent is claimed");

    block_on(storage.conclude_intent(reference, "resolved")).expect("the intent concludes")
}

#[test]
fn one_intent_concludes_at_one_address_whichever_name_the_caller_uses() {
    let canonical = CoordId::resolve("i_ref").to_string();

    let by_label = conclude_through("i_ref");
    let by_canonical = conclude_through(&canonical);

    assert_eq!(
        by_label.id, by_canonical.id,
        "the reference spelling must not change the conclusion address"
    );
    assert_eq!(by_label.origin, by_canonical.origin);
    assert_eq!(by_label.origin, format!("conclusion:{canonical}"));
    assert_eq!(by_label.content.data, b"resolved");
}
