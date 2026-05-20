// nexus-table — Integration tests for SqlBlackboard + SqliteStorage.

use nexus_table::{
    Blackboard, BlackboardError, Fact, FihHash, Hint, Intent, SqlBlackboard, SqliteStorage, Storage,
};

fn make_fact(id: &str, content: &str) -> Fact {
    Fact {
        id: FihHash(id.into()),
        origin: "test".into(),
        content: serde_json::Value::String(content.into()),
        creator: "tester".into(),
    }
}

fn make_intent(id: &str, from: Vec<&str>, desc: &str) -> Intent {
    Intent {
        id: FihHash(id.into()),
        from_facts: from.into_iter().map(|s| s.to_string()).collect(),
        description: desc.into(),
        creator: "tester".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    }
}

#[test]
fn test_submit_fact() {
    let mut bb = SqlBlackboard::memory().unwrap();
    let hash = bb.submit_fact(&make_fact("f001", "test fact")).unwrap();
    assert_eq!(hash.0, "f001");
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].id.0, "f001");
}

#[test]
fn test_submit_intent() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&make_fact("f001", "source fact")).unwrap();
    let hash = bb
        .submit_intent(&make_intent("i001", vec!["f001"], "test intent"))
        .unwrap();
    assert_eq!(hash.0, "i001");
    let state = bb.read_state();
    assert_eq!(state.intents.len(), 1);
}

#[test]
fn test_intent_missing_fact() {
    let mut bb = SqlBlackboard::memory().unwrap();
    let result = bb.submit_intent(&make_intent("i001", vec!["f_nonexistent"], "test"));
    assert!(result.is_err());
}

#[test]
fn test_heartbeat_and_conclude() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&make_fact("f001", "source")).unwrap();
    bb.submit_intent(&make_intent("i001", vec!["f001"], "explore"))
        .unwrap();
    bb.heartbeat("i001", "agent-a").unwrap();
    let result = serde_json::Value::String("discovery!".into());
    let fact = bb.conclude_intent("i001", &result).unwrap();
    assert_eq!(fact.content, "discovery!");
    let state = bb.read_state();
    assert!(state.facts.iter().any(|f| f.content == "discovery!"));
}

#[test]
fn test_release_intent() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&make_fact("f001", "source")).unwrap();
    bb.submit_intent(&make_intent("i001", vec!["f001"], "explore"))
        .unwrap();
    bb.heartbeat("i001", "agent-a").unwrap();
    bb.release_intent("i001", "agent-a").unwrap();
    let state = bb.read_state();
    let intent = state.intents.iter().find(|i| i.id.0 == "i001").unwrap();
    assert!(intent.worker.is_none());
}

#[test]
fn test_concurrent_session() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    {
        let mut bb = SqlBlackboard::open(&path).unwrap();
        bb.submit_fact(&make_fact("f001", "persistent fact"))
            .unwrap();
        bb.submit_fact(&make_fact("f002", "another fact")).unwrap();
        bb.submit_intent(&make_intent("i001", vec!["f001"], "persistent intent"))
            .unwrap();
        assert_eq!(bb.read_state().facts.len(), 2);
    }
    {
        let bb = SqlBlackboard::open(&path).unwrap();
        let state = bb.read_state();
        assert_eq!(state.facts.len(), 2);
        assert_eq!(state.intents.len(), 1);
    }
}

#[test]
fn test_hint() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_hint(&Hint {
        id: FihHash("h001".into()),
        content: "check the web service first".into(),
        creator: "analyst".into(),
    })
    .unwrap();
    let state = bb.read_state();
    assert_eq!(state.hints.len(), 1);
}

#[test]
fn test_intent_sources_join_on_read() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&make_fact("f001", "observation")).unwrap();
    bb.submit_fact(&make_fact("f002", "inference")).unwrap();
    bb.submit_fact(&make_fact("f003", "conclusion")).unwrap();
    let intent = Intent {
        id: FihHash("i_hyper_001".into()),
        from_facts: vec!["f001".into(), "f002".into(), "f003".into()],
        description: "multi-source analysis".into(),
        creator: "researcher".into(),
        worker: Some("researcher".into()),
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    };
    bb.submit_intent(&intent).unwrap();
    let state = bb.read_state();
    let i = state
        .intents
        .iter()
        .find(|i| i.id.0 == "i_hyper_001")
        .unwrap();
    assert_eq!(i.from_facts.len(), 3);
}

