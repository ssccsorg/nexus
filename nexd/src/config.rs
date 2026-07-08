// ── NexdConfig — daemon configuration ──────────────────────────────────
//
// Configuration for the nexd daemon. Controls socket path, scheduling
// intervals, and initial agent command.

use std::path::PathBuf;

/// Daemon-level configuration loaded from CLI args or environment.
#[derive(Clone, Debug)]
pub struct NexdConfig {
    /// Unix domain socket path for IPC.
    pub socket_path: PathBuf,
    /// Interval between scheduler ticks (milliseconds).
    pub tick_interval_ms: u64,
    /// Heartbeat TTL in seconds. Intents not heartbeated within this window
    /// are released automatically.
    pub heartbeat_ttl_secs: u64,
    /// Maximum age for unclaimed, unconcluded intents before eviction.
    pub unclaimed_intent_ttl_secs: u64,
    /// Optional agent command to spawn at startup (e.g., "actus").
    pub agent_command: Option<String>,
    /// Arguments passed to the agent command.
    pub agent_args: Vec<String>,
    /// Path to nex-server binary (spawned as child process).
    pub nex_server_path: String,
    /// Unix socket path for nex-server IPC.
    pub nex_server_socket: String,
}

impl Default for NexdConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from("/tmp/nexd.sock"),
            tick_interval_ms: 100,
            heartbeat_ttl_secs: 60,
            unclaimed_intent_ttl_secs: 3600,
            agent_command: None,
            agent_args: Vec::new(),
            nex_server_path: "nex-server".into(),
            nex_server_socket: "/tmp/nex-server.sock".into(),
        }
    }
}

impl NexdConfig {
    /// Parse configuration from environment variables and CLI args.
    pub fn parse() -> Self {
        let mut config = NexdConfig::default();

        if let Ok(path) = std::env::var("NEXD_SOCKET_PATH") {
            config.socket_path = PathBuf::from(path);
        }
        if let Ok(ms) = std::env::var("NEXD_TICK_INTERVAL_MS")
            && let Ok(v) = ms.parse()
        {
            config.tick_interval_ms = v;
        }
        if let Ok(secs) = std::env::var("NEXD_HEARTBEAT_TTL_SECS")
            && let Ok(v) = secs.parse()
        {
            config.heartbeat_ttl_secs = v;
        }
        if let Ok(secs) = std::env::var("NEXD_UNCLAIMED_INTENT_TTL_SECS")
            && let Ok(v) = secs.parse()
        {
            config.unclaimed_intent_ttl_secs = v;
        }

        if let Ok(path) = std::env::var("NEXD_NEX_SERVER_PATH") {
            config.nex_server_path = path;
        }
        if let Ok(path) = std::env::var("NEX_SOCKET_PATH") {
            config.nex_server_socket = path;
        }

        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 {
            config.agent_command = Some(args[1].clone());
            if args.len() > 2 {
                config.agent_args = args[2..].to_vec();
            }
        }

        config
    }
}
