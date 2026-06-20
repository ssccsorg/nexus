// ── nex-zed: Helix remote_server launcher ─────────────────────────────
//
// Downloads/executes a pre-compiled Helix remote_server binary and
// connects to it via WebSocket. No Helix/remote_server source code
// is built directly — just runs the official binary.
//
// Flow:
//   nex-zed ──spawn──→ helix-remote-server (pre-compiled binary)
//     │                    └── WebSocket :9876
//     └── connect WebSocket → send/receive JSON-RPC messages
//
// Usage:
//   nex-zed                              # auto-download + run
//   nex-zed --bin /path/to/remote-server  # use existing binary
//   nex-zed --ws ws://host:9876           # connect to remote

use clap::Parser;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "nex-zed", version, about = "Helix remote_server launcher")]
struct Args {
    /// Path to helix-remote-server binary (optional; auto-download if absent)
    #[arg(long)]
    bin: Option<PathBuf>,

    /// Helix remote_server WebSocket URL
    #[arg(long, default_value = "ws://localhost:9876")]
    ws: String,

    /// Working directory (project root)
    #[arg(long, default_value = ".")]
    workdir: String,
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "nex_zed=info"); }
    }
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "nex_zed=info".parse().unwrap()))
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    // 1. Find or prepare the Helix remote_server binary
    let bin_path = resolve_binary(args.bin.as_ref()).await;

    // 2. Spawn the Helix remote_server as a subprocess
    info!("Spawning Helix remote_server...");
    info!("  binary: {}", bin_path.display());
    info!("  workdir: {}", args.workdir);
    info!("  ws: {}", args.ws);

    let mut child = Command::new(&bin_path)
        .arg("run")
        .arg("--log-file").arg("/tmp/nex-zed-helix.log")
        .arg("--pid-file").arg("/tmp/nex-zed-helix.pid")
        .arg("--stdin-socket").arg("/tmp/nex-zed-stdin.sock")
        .arg("--stdout-socket").arg("/tmp/nex-zed-stdout.sock")
        .arg("--stderr-socket").arg("/tmp/nex-zed-stderr.sock")
        .current_dir(&args.workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to spawn Helix remote_server: {}", e);
            error!("Make sure the binary exists or is downloadable.");
            std::process::exit(1);
        }
    };

    info!("Helix remote_server started (PID {})", child.id().unwrap_or(0));

    // 3. TODO: Connect to WebSocket and bridge messages
    //    tokio-tungstenite connect to args.ws
    //    forward JSON-RPC messages to/from stdin/stdout
    //    FIH sync (later)

    // Wait for the remote_server process to exit
    let status = child.wait().await.expect("wait failed");
    info!("Helix remote_server exited (code {})", status.code().unwrap_or(-1));
}

async fn resolve_binary(custom: Option<&PathBuf>) -> PathBuf {
    if let Some(path) = custom {
        if path.is_file() {
            return path.clone();
        }
        warn!("Specified binary not found: {:?}", path);
    }

    // Check common locations
    let candidates = [
        // Helix-specific naming
        "helix-remote-server",
        "helix-remote-server-arm64",
        "helix-remote-server-x86_64",
        // Generic Zed remote_server
        "zed-remote-server",
        // Development builds
        "../helix/target/release/helix-remote-server",
        "../helix/target/debug/helix-remote-server",
        "../zed/target/release/zed-remote-server",
        "../zed/target/debug/zed-remote-server",
    ];

    for name in &candidates {
        let p = PathBuf::from(name);
        if p.is_file() {
            return p;
        }
    }

    // Not found — return a default path so the error message is clear
    PathBuf::from("helix-remote-server")
}
