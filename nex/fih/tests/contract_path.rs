// The governed path: gate, storage, and evidence in one call.
//
// `FihContract` is what an application uses when it wants governance rather than a raw
// store. The gate decides whether a schema may be admitted, storage decides whether the
// record is acceptable, and the evidence chain records that the transition happened. A
// failure in either check has to leave the chain untouched, because the chain is the
// audit trail rather than a log of attempts.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead, BlackboardError, Content, CoordId,
    Fact, FihContract, FihStorage, HintRule, Intent,
};

/// An in-memory medium shared by every storage that holds a clone of it.
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

fn fact(payload: &str) -> Fact {
    Fact::new("origin/one".into(), Content::from(payload), "agent".into())
}

/// An intent has to name at least one fact, so each conclusion test seeds one.
fn seed_fact(storage: &FihStorage<MemoryIo>) -> Fact {
    let fact = fact("the fact an intent refers to");
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
    fact
}

#[test]
fn a_governed_fact_passes_the_gate_and_is_recorded() {
    let storage = FihStorage::new(MemoryIo::default(), "contract");
    let contract = FihContract::new();
    contract.register_default_schemas();

    let fact = fact("hello");
    let id = block_on(contract.submit_fact(&storage, &fact, None)).expect("the fact is admitted");

    assert_eq!(id, fact.id);
    assert_eq!(block_on(storage.read_state()).facts.len(), 1);
    assert_eq!(contract.evidence.len(), 1);

    let entry = &contract.evidence.entries()[0];
    assert_eq!(entry.action_type, "fact:submit");
    assert_eq!(entry.action_hash, fact.id.to_string());
    assert!(
        entry.timestamp_ns > 0,
        "the injected clock stamped the entry"
    );
}

#[test]
fn a_schema_the_gate_does_not_know_leaves_the_store_and_the_chain_untouched() {
    let storage = FihStorage::new(MemoryIo::default(), "contract");
    let contract = FihContract::new();
    contract.register_default_schemas();

    let fact = Fact::new(
        "origin/one".into(),
        Content {
            mime_type: "application/x-unregistered".into(),
            data: b"raw".to_vec(),
        },
        "agent".into(),
    );

    let error =
        block_on(contract.submit_fact(&storage, &fact, None)).expect_err("the gate refuses");
    assert!(matches!(error, BlackboardError::Forbidden(_)), "{error:?}");
    assert_eq!(contract.evidence.len(), 0);
    assert!(block_on(storage.read_state()).facts.is_empty());
}

#[test]
fn a_conclusion_is_checked_against_the_hints_and_recorded() {
    let storage = FihStorage::new(MemoryIo::default(), "contract");
    let contract = FihContract::new();
    contract.register_default_schemas();
    contract.hints.add("positive", HintRule::Gt(0));

    let seed = seed_fact(&storage);
    let intent = Intent::new(
        CoordId::resolve("i_contract"),
        vec![seed.id],
        None,
        "an intent to conclude".to_string(),
        "agent".to_string(),
    );
    block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    block_on(storage.claim_intent(&intent.id.to_string(), "agent")).expect("the intent is claimed");

    let concluded = block_on(contract.conclude_intent(&storage, &intent.id.to_string(), "42"))
        .expect("the result satisfies the hint");
    assert_eq!(concluded.content.data, b"42");
    assert_eq!(contract.evidence.len(), 1);
    assert_eq!(
        contract.evidence.entries()[0].action_type,
        "intent:conclude"
    );
}

#[test]
fn a_result_the_hints_reject_does_not_conclude_the_intent() {
    let storage = FihStorage::new(MemoryIo::default(), "contract");
    let contract = FihContract::new();
    contract.register_default_schemas();
    contract.hints.add("positive", HintRule::Gt(0));

    let seed = seed_fact(&storage);
    let intent = Intent::new(
        CoordId::resolve("i_rejected"),
        vec![seed.id],
        None,
        "an intent whose result is outside the rule".to_string(),
        "agent".to_string(),
    );
    block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    block_on(storage.claim_intent(&intent.id.to_string(), "agent")).expect("the intent is claimed");

    let error = block_on(contract.conclude_intent(&storage, &intent.id.to_string(), "0"))
        .expect_err("the hint refuses the result");
    assert!(matches!(error, BlackboardError::Forbidden(_)), "{error:?}");
    assert_eq!(contract.evidence.len(), 0, "a refusal is not a transition");
}

#[test]
fn the_default_contract_wires_the_host_clock_and_the_default_schemas() {
    let contract = FihContract::default();
    contract.register_default_schemas();

    assert_eq!(contract.gate.schema_count(), 4);
    assert!(contract.gate.has_schema("text/plain"));
}
