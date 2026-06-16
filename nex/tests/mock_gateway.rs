use nexus_gateway_serde_proxy::SerdeProxy;
use nexus_model::{Fact, FactCapable, FihHash, StorageRead};
use nexus_storage_composite::HybridBlackboard;

#[test]
fn test_serde_proxy_submit_fact() {
    let gw = SerdeProxy::new(HybridBlackboard::new());
    let fact = Fact {
        id: FihHash::from_hex("f_gw_001"),
        origin: "gateway-test".into(),
        content: "Gateway driver test".into(),
        creator: "tester".into(),
    };
    let hash = gw.submit_fact(&fact).unwrap();
    assert_eq!(hash, FihHash::from_hex("f_gw_001"));

    let state = gw.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content, "Gateway driver test");
}
