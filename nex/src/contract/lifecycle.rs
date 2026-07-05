// ── NexLifecycle: abstract lifecycle for nex instances ─────────────────
//
// Defines the lifecycle contract for a nex blackboard instance.
// Two implementations are anticipated:
//   - InProcessNex: FihStorage in-process (for testing, single-binary)
//   - ChildProcessNex: nex-server as subprocess (for nexd decoupled mode)
//
// Both implementations compile on wasm32-unknown-unknown since the trait
// and its types use only std + async-trait. Subprocess implementation is
// cfg-gated to native platforms only.

use async_trait::async_trait;
use std::time::Duration;

// ── NexConfig ──────────────────────────────────────────────────────────

/// Configuration for a nex instance.
///
/// Minimal set of parameters required to start a nex instance.
/// Extensible with additional fields as needed.
#[derive(Debug, Clone)]
pub struct NexConfig {
    /// Unique project/instance identifier.
    pub project_id: String,
    /// Base path for IO storage (filesystem or memory key).
    pub base_path: String,
    /// Whether to enable the contract governance layer.
    pub enable_contract: bool,
    /// Optional custom configuration key-value pairs.
    pub extra: Vec<(String, String)>,
}

impl NexConfig {
    /// Create a new NexConfig with the given project ID and base path.
    pub fn new(project_id: &str, base_path: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            base_path: base_path.to_string(),
            enable_contract: false,
            extra: Vec::new(),
        }
    }

    /// Builder-style method to enable the contract layer.
    pub fn with_contract(mut self, enabled: bool) -> Self {
        self.enable_contract = enabled;
        self
    }

    /// Builder-style method to add an extra config value.
    pub fn with_extra(mut self, key: &str, value: &str) -> Self {
        self.extra.push((key.to_string(), value.to_string()));
        self
    }
}

// ── NexInstanceInfo ────────────────────────────────────────────────────

/// Read-only metadata about a running nex instance.
#[derive(Debug, Clone)]
pub struct NexInstanceInfo {
    /// Project/instance identifier.
    pub project_id: String,
    /// Instance uptime in seconds (approximate).
    pub uptime_secs: u64,
    /// Number of facts in storage.
    pub fact_count: usize,
    /// Number of intents in storage.
    pub intent_count: usize,
    /// Number of hints in storage.
    pub hint_count: usize,
    /// Whether the contract layer is active.
    pub contract_enabled: bool,
    /// Evidence chain tip hash, if contract is enabled.
    pub evidence_tip: Option<String>,
    /// Number of entries in the evidence chain.
    pub evidence_count: usize,
}

// ── HealthStatus ───────────────────────────────────────────────────────

/// Health status of a nex instance.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    /// Instance is running and healthy.
    Healthy,
    /// Instance is running but degraded.
    Degraded { reason: String },
    /// Instance is unhealthy.
    Unhealthy { reason: String },
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Degraded { reason } => write!(f, "degraded: {reason}"),
            Self::Unhealthy { reason } => write!(f, "unhealthy: {reason}"),
        }
    }
}

// ── NexLifecycle trait ─────────────────────────────────────────────────

/// Abstract lifecycle for a nex instance.
///
/// Provides a uniform interface over in-process and child-process
/// nex instances. All methods are async for compatibility with both
/// implementations.
///
/// The `start` associated function creates a new instance.
/// `dispatch` routes an RPC request to the blackboard.
/// `shutdown` stops the instance with a configurable timeout.
#[async_trait]
pub trait NexLifecycle: Send + Sync + Sized {
    /// Error type returned by lifecycle operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Start a new nex instance with the given configuration.
    async fn start(config: NexConfig) -> Result<Self, Self::Error>;

    /// Check the health of the instance.
    async fn health(&self) -> HealthStatus;

    /// Dispatch an RPC request to the blackboard.
    ///
    /// The request is a JSON-RPC-like string containing the method
    /// and parameters. The response is a JSON-RPC-like string.
    async fn dispatch(&self, request: &str) -> String;

    /// Gracefully shut down the instance.
    ///
    /// The instance will wait at most `timeout` for pending work to
    /// complete before forcing shutdown.
    async fn shutdown(self, timeout: Duration) -> Result<(), Self::Error>;

    /// Return metadata about the running instance.
    fn info(&self) -> NexInstanceInfo;
}
