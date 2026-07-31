use nex::FihBlackboard;
use nex_fih::{CoordId, Fact, FactCapable, StorageRead};
use nexus_gateway_serde_proxy::SerdeProxy;
use nexus_storage_sim::SimIo;

#[test]
fn test_serde_proxy_submit_fact() {
    let gw = SerdeProxy::new(FihBlackboard::new(SimIo::new(), "test"));
    let fact = Fact::with_id(
        CoordId::from_string("f_gw_001"),
        "gateway-test".into(),
        "Gateway driver test".into(),
        "tester".into(),
    );
    let hash = gw.submit_fact(&fact).unwrap();
    assert_eq!(hash, CoordId::from_string("f_gw_001"));

    let state = gw.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content, "Gateway driver test");
}
