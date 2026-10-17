// The lifecycle configuration and status the contract layer exposes.
//
// A nex instance is started from a `NexConfig`, and its health is reported as one of
// three states. Both are small value types, and both are rendered or extended by the
// caller, so the builders and the display are pinned here.

use fih_model::contract::core::{HealthStatus, NexConfig};

#[test]
fn a_nex_config_is_built_and_extended() {
    let config = NexConfig::new("project", "/tmp/base");
    assert_eq!(config.project_id, "project");
    assert_eq!(config.base_path, "/tmp/base");
    assert!(!config.enable_contract);
    assert!(config.extra.is_empty());

    let config = config.with_contract(true).with_extra("key", "value");
    assert!(config.enable_contract);
    assert_eq!(config.extra, vec![("key".to_string(), "value".to_string())]);
}

#[test]
fn health_renders_each_state() {
    assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
    assert_eq!(
        HealthStatus::Degraded {
            reason: "lag".into()
        }
        .to_string(),
        "degraded: lag"
    );
    assert_eq!(
        HealthStatus::Unhealthy {
            reason: "down".into()
        }
        .to_string(),
        "unhealthy: down"
    );
}