#[test]
fn test_playbook_sre_lifecycle() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_deploy_001".into()),
        origin: "ci-bot".into(),
        content: serde_json::json!({"event": "deploy_complete", "service": "api-gateway", "version": "v2.4.1", "duration_ms": 3420, "status": "success"}),
        creator: "ci-bot".into(),
    }).unwrap();
    bb.submit_intent(&Intent {
        id: FihHash("i_sre_001".into()),
        from_facts: vec!["f_deploy_001".into()],
        description: "Investigate deploy duration regression (3.4s vs 1.2s baseline)".into(),
        creator: "sre-agent".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    })
    .unwrap();
    bb.heartbeat("i_sre_001", "sre-agent").unwrap();
    let finding = serde_json::json!({"finding": "healthcheck timeout regression", "fix": "Reduce timeout to 5s", "effort_hours": 0.5});
    let concluded = bb.conclude_intent("i_sre_001", &finding).unwrap();
    assert_eq!(
        concluded.content["finding"],
        "healthcheck timeout regression"
    );
    let state = bb.read_state();
    assert!(state.facts.iter().any(|f| f.creator == "sre-agent"));
}

#[test]
fn test_multi_blackboard_isolation() {
    let mut bb_sensor = SqlBlackboard::memory().unwrap();
    let mut bb_knowledge = SqlBlackboard::memory().unwrap();
    bb_sensor
        .submit_fact(&make_fact("f_s1", "sensor reading"))
        .unwrap();
    bb_knowledge
        .submit_fact(&make_fact("f_k1", "knowledge graph node"))
        .unwrap();
    bb_sensor
        .submit_intent(&make_intent("i_s1", vec!["f_s1"], "analyze sensor"))
        .unwrap();
    bb_knowledge
        .submit_intent(&make_intent("i_k1", vec!["f_k1"], "query knowledge"))
        .unwrap();
    assert_eq!(bb_sensor.read_state().facts.len(), 1);
    assert_eq!(bb_knowledge.read_state().facts.len(), 1);
}

#[test]
fn test_multi_agent_handoff() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&make_fact("f001", "discovery")).unwrap();
    bb.submit_intent(&make_intent("i001", vec!["f001"], "explore anomaly"))
        .unwrap();
    bb.heartbeat("i001", "agent-a").unwrap();
    bb.release_intent("i001", "agent-a").unwrap();
    bb.heartbeat("i001", "agent-b").unwrap();
    bb.conclude_intent("i001", &"resolved by agent-b".into())
        .unwrap();
    let state = bb.read_state();
    assert!(
        state
            .intents
            .iter()
            .find(|i| i.id.0 == "i001")
            .unwrap()
            .concluded_at
            .is_some()
    );
}

#[test]
fn test_protocol_enforcement() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&make_fact("f001", "data")).unwrap();
    bb.submit_intent(&make_intent("i001", vec!["f001"], "critical task"))
        .unwrap();
    bb.heartbeat("i001", "agent-a").unwrap();
    let err = bb.release_intent("i001", "intruder").unwrap_err();
    assert!(matches!(err, BlackboardError::Forbidden(_)));
    bb.release_intent("i001", "agent-a").unwrap();
    bb.heartbeat("i001", "agent-b").unwrap();
    let _concluded = bb.conclude_intent("i001", &"done".into()).unwrap();
    let err2 = bb.conclude_intent("i001", &"again".into()).unwrap_err();
    assert!(matches!(err2, BlackboardError::NotFound(_)));
}

#[test]
fn test_full_persistence_across_sessions() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    {
        let mut bb = SqlBlackboard::open(&path).unwrap();
        bb.submit_fact(&make_fact("f001", "alpha")).unwrap();
        bb.submit_fact(&make_fact("f002", "beta")).unwrap();
        bb.submit_intent(&make_intent("i001", vec!["f001"], "first"))
            .unwrap();
        bb.submit_intent(&make_intent("i002", vec!["f002"], "second"))
            .unwrap();
        bb.heartbeat("i001", "worker-x").unwrap();
        bb.conclude_intent("i001", &"result-a".into()).unwrap();
        bb.submit_hint(&Hint {
            id: FihHash("h001".into()),
            content: "strategic hint".into(),
            creator: "planner".into(),
        })
        .unwrap();
    }
    {
        let bb = SqlBlackboard::open(&path).unwrap();
        let state = bb.read_state();
        assert_eq!(state.facts.len(), 3);
        assert_eq!(state.intents.len(), 2);
        assert_eq!(state.hints.len(), 1);
    }
}

