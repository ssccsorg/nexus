// ── nex-zed: Helix remote_server WebSocket client ────────────────────
//
// Connects to Helix remote_server's WebSocket and provides
// an interactive chat interface for coding via ACP.
//
// Flow:
//   Helix remote_server (headless Zed)
//     └── WebSocket ws://localhost:9876 ──→ nex-zed (this binary)
//           ├── chat_message: send prompts
//           ├── message_added: receive streaming responses
//           └── message_completed: receive final results
//
// Usage:
//   nex-zed                              # connect to localhost:9876
//   nex-zed --ws ws://server:9876         # connect to remote
//   nex-zed --bin /path/to/remote-server  # spawn + connect

use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "nex-zed", version, about = "Helix headless Zed client")]
struct Args {
    /// Path to helix-remote-server binary (optional, spawn if provided)
    #[arg(long)]
    bin: Option<PathBuf>,

    /// Helix remote_server WebSocket URL
    #[arg(long, default_value = "ws://localhost:9876")]
    ws: String,

    /// Working directory (project root)
    #[arg(long, default_value = ".")]
    workdir: String,

    /// Auth token for WebSocket connection
    #[arg(long, default_value = "nex-zed-token")]
    auth_token: String,

    /// Agent name to use (e.g., "zed-agent", "qwen")
    #[arg(long, default_value = "zed-agent")]
    agent: String,
}

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "nex_zed=info"); }
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nex_zed=info".parse().unwrap()),
        )
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    // Spawn remote_server if --bin provided
    if let Some(ref bin_path) = args.bin {
        info!("Spawning Helix remote_server from {:?}", bin_path);
        let mut child = Command::new(bin_path)
            .arg("run")
            .arg("--log-file")
            .arg("/tmp/nex-zed-helix.log")
            .arg("--pid-file")
            .arg("/tmp/nex-zed-helix.pid")
            .arg("--stdin-socket")
            .arg("/tmp/nex-zed-stdin.sock")
            .arg("--stdout-socket")
            .arg("/tmp/nex-zed-stdout.sock")
            .arg("--stderr-socket")
            .arg("/tmp/nex-zed-stderr.sock")
            .current_dir(&args.workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn();

        match child {
            Ok(mut c) => {
                info!("Helix remote_server started (PID {})", c.id().unwrap_or(0));
                // Don't wait, let it run in background
                tokio::spawn(async move {
                    let status = c.wait().await;
                    info!(
                        "Helix remote_server exited ({:?})",
                        status.map(|s| s.code().unwrap_or(-1))
                    );
                });
            }
            Err(e) => {
                error!("Failed to spawn: {}", e);
                std::process::exit(1);
            }
        }

        // Wait for server to be ready
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }

    // Connect WebSocket
    let ws_url = format!("{}/api/v1/external-agents/sync?session_id={}", args.ws, Uuid::new_v4());
    info!("Connecting to WebSocket: {}", ws_url);

    let (ws_stream, _) = match connect_async(&ws_url).await {
        Ok(s) => s,
        Err(e) => {
            error!("WebSocket connection failed: {}", e);
            error!("Make sure Helix remote_server is running");
            std::process::exit(1);
        }
    };

    info!("WebSocket connected");

    let (mut write, mut read) = ws_stream.split();

    // Read user input from stdin and send as chat_message
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    // Spawn read task
    let read_handle = tokio::spawn(async move {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Print received messages
                    println!("{}", text);
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Main loop: read stdin, send WebSocket messages
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let input = line.trim().to_string();
                if input.is_empty() {
                    continue;
                }
                if input == "/exit" || input == "/quit" {
                    break;
                }

                // Send chat_message command to Helix remote_server
                let msg = serde_json::json!({
                    "type": "chat_message",
                    "data": {
                        "message": input,
                        "request_id": Uuid::new_v4().to_string(),
                        "acp_thread_id": null,
                        "agent_name": args.agent,
                    }
                });

                if let Err(e) = write.send(Message::Text(msg.to_string().into())).await {
                    error!("Send failed: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("Stdin error: {}", e);
                break;
            }
        }
    }

    read_handle.abort();
    info!("nex-zed shutting down");
}
