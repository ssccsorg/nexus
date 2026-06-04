use nex::DefaultBlackboard;
use nex::MockGateway;
use nexus_model::{Blackboard, Fact, FihHash};

#[test]
fn test_mock_gateway_submit_fact() {
    let gw = MockGateway::new(DefaultBlackboard::new());
    let fact = Fact {
        id: FihHash("f_mock_001".into()),
        origin: "mock-test".into(),
        content: "Mock gateway test".into(),
        creator: "tester".into(),
    };
    let hash = gw.submit_fact(&fact).unwrap();
    assert_eq!(hash.0, "f_mock_001");

    let state = gw.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content, "Mock gateway test");
}
