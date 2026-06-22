// ── nex-zed: Helix remote_server simple stdin/stdout client ──────────
//
// Connects to Helix remote_server's Unix sockets.
// Reads from stdin, writes to remote stdin socket.
// Reads from remote stdout socket, writes to stdout.
//
// This is a raw I/O bridge. No protobuf, no ACP, no HTTP.
// Just pipe bytes between user and remote_server.
//
// Usage:
//   nex-zed                                 # default socket paths
//   nex-zed --bin .bin/helix-remote-server  # spawn + connect
//
// Note: remote_server expects protobuf messages. This raw bridge
// will initially show garbage responses. Next step: implement
// protobuf message framing.

use clap::Parser;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "nex-zed", version, about = "Helix headless Zed client")]
struct Args {
    #[arg(long)]
    bin: Option<PathBuf>,
    #[arg(long, default_value = "/tmp/hl-stdin.sock")]
    stdin_sock: String,
    #[arg(long, default_value = "/tmp/hl-stdout.sock")]
    stdout_sock: String,
    #[arg(long, default_value = ".")]
    workdir: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("RUST_LOG").is_err() {
        unsafe {
            std::env::set_var("RUST_LOG", "nex_zed=info");
        }
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

    if let Some(ref bin_path) = args.bin {
        info!("Spawning: {:?}", bin_path);
        Command::new(bin_path)
            .arg("run")
            .arg("--log-file")
            .arg("/tmp/hl.log")
            .arg("--pid-file")
            .arg("/tmp/hl.pid")
            .arg("--stdin-socket")
            .arg(&args.stdin_sock)
            .arg("--stdout-socket")
            .arg(&args.stdout_sock)
            .arg("--stderr-socket")
            .arg("/tmp/hl-stderr.sock")
            .current_dir(&args.workdir)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    info!("Connecting to stdin: {}", args.stdin_sock);
    info!("Connecting to stdout: {}", args.stdout_sock);

    let mut to_server = UnixStream::connect(&args.stdin_sock).await?;
    let from_server = UnixStream::connect(&args.stdout_sock).await?;

    info!("Connected. Type and press Enter. /exit to quit.");

    // stdout reader task
    let recv = tokio::spawn(async move {
        let mut reader = BufReader::new(from_server);
        let mut buf = String::new();
        loop {
            buf.clear();
            match reader.read_line(&mut buf).await {
                Ok(0) => break,
                Ok(_) => {
                    print!("{}", buf);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
                Err(e) => {
                    error!("Read error: {}", e);
                    break;
                }
            }
        }
    });

    // stdin reader + write to server
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut line = String::new();
    loop {
        line.clear();
        match stdin.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                if input == "/exit" || input == "/quit" {
                    break;
                }
                to_server.write_all(line.as_bytes()).await?;
                to_server.flush().await?;
            }
            Err(e) => {
                error!("stdin: {}", e);
                break;
            }
        }
    }

    recv.abort();
    info!("Shutdown");
    Ok(())
}
