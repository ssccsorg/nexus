/// Storage backend trait for GraphBlackboard persistence.
///
/// Every FIH mutation is logged through this interface.
/// Different backends can be plugged in without changing core logic.

/// A single persisted FIH event.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub event_type: String,
    pub payload: String,
}

/// Storage abstraction for FIH event persistence.
/// Implementations must be thread-safe (Send + Sync).
pub trait Storage: Send + Sync {
    /// Persist one FIH operation. Called on every FIH mutation.
    fn log_fih(&self, event_type: &str, payload: &str);

    /// Load all past FIH operations in order. Called once on startup.
    fn load_events(&self) -> Vec<StoredEvent>;
}

// ── Null storage (no persistence) ─────────────────────────────────────────

/// No-op storage. All FIH operations are discarded.
pub struct NullStorage;

impl Storage for NullStorage {
    fn log_fih(&self, _event_type: &str, _payload: &str) {}
    fn load_events(&self) -> Vec<StoredEvent> { Vec::new() }
}