#[test]
fn test_structured_json_content() {
    let mut bb = SqlBlackboard::memory().unwrap();
    let complex_content = serde_json::json!({"nested": {"array": [1,2,3], "object": {"key":"value"}, "null": null, "bool": true, "number": 42.5}});
    bb.submit_fact(&Fact {
        id: FihHash("f_complex".into()),
        origin: "json-test".into(),
        content: complex_content.clone(),
        creator: "tester".into(),
    })
    .unwrap();
    let binding = bb.read_state();
    let fact = binding
        .facts
        .iter()
        .find(|f| f.id.0 == "f_complex")
        .unwrap();
    assert_eq!(
        fact.content["nested"]["array"],
        serde_json::json!([1, 2, 3])
    );
}

#[test]
fn test_research_cross_document_entity_linking() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_doc_a_001".into()),
        origin: "whitepaper.llms.md".into(),
        content: serde_json::json!({"concept":"homeomorphic verification","source":"whitepaper §3.4"}),
        creator: "doc-ingest-agent".into(),
    }).unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_doc_b_001".into()),
        origin: "nexus-readme.llms.md".into(),
        content: serde_json::json!({"concept":"boundaryless extension","source":"README.md"}),
        creator: "doc-ingest-agent".into(),
    })
    .unwrap();
    bb.submit_intent(&Intent {
        id: FihHash("i_research_001".into()),
        from_facts: vec!["f_doc_a_001".into(), "f_doc_b_001".into()],
        description: "Link homeomorphic verification and boundaryless extension".into(),
        creator: "cross-ref-agent".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    })
    .unwrap();
    bb.heartbeat("i_research_001", "review-agent").unwrap();
    bb.conclude_intent(
        "i_research_001",
        &serde_json::json!({"finding": "linked", "confidence": 0.92}),
    )
    .unwrap();
    let state = bb.read_state();
    assert!(
        state
            .intents
            .iter()
            .find(|i| i.id.0 == "i_research_001")
            .unwrap()
            .concluded_at
            .is_some()
    );
}

#[test]
fn test_research_contradiction_detection() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_claim_a".into()),
        origin: "doc1".into(),
        content: serde_json::json!({"claim": "A"}),
        creator: "ingest".into(),
    })
    .unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_claim_b".into()),
        origin: "doc2".into(),
        content: serde_json::json!({"claim": "not A"}),
        creator: "ingest".into(),
    })
    .unwrap();
    bb.submit_intent(&Intent {
        id: FihHash("i_contradiction_001".into()),
        from_facts: vec!["f_claim_a".into(), "f_claim_b".into()],
        description: "CONTRADICTION".into(),
        creator: "gap-detector".into(),
        worker: Some("gap-detector".into()),
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    })
    .unwrap();
    let state = bb.read_state();
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.facts.len(), 2);
}

#[test]
fn test_research_concept_drift_across_sources() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_v1".into()),
        origin: "doc1".into(),
        content: serde_json::json!({"concept":"Evolving Memory"}),
        creator: "ingest".into(),
    })
    .unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_v2".into()),
        origin: "doc2".into(),
        content: serde_json::json!({"concept":"eKG"}),
        creator: "ingest".into(),
    })
    .unwrap();
    bb.submit_intent(&Intent {
        id: FihHash("i_drift_001".into()),
        from_facts: vec!["f_v1".into(), "f_v2".into()],
        description: "Track concept evolution".into(),
        creator: "tracker".into(),
        worker: Some("tracker".into()),
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    })
    .unwrap();
    bb.submit_hint(&Hint {
        id: FihHash("h_drift_001".into()),
        content: "check backward compat".into(),
        creator: "reviewer".into(),
    })
    .unwrap();
    let state = bb.read_state();
    assert_eq!(state.hints.len(), 1);
}

