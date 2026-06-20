// ── nex-zed: Helix remote_server HTTP API client ─────────────────────
//
// Connects to Helix remote_server's external_websocket_sync HTTP API
// to send coding prompts and receive results.
//
// remote_server provides all Zed native tools via HTTP.
// nex-zed translates stdin prompts to API calls.
//
// Usage:
//   nex-zed --api http://localhost:3030         # connect to API
//   nex-zed --bin .bin/helix-remote-server-arm64 # spawn + connect
//
// API endpoints (from external_websocket_sync):
//   POST /api/v1/contexts              - create conversation
//   POST /api/v1/contexts/:id/messages  - send message
//   GET  /api/v1/contexts/:id/messages  - get messages
//   GET  /api/v1/ws                     - WebSocket streaming

use clap::Parser;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "nex-zed", version, about = "Helix headless Zed client")]
struct Args {
    /// Spawn Helix remote_server binary
    #[arg(long)]
    bin: Option<PathBuf>,

    /// external_websocket_sync HTTP API base URL
    #[arg(long, default_value = "http://localhost:3030")]
    api: String,

    /// Working directory
    #[arg(long, default_value = ".")]
    workdir: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "nex_zed=info"); }
    }
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "nex_zed=info".parse().unwrap()))
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    // Spawn remote_server if --bin provided
    if let Some(ref bin_path) = args.bin {
        info!("Spawning: {:?}", bin_path);
        Command::new(bin_path)
            .arg("run")
            .arg("--log-file").arg("/tmp/hl.log")
            .arg("--pid-file").arg("/tmp/hl.pid")
            .arg("--stdin-socket").arg("/tmp/hl-stdin.sock")
            .arg("--stdout-socket").arg("/tmp/hl-stdout.sock")
            .arg("--stderr-socket").arg("/tmp/hl-stderr.sock")
            .current_dir(&args.workdir)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        info!("Remote server spawned");
    }

    info!("API endpoint: {}/api/v1", args.api);

    // TODO:
    // 1. POST /api/v1/contexts to create a conversation
    // 2. Loop: read stdin → POST /api/v1/contexts/:id/messages
    // 3. Poll GET /api/v1/contexts/:id/messages for responses
    // 4. WebSocket for streaming (/api/v1/ws)

    info!("Waiting for commands. Type /exit to quit.");
    info!("(API integration pending)");

    tokio::signal::ctrl_c().await?;
    info!("Shutting down");
    Ok(())
}
