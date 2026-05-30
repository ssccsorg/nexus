// Multi-transport scenario tests.
//
// Each scenario simulates a different real-world transport and agent type,
// all communicating through the FIH protocol via MockGateway's JSON boundary.

use nexus_graph::mock_gateway::MockGateway;
use nexus_graph::{Blackboard, BlackboardError, Fact, FihHash, Intent, create_blackboard};

// ── Scenario: Intermittent agent (Bluetooth / short-range radio) ─────────
//
// A field sensor agent connects periodically, submits observations, then
// goes offline. Later it reconnects, reads the current state, and continues
// working. This simulates Bluetooth, LoRa, or other intermittent transports
// where connections are brief and unreliable.

#[test]
fn scenario_intermittent_sensor_agent() {
    let mut bb = create_blackboard();

    // Session 1: agent connects, submits a fact, disconnects
    {
        let mut gw = MockGateway::new(&mut bb);
        gw.submit_fact(&Fact {
            id: FihHash("f_temp_001".into()),
            origin: "sensor-alpha".into(),
            content: serde_json::json!({
                "type": "temperature",
                "value": 42.5,
                "unit": "C",
                "sector": 7
            }).into(),
            creator: "drone-a".into(),
        })
        .unwrap();
        // gw dropped — borrow released
    }

    // Session 2: same agent reconnects after offline period
    {
        let mut gw = MockGateway::new(&mut bb);

        let state = gw.read_state();
        assert_eq!(state.facts.len(), 1, "fact persisted across sessions");
        assert_eq!(state.facts[0].origin, "sensor-alpha");

        gw.submit_fact(&Fact {
            id: FihHash("f_temp_002".into()),
            origin: "sensor-alpha".into(),
            content: serde_json::json!({
                "type": "temperature",
                "value": 43.1,
                "unit": "C",
                "sector": 7
            }).into(),
            creator: "drone-a".into(),
        })
        .unwrap();

        let state = gw.read_state();
        assert_eq!(state.facts.len(), 2, "second fact visible");
    }

    println!("  ✓ Intermittent sensor: 2 sessions, JSON state preserved across disconnects");
}

// ── Scenario: High-latency agent (Satellite / deep-space link) ──────────
//
// A satellite agent has limited uplink windows. It queues observations and
// submits them in bursts. Heartbeats arrive late but within tolerance.
// Simulates Starlink, Iridium, or deep-space network characteristics.

#[test]
fn scenario_satellite_burst_agent() {
    let mut gw = MockGateway::new(create_blackboard());

    let readings = [
        (
            "f_sat_001",
            "band-x",
            serde_json::json!({"freq": 12.4, "snr": 8.2}),
        ),
        (
            "f_sat_002",
            "band-x",
            serde_json::json!({"freq": 12.5, "snr": 7.9}),
        ),
        (
            "f_sat_003",
            "band-ku",
            serde_json::json!({"freq": 14.1, "snr": 6.5}),
        ),
    ];
    for (id, origin, content) in &readings {
        gw.submit_fact(&Fact {
            id: FihHash(id.to_string()),
            origin: origin.to_string(),
            content: content.clone().into(),
            creator: "sat-1".into(),
        })
        .unwrap();
    }

    let state = gw.read_state();
    assert_eq!(state.facts.len(), 3, "burst of 3 facts received");

    gw.submit_intent(&Intent {
        id: FihHash("i_sat_analysis".into()),
        from_facts: vec!["f_sat_001".into(), "f_sat_002".into()],
        description: "Analyze band-x SNR degradation trend".into(),
        creator: "ground-station".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    })
    .unwrap();

    gw.claim_intent("i_sat_analysis", "sat-1").unwrap();
    gw.heartbeat("i_sat_analysis", "sat-1").unwrap();

    gw.conclude_intent(
        "i_sat_analysis",
        &"Band-x SNR dropped 0.3dB between samples: atmospheric interference hypothesis.".into(),
    )
    .unwrap();

    let state = gw.read_state();
    assert_eq!(state.facts.len(), 4, "3 original + 1 concluded");
    assert_eq!(state.intents.len(), 1, "1 original intent");

    println!("  ✓ Satellite burst: 3 facts in one pass, full intent lifecycle via JSON");
}

// ── Scenario: Web browser agent (JavaScript / REST) ─────────────────────
//
// A browser-based UI agent manages multiple intents through HTTP-like
// interactions. This simulates the Cairn web UI pattern where a human
// reviews agent output through a browser.