#[test]
fn test_research_gap_unexplored_territory() {
    let mut bb = SqlBlackboard::memory().unwrap();
    bb.submit_fact(&make_fact("f_gap_a", "topic A")).unwrap();
    bb.submit_fact(&make_fact("f_gap_b", "topic B")).unwrap();
    bb.submit_fact(&make_fact("f_gap_c", "topic C")).unwrap();
    bb.submit_intent(&Intent {
        id: FihHash("i_gap_001".into()),
        from_facts: vec!["f_gap_a".into(), "f_gap_b".into(), "f_gap_c".into()],
        description: "GAP: unexplored connection".into(),
        creator: "gap-detector".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    })
    .unwrap();
    let state = bb.read_state();
    assert_eq!(
        state
            .intents
            .iter()
            .find(|i| i.id.0 == "i_gap_001")
            .unwrap()
            .from_facts
            .len(),
        3
    );
}

#[test]
fn test_research_memory_across_sessions() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap().to_string();
    {
        let mut bb = SqlBlackboard::open(&path).unwrap();
        for i in 0..4 {
            bb.submit_fact(&Fact {
                id: FihHash(format!("f_doc_{:03}", i)),
                origin: format!("doc{i}").into(),
                content: serde_json::json!({"desc": format!("doc {i}")}),
                creator: "sync-agent".into(),
            })
            .unwrap();
        }
        assert_eq!(bb.read_state().facts.len(), 4);
    }
    {
        let mut bb = SqlBlackboard::open(&path).unwrap();
        bb.submit_intent(&Intent {
            id: FihHash("i_link_001".into()),
            from_facts: vec!["f_doc_000".into(), "f_doc_001".into()],
            description: "Link".into(),
            creator: "linker".into(),
            worker: Some("linker".into()),
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        })
        .unwrap();
        bb.submit_intent(&Intent {
            id: FihHash("i_link_002".into()),
            from_facts: vec!["f_doc_002".into(), "f_doc_003".into()],
            description: "Link 2".into(),
            creator: "linker".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        })
        .unwrap();
        assert_eq!(bb.read_state().intents.len(), 2);
    }
    {
        let mut bb = SqlBlackboard::open(&path).unwrap();
        bb.heartbeat("i_link_001", "linker").unwrap();
        bb.conclude_intent("i_link_001", &serde_json::json!({"finding": "confirmed"}))
            .unwrap();
        let state = bb.read_state();
        assert_eq!(state.facts.len(), 5);
        assert!(
            state
                .intents
                .iter()
                .find(|i| i.id.0 == "i_link_001")
                .unwrap()
                .concluded_at
                .is_some()
        );
        assert!(
            state
                .intents
                .iter()
                .find(|i| i.id.0 == "i_link_002")
                .unwrap()
                .concluded_at
                .is_none()
        );
    }
}

#[test]
fn test_multi_project_isolation() {
    let mut bb_a = SqlBlackboard::memory_with_project("proj_a").unwrap();
    let mut bb_b = SqlBlackboard::memory_with_project("proj_b").unwrap();
    bb_a.submit_fact(&make_fact("f001", "project A data"))
        .unwrap();
    bb_b.submit_fact(&make_fact("f001", "project B data"))
        .unwrap();
    assert_eq!(bb_a.read_state().facts[0].content, "project A data");
    assert_eq!(bb_b.read_state().facts[0].content, "project B data");
    bb_a.submit_intent(&make_intent("i001", vec!["f001"], "A's intent"))
        .unwrap();
    assert_eq!(bb_a.read_state().intents.len(), 1);
    assert_eq!(bb_b.read_state().intents.len(), 0);
}

#[test]
fn test_project_lifecycle() {
    let bb = SqlBlackboard::memory_with_project("lifecycle_test").unwrap();
    assert_eq!(bb.get_project().unwrap().status, "active");
    bb.set_project_status("stopped").unwrap();
    assert_eq!(bb.get_project().unwrap().status, "stopped");
    bb.set_project_status("completed").unwrap();
    assert_eq!(bb.get_project().unwrap().status, "completed");
}

#[test]
fn test_project_with_custom_title() {
    let bb = SqlBlackboard::memory_with_project("research_001").unwrap();
    let proj = bb.get_project().unwrap();
    assert_eq!(proj.id, "research_001");
    assert_eq!(proj.status, "active");
    assert!(proj.created_at.len() > 10);
}

#[test]
fn test_sqlite_storage_backward_compat() {
    let store = SqliteStorage::memory().unwrap();
    store.log_fih("test_event", "{\"key\": \"value\"}");
    let events = store.load_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "test_event");
}
