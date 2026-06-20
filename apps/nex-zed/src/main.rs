// ── nex-zed: Helix remote_server Unix socket client ──────────────────
//
// Connects to Helix remote_server via Unix socket and provides
// an interactive prompt for coding.
//
// Helix remote_server uses Zed's internal protocol over Unix socket.
// nex-zed acts as a client, sending requests and receiving responses.
//
// Usage:
//   nex-zed --stdin-sock /tmp/hl-stdin.sock --stdout-sock /tmp/hl-stdout.sock
//   nex-zed --bin /path/to/remote-server  (spawn + infer socket paths)

use clap::Parser;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "nex-zed", version, about = "Helix headless Zed client")]
struct Args {
    /// Path to helix-remote-server binary (spawn if provided)
    #[arg(long)]
    bin: Option<PathBuf>,

    /// Path to remote_server stdin socket
    #[arg(long, default_value = "/tmp/hl-stdin.sock")]
    stdin_sock: String,

    /// Path to remote_server stdout socket
    #[arg(long, default_value = "/tmp/hl-stdout.sock")]
    stdout_sock: String,

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

    // Spawn remote_server if --bin provided
    if let Some(ref bin_path) = args.bin {
        info!("Spawning remote_server: {:?}", bin_path);
        Command::new(bin_path)
            .arg("run")
            .arg("--log-file").arg("/tmp/hl.log")
            .arg("--pid-file").arg("/tmp/hl.pid")
            .arg("--stdin-socket").arg(&args.stdin_sock)
            .arg("--stdout-socket").arg(&args.stdout_sock)
            .arg("--stderr-socket").arg("/tmp/hl-stderr.sock")
            .current_dir(&args.workdir)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to spawn remote_server");
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // Connect to remote_server's Unix sockets
    info!("Connecting to Unix sockets...");
    info!("  stdin:  {}", args.stdin_sock);
    info!("  stdout: {}", args.stdout_sock);

    let mut stdin_stream = UnixStream::connect(&args.stdin_sock).await
        .unwrap_or_else(|e| { error!("Cannot connect to stdin socket: {}", e); std::process::exit(1); });
    let mut stdout_stream = UnixStream::connect(&args.stdout_sock).await
        .unwrap_or_else(|e| { error!("Cannot connect to stdout socket: {}", e); std::process::exit(1); });

    info!("Connected to Remote_server. Type your prompt and press Enter.");
    info!("  Type /exit to quit.");

    // Forward stdin → remote_server stdin socket
    let mut stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    // Read from stdout socket → print to our stdout
    let mut stdout_reader = BufReader::new(stdout_stream);
    let mut out_buf = String::new();

    let recv_task = tokio::spawn(async move {
        loop {
            out_buf.clear();
            match stdout_reader.read_line(&mut out_buf).await {
                Ok(0) => break,
                Ok(_) => {
                    print!("{}", out_buf);
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

    // Main loop: read stdin → write to remote_server stdin socket
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let input = line.trim();
                if input.is_empty() { continue; }
                if input == "/exit" || input == "/quit" { break; }

                if let Err(e) = stdin_stream.write_all(line.as_bytes()).await {
                    error!("Write error: {}", e);
                    break;
                }
                if let Err(e) = stdin_stream.flush().await {
                    error!("Flush error: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("Stdin error: {}", e);
                break;
            }
        }
    }

    recv_task.abort();
    info!("nex-zed shutting down");
}