#[test]
fn scenario_browser_agent() {
    let mut gw = MockGateway::new(create_blackboard());

    gw.submit_fact(&Fact {
        id: FihHash("f_background".into()),
        origin: "system".into(),
        content: serde_json::Value::String(
            "Server load exceeds 85% for 3 consecutive hours".into(),
        ).into(),
        creator: "monitor".into(),
    })
    .unwrap();

    gw.submit_intent(&Intent {
        id: FihHash("i_investigate".into()),
        from_facts: vec!["f_background".into()],
        description: "Find root cause of sustained high server load".into(),
        creator: "human-operator".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    })
    .unwrap();

    let state = gw.read_state();
    assert_eq!(state.intents.len(), 1, "intent visible in browser poll");

    gw.claim_intent("i_investigate", "analysis-agent").unwrap();
    gw.conclude_intent(
        "i_investigate",
        &"Root cause: memory leak in cache layer (redis eviction storm). Mitigation: increase maxmemory by 2GB, patch due next sprint.".into(),
    )
    .unwrap();

    let state = gw.read_state();
    assert_eq!(state.facts.len(), 2, "background + conclusion fact");
    assert!(
        state.facts[1]
            .content
            .as_str()
            .unwrap_or("")
            .contains("redis eviction"),
        "browser sees root cause"
    );

    println!("  ✓ Browser agent: AJAX-style polling, human-in-the-loop via JSON");
}

// ── Scenario: Multi-language agents (heterogeneous clients) ─────────────
//
// Three agents built in different languages (simulated by separate
// MockGateway instances sharing the same DefaultBlackboard) communicate
// through JSON. Each gateway drops its borrow before the next acquires it.

#[test]
fn scenario_multi_language_agents() {
    let mut bb = create_blackboard();

    // Python agent submits a fact
    {
        let mut gw = MockGateway::new(&mut bb);
        gw.submit_fact(&Fact {
            id: FihHash("f_py_001".into()),
            origin: "python-etl".into(),
            content: serde_json::Value::String("Data pipeline processed 15K records".into()).into(),
            creator: "py-agent".into(),
        })
        .unwrap();
    }

    // Rust agent submits a fact
    {
        let mut gw = MockGateway::new(&mut bb);
        gw.submit_fact(&Fact {
            id: FihHash("f_rs_001".into()),
            origin: "rust-analyzer".into(),
            content: serde_json::json!({
                "module": "inference",
                "latency_p50_ms": 42,
                "latency_p99_ms": 187
            }).into(),
            creator: "rs-agent".into(),
        })
        .unwrap();
    }

    // TypeScript agent reads both and submits an intent
    {
        let mut gw = MockGateway::new(&mut bb);

        let state = gw.read_state();
        assert_eq!(
            state.facts.len(),
            2,
            "all agents' facts visible to TS agent"
        );

        gw.submit_intent(&Intent {
            id: FihHash("i_cross_lang".into()),
            from_facts: vec!["f_py_001".into(), "f_rs_001".into()],
            description: "Correlate pipeline throughput with inference latency".into(),
            creator: "ts-agent".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        })
        .unwrap();
    }

    // Rust agent claims and concludes
    {
        let mut gw = MockGateway::new(&mut bb);
        gw.claim_intent("i_cross_lang", "rs-agent").unwrap();
        gw
            .conclude_intent(
                "i_cross_lang",
                &"Pipeline throughput (15K records) correlates with p99 latency (187ms). Bottleneck: data serialization in Python stage.".into(),
            )
            .unwrap();
    }

    // Python agent sees the result
    {
        let gw = MockGateway::new(&mut bb);
        let state = gw.read_state();
        assert_eq!(state.facts.len(), 3, "py agent sees concluded fact");
        assert_eq!(state.intents.len(), 1, "py agent sees original intent only");
    }

    println!(
        "  ✓ Multi-language: 3 gateway instances (py/rs/ts), shared Blackboard, JSON protocol"
    );
}

// ── Scenario: Conflicting claims (race condition) ───────────────────────
//
// Two agents attempt to claim the same intent simultaneously.
// First claim succeeds, second is rejected. Validates that
// the Blackboard's conflict detection works across transport boundary.

#[test]
fn scenario_conflicting_claims() {
    let mut gw = MockGateway::new(create_blackboard());

    gw.submit_fact(&Fact {
        id: FihHash("f_conflict".into()),
        origin: "test".into(),
        content: serde_json::Value::String("Conflict test ground truth".into()).into(),
        creator: "system".into(),
    })
    .unwrap();

    gw.submit_intent(&Intent {
        id: FihHash("i_conflict".into()),
        from_facts: vec!["f_conflict".into()],
        description: "Intent that two agents will race to claim".into(),
        creator: "system".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    })
    .unwrap();

    // Agent-1 claims first (should succeed)
    gw.claim_intent("i_conflict", "agent-1").unwrap();

    // Agent-2 tries to claim (should fail — conflict)
    let result = gw.claim_intent("i_conflict", "agent-2");
    assert!(result.is_err(), "second claim must fail");
    assert!(
        matches!(result, Err(BlackboardError::Conflict(_))),
        "error must be Conflict"
    );

    // Agent-1 heartbeats (still owns it)
    gw.heartbeat("i_conflict", "agent-1").unwrap();

    // Agent-2 heartbeat (should fail — not the owner)
    let hb_result = gw.heartbeat("i_conflict", "agent-2");
    assert!(hb_result.is_err(), "non-owner heartbeat must fail");

    // Agent-1 concludes successfully
    gw.conclude_intent("i_conflict", &"Agent-1 resolved the conflict".into())
        .unwrap();

    println!("  ✓ Conflicting claims: 2 agents race, exactly 1 wins, conflict detection via JSON");
}
