// The storage traits' default implementations and blanket impls.
//
// A consumer implements a published trait and plugs it in. Two things make that work
// without every implementor writing them: the defaults a backend inherits (an eviction
// no-op, a query that says it is unsupported) and the blanket impls that let a borrow be
// used wherever the owner is. `NullStorage` is the reference implementor for the first,
// and it is itself the smallest consumer of the second.

use fih_model::{
    ColdQuery, ColdStorage, Content, CoordId, EvictCapable, Fact, FactCapable, FilterCapable, Hint,
    HintCapable, Intent, IntentCapable, NullStorage, QueryCapable, ScanCapable, StateFilter,
    StorageRead, TimeRangeCapable,
};

fn base_fact() -> Fact {
    Fact::new("origin".into(), Content::from("payload"), "creator".into())
}

fn base_intent(fact: &Fact) -> Intent {
    Intent::new(
        CoordId::resolve("i_null"),
        vec![fact.id],
        None,
        "an intent".into(),
        "creator".into(),
    )
}

fn base_hint() -> Hint {
    Hint {
        id: CoordId::resolve("h_null"),
        content: "a hint".into(),
        creator: "creator".into(),
    }
}

#[test]
fn the_null_storage_answers_every_trait_with_its_no_op() {
    let storage = NullStorage;
    let fact = base_fact();
    let intent = base_intent(&fact);

    assert_eq!(StorageRead::project_id(&storage), "default");
    let state = StorageRead::read_state(&storage);
    assert!(state.facts.is_empty() && state.intents.is_empty() && state.hints.is_empty());
    assert_eq!(
        FactCapable::submit_fact(&storage, &fact).expect("nothing is refused"),
        fact.id
    );
    assert_eq!(
        IntentCapable::submit_intent(&storage, &intent).expect("nothing is refused"),
        intent.id
    );
    assert!(IntentCapable::claim_intent(&storage, "i_null", "agent").is_ok());
    assert!(IntentCapable::heartbeat(&storage, "i_null", "agent").is_ok());
    assert!(IntentCapable::release_intent(&storage, "i_null", "agent").is_ok());
    let conclusion =
        IntentCapable::conclude_intent(&storage, "i_null", "result").expect("nothing is refused");
    assert_eq!(conclusion.content.as_str(), Some("null"));
    assert!(HintCapable::submit_hint(&storage, &base_hint()).is_ok());

    let filtered = FilterCapable::read_state_filtered(&storage, &StateFilter::default());
    assert!(filtered.facts.is_empty() && filtered.intents.is_empty() && filtered.hints.is_empty());

    let partition = ScanCapable::scan_partition(&storage, "alpha").expect("nothing is refused");
    assert_eq!(partition.partition, "alpha");
    assert!(partition.facts.is_empty());

    assert_eq!(EvictCapable::approximate_size(&storage), 0);
    assert_eq!(
        EvictCapable::evict_before(&storage, "0").expect("nothing is refused"),
        0
    );
    assert_eq!(
        EvictCapable::evict_stale_intents(&storage, 60).expect("the default is a no-op"),
        0
    );
    assert!(ColdStorage::write_blob(&storage, "key", b"data").is_ok());
    assert!(TimeRangeCapable::time_range(&storage).is_none());

    let plan = ColdQuery::new("Fact");
    assert_eq!(plan.label, "Fact");
    assert!(plan.filters.is_empty() && plan.projections.is_empty());
    let error = QueryCapable::query_plan(&storage, &plan).expect_err("the default is unsupported");
    assert!(error.contains("not yet implemented"), "{error}");
}

#[test]
fn a_borrowed_storage_answers_as_the_owner_does() {
    let mut storage = NullStorage;
    let fact = base_fact();
    let intent = base_intent(&fact);
    let hint = base_hint();

    let borrowed = &storage;
    assert_eq!(
        <&NullStorage as StorageRead>::project_id(&borrowed),
        "default"
    );
    assert!(
        <&NullStorage as StorageRead>::read_state(&borrowed)
            .facts
            .is_empty()
    );
    assert_eq!(
        <&NullStorage as FactCapable>::submit_fact(&borrowed, &fact).expect("nothing is refused"),
        fact.id
    );
    assert_eq!(
        <&NullStorage as IntentCapable>::submit_intent(&borrowed, &intent)
            .expect("nothing is refused"),
        intent.id
    );
    assert!(<&NullStorage as IntentCapable>::claim_intent(&borrowed, "i_null", "agent").is_ok());
    assert!(<&NullStorage as IntentCapable>::heartbeat(&borrowed, "i_null", "agent").is_ok());
    assert!(<&NullStorage as IntentCapable>::release_intent(&borrowed, "i_null", "agent").is_ok());
    assert!(
        <&NullStorage as IntentCapable>::conclude_intent(&borrowed, "i_null", "result").is_ok()
    );
    assert!(<&NullStorage as HintCapable>::submit_hint(&borrowed, &hint).is_ok());

    let borrowed = &mut storage;
    assert_eq!(
        <&mut NullStorage as StorageRead>::project_id(&borrowed),
        "default"
    );
    assert!(
        <&mut NullStorage as StorageRead>::read_state(&borrowed)
            .facts
            .is_empty()
    );
    assert_eq!(
        <&mut NullStorage as FactCapable>::submit_fact(&borrowed, &fact)
            .expect("nothing is refused"),
        fact.id
    );
    assert_eq!(
        <&mut NullStorage as IntentCapable>::submit_intent(&borrowed, &intent)
            .expect("nothing is refused"),
        intent.id
    );
    assert!(
        <&mut NullStorage as IntentCapable>::claim_intent(&borrowed, "i_null", "agent").is_ok()
    );
    assert!(<&mut NullStorage as IntentCapable>::heartbeat(&borrowed, "i_null", "agent").is_ok());
    assert!(
        <&mut NullStorage as IntentCapable>::release_intent(&borrowed, "i_null", "agent").is_ok()
    );
    assert!(
        <&mut NullStorage as IntentCapable>::conclude_intent(&borrowed, "i_null", "result").is_ok()
    );
    assert!(<&mut NullStorage as HintCapable>::submit_hint(&borrowed, &hint).is_ok());
}
