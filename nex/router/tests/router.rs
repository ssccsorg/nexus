// The table refuses an ambiguous method, forgets a node that left, and answers by lookup.

use nex_router::Router;

fn methods(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn a_node_owns_the_methods_it_registers() {
    let mut router = Router::new();
    router
        .register("node-a", &methods(&["read_state", "write_fact"]))
        .expect("the first registration succeeds");
    assert_eq!(router.route("read_state"), Some("node-a"));
    assert_eq!(router.route("write_fact"), Some("node-a"));
    assert_eq!(router.route("conclude_intent"), None);
    assert_eq!(router.nodes(), vec!["node-a".to_string()]);
    assert_eq!(
        router.methods_of("node-a"),
        vec!["read_state".to_string(), "write_fact".to_string()]
    );
}

#[test]
fn a_second_node_cannot_claim_a_method_that_has_an_owner() {
    let mut router = Router::new();
    router
        .register("node-a", &methods(&["read_state"]))
        .expect("the first registration succeeds");
    let error = router
        .register("node-b", &methods(&["read_state"]))
        .expect_err("the method already has an owner");
    assert!(error.contains("node-a"), "{error}");
    assert_eq!(router.route("read_state"), Some("node-a"));
}

#[test]
fn a_node_that_reconnects_registers_again_without_error() {
    let mut router = Router::new();
    router
        .register("node-a", &methods(&["read_state"]))
        .expect("the first registration succeeds");
    router
        .register("node-a", &methods(&["read_state"]))
        .expect("its own re-registration is idempotent");
    assert_eq!(router.len(), 1);
}

#[test]
fn a_node_that_leaves_takes_its_methods_with_it() {
    let mut router = Router::new();
    router
        .register("node-a", &methods(&["read_state", "write_fact"]))
        .expect("registered");
    router
        .register("node-b", &methods(&["read_hint"]))
        .expect("registered");
    router.deregister("node-a");
    assert_eq!(router.route("read_state"), None);
    assert_eq!(router.route("read_hint"), Some("node-b"));
    assert_eq!(router.nodes(), vec!["node-b".to_string()]);
}
