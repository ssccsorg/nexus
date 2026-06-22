use clap::Parser;

/// neXus instance embedding ACP as one of its communication surfaces for the Zed editor.
///
/// Reads ACP JSON-RPC 2.0 from stdin, translates messages to FIH blocks
/// (Intent, Fact, Hint), and returns responses via stdout.
#[derive(Parser, Debug, Clone)]
#[command(name = "nex-zed", version, about)]
pub struct Args {
    /// Path to neXus daemon Unix socket (unused in Phase 1).
    #[arg(long, default_value = "/var/run/nexus.sock")]
    pub nexus_socket: String,

    /// Enable verbose logging.
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Log level filter (overrides -v).
    #[arg(long, default_value = "info")]
    pub log_level: String,
}
