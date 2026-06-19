// ── nex-zed: Zed-based coding agent, neXus-ified ──────────────────────
//
// Spawns Zed CLI (with embedded remote_server) over ACP stdio and bridges
// its I/O to the neXus FIH blackboard.
//
// Responsibilities:
//   - Run Zed CLI as an ACP agent subprocess
//   - Forward ACP JSON-RPC (stdin/stdout) to neXus FIH
//
// Orchestration / agent factory role is reserved for nex-queen (future).
//
// Flow:
//   Zed CLI ──ACP stdio──→ nex-zed ──FIH──→ neXus Blackboard ←→ nex-cf

use clap::Parser;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "nex-zed", version, about = "Zed coding agent → neXus")]
struct Args {
    /// Path to Zed CLI binary (default: search PATH and common locations)
    #[arg(long)]
    zed: Option<PathBuf>,

    /// Path to neXus FIH daemon socket
    #[arg(long, default_value = "/var/run/nexus.sock")]
    fih_socket: String,
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

    let zed = args.zed.clone().unwrap_or_else(|| {
        ["/Applications/Zed.app/Contents/MacOS/cli", "/Applications/Zed.app/Contents/MacOS/zed", "zed"]
            .iter().map(PathBuf::from).find(|p| p.is_file() || in_path(p)).unwrap_or(PathBuf::from("zed"))
    });

    if !zed.is_file() && !in_path(&zed) {
        error!("Zed CLI not found. Install from https://zed.dev");
        std::process::exit(1);
    }

    info!("nex-zed starting — zed={} fih={}", zed.display(), args.fih_socket);

    loop {
        let mut cmd = Command::new(&zed);
        cmd.arg("--foreground")
           .env("NEXUS_FIH_SOCKET", &args.fih_socket)
           .stdin(Stdio::piped())
           .stdout(Stdio::piped())
           .stderr(Stdio::inherit());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => { error!("spawn: {e}"); break; }
        };
        info!("Zed started (PID {})", child.id().unwrap_or(0));

        let mut zed_stdin = child.stdin.take().unwrap();
        let zed_stdout = child.stdout.take().unwrap();

        let stdin_fwd = tokio::spawn(async move {
            let mut r = BufReader::new(tokio::io::stdin());
            let mut b = String::new();
            loop {
                b.clear();
                match r.read_line(&mut b).await {
                    Ok(0) => { let _ = zed_stdin.shutdown().await; break; }
                    Ok(_) => { if zed_stdin.write_all(b.as_bytes()).await.is_err() { break; } let _ = zed_stdin.flush().await; }
                    Err(_) => break,
                }
            }
        });

        let stdout_fwd = tokio::spawn(async move {
            let mut r = BufReader::new(zed_stdout);
            let mut b = String::new();
            loop {
                b.clear();
                match r.read_line(&mut b).await {
                    Ok(0) => break,
                    Ok(_) => { print!("{b}"); use std::io::Write; let _ = std::io::stdout().flush(); }
                    Err(_) => break,
                }
            }
        });

        tokio::select! {
            _ = stdin_fwd => {}
            _ = stdout_fwd => {}
            _ = tokio::signal::ctrl_c() => {}
        }

        let status = child.wait().await.ok();
        info!("Zed exited (code {})", status.and_then(|s| s.code()).unwrap_or(-1));
    }
}

fn in_path(p: &PathBuf) -> bool {
    std::env::var_os("PATH").map_or(false, |paths| std::env::split_paths(&paths).any(|d| d.join(p).is_file()))
}
