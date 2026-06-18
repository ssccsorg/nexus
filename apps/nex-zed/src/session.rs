use std::collections::HashMap;

/// A single ACP session mapped to a neXus scope.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Corresponding neXus scope identifier (empty in Phase 1).
    pub nexus_scope_id: String,
    /// Unix timestamp when the session was created.
    pub created_at: u64,
    /// Current session mode, if set via SetSessionModeRequest.
    pub mode: Option<String>,
    /// Current session model, if set via SetSessionModelRequest.
    pub model: Option<String>,
    /// Arbitrary config options set via SetSessionConfigOptionRequest.
    pub config_options: HashMap<String, String>,
}

impl SessionState {
    pub fn new(nexus_scope_id: String) -> Self {
        Self {
            nexus_scope_id,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            mode: None,
            model: None,
            config_options: HashMap::new(),
        }
    }
}

/// Manages active ACP sessions and their mapping to neXus scopes.
#[derive(Debug)]
pub struct SessionManager {
    sessions: HashMap<String, SessionState>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Create a new session and return its ID.
    pub fn create_session(&mut self, session_id: String) -> &SessionState {
        // Phase 1: empty scope ID; Phase 2+: created from neXus.
        self.sessions
            .entry(session_id)
            .or_insert_with(|| SessionState::new(String::new()))
    }

    /// Get session state by ID.
    pub fn get(&self, session_id: &str) -> Option<&SessionState> {
        self.sessions.get(session_id)
    }

    /// Get mutable session state by ID.
    pub fn get_mut(&mut self, session_id: &str) -> Option<&mut SessionState> {
        self.sessions.get_mut(session_id)
    }

    /// Get the neXus scope ID for a session.
    pub fn get_scope(&self, session_id: &str) -> Option<&str> {
        self.sessions
            .get(session_id)
            .map(|s| s.nexus_scope_id.as_str())
    }

    /// Remove a session.
    pub fn remove(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether any sessions exist.
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
