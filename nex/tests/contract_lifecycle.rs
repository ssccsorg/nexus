// ── NexLifecycle types tests ──────────────────────────────────────────

use nex::contract::{HealthStatus, NexConfig};

#[test]
fn test_nex_config_default() {
    let config = NexConfig::new("test-proj", "/tmp/nex");
    assert_eq!(config.project_id, "test-proj");
    assert_eq!(config.base_path, "/tmp/nex");
    assert!(!config.enable_contract);
    assert!(config.extra.is_empty());
}

#[test]
fn test_nex_config_builder() {
    let config = NexConfig::new("p", "/b")
        .with_contract(true)
        .with_extra("key1", "val1");
    assert!(config.enable_contract);
    assert_eq!(config.extra.len(), 1);
    assert_eq!(config.extra[0], ("key1".to_string(), "val1".to_string()));
}

#[test]
fn test_health_status_display() {
    assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
    assert_eq!(
        HealthStatus::Degraded {
            reason: "slow".into()
        }
        .to_string(),
        "degraded: slow"
    );
    assert_eq!(
        HealthStatus::Unhealthy {
            reason: "panic".into()
        }
        .to_string(),
        "unhealthy: panic"
    );
}

#[test]
fn test_health_status_equality() {
    assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
    assert_ne!(
        HealthStatus::Healthy,
        HealthStatus::Degraded { reason: "x".into() }
    );
}

#[test]
fn test_nex_config_with_multiple_extras() {
    let config = NexConfig::new("multi", "/p")
        .with_extra("a", "1")
        .with_extra("b", "2")
        .with_extra("c", "3");
    assert_eq!(config.extra.len(), 3);
}
