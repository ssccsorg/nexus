use nex::DefaultBlackboard;
use nex::gateway_driver::GatewayDriver;
use nexus_model::{Fact, FactCapable, FihHash, StorageRead};

#[test]
fn test_gateway_driver_submit_fact() {
    let gw = GatewayDriver::new(DefaultBlackboard::new());
    let fact = Fact {
        id: FihHash("f_gw_001".into()),
        origin: "gateway-test".into(),
        content: "Gateway driver test".into(),
        creator: "tester".into(),
    };
    let hash = gw.submit_fact(&fact).unwrap();
    assert_eq!(hash.0, "f_gw_001");

    let state = gw.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content, "Gateway driver test");
}
