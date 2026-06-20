// ── nex-zed: Helix remote_server HTTP API client ─────────────────────
//
// Connects to Helix remote_server (via external_websocket_sync) HTTP API.
// Sends prompts, receives responses — same as Zed agent panel chat.
//
// API endpoints:
//   POST /api/v1/contexts              — create conversation
//   POST /api/v1/contexts/:id/messages  — send message
//   GET  /api/v1/contexts/:id/messages  — get messages
//
// Usage:
//   nex-zed --api http://localhost:3030
//   nex-zed --bin .bin/helix-remote-server-arm64  (spawn + connect)

use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, error, info, warn};

#[derive(Parser)]
#[command(name = "nex-zed", version, about = "Helix headless Zed client")]
struct Args {
    #[arg(long)]
    bin: Option<PathBuf>,
    #[arg(long, default_value = "http://localhost:3030")]
    api: String,
    #[arg(long, default_value = ".")]
    workdir: String,
}

#[derive(Serialize)]
struct CreateContextRequest {
    title: Option<String>,
}

#[derive(Deserialize)]
struct CreateContextResponse {
    context_id: String,
}

#[derive(Serialize)]
struct SendMessageRequest {
    content: String,
    role: String,
}

#[derive(Deserialize)]
struct MessageInfo {
    id: u64,
    content: String,
    role: String,
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
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let api = args.api.trim_end_matches('/').to_string();

    // 1. Create a conversation context
    info!("Creating conversation...");
    let ctx: CreateContextResponse = client
        .post(format!("{}/api/v1/contexts", api))
        .json(&CreateContextRequest { title: Some("nex-zed chat".into()) })
        .send()
        .await?
        .json()
        .await?;

    let ctx_id = ctx.context_id;
    info!("Context created: {}", ctx_id);
    info!("Type your prompt and press Enter. /exit to quit.");

    // 2. Stdin loop
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    // Polling task for incoming messages
    let poll_api = api.clone();
    let poll_ctx = ctx_id.clone();
    let poll_handle = tokio::spawn(async move {
        let poll_client = Client::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            match poll_client
                .get(format!("{}/api/v1/contexts/{}/messages", poll_api, poll_ctx))
                .send()
                .await
            {
                Ok(resp) => {
                    if let Ok(msgs) = resp.json::<Vec<MessageInfo>>().await {
                        for msg in &msgs {
                            if msg.role == "assistant" {
                                println!("[{}]: {}", msg.role, msg.content);
                            }
                        }
                    }
                }
                Err(e) => debug!("Poll error: {}", e),
            }
        }
    });

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let input = line.trim().to_string();
                if input.is_empty() { continue; }
                if input == "/exit" || input == "/quit" { break; }

                // Send message
                match client
                    .post(format!("{}/api/v1/contexts/{}/messages", api, ctx_id))
                    .json(&SendMessageRequest { content: input, role: "user".into() })
                    .send()
                    .await
                {
                    Ok(_) => info!("Message sent"),
                    Err(e) => error!("Send error: {}", e),
                }
            }
            Err(e) => {
                error!("Stdin error: {}", e);
                break;
            }
        }
    }

    poll_handle.abort();
    info!("Shutting down");
    Ok(())
}
