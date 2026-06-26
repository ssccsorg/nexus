// nex-zed: Headless Zed AI agent — REST API server
//
// Launches Zed in --headless mode, connects via WebSocket,
// and exposes a REST API for multi-thread chat with async task queue.
//
// Usage:
//   nex-zed --workdir /path/to/project

mod server;
mod zed;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::server::AppState;
use crate::zed::ZedManager;

#[derive(clap::Parser, Debug, Clone)]
#[command(name = "nex-zed", version, about = "Headless Zed AI agent server")]
struct Args {
    /// Helix headless Zed binary path
    #[arg(long)]
    bin: Option<PathBuf>,

    /// Working directory for Zed
    #[arg(long, default_value = ".")]
    workdir: PathBuf,

    /// HTTP API port
    #[arg(long, default_value = "9090")]
    http_port: u16,

    /// WebSocket port for Zed to connect to
    #[arg(long, default_value = "8080")]
    ws_port: u16,

    /// DeepSeek API key (default: DEEPSEEK_API_KEY env var)
    #[arg(long)]
    api_key: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Init logging
    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "nex_zed=info"); }
    }
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()?)
        .init();

    let args: Args = clap::Parser::parse();

    // Resolve API key
    let api_key = args
        .api_key
        .or_else(|| std::env::var("DEEPSEEK_API_KEY").ok())
        .or_else(|| std::env::var("LLM_API_KEY").ok())
        .ok_or_else(|| anyhow::anyhow!("API key required: set DEEPSEEK_API_KEY or --api-key"))?;

    // Resolve binary path
    let bin_path = if let Some(p) = args.bin {
        p
    } else {
        // Search common locations
        let candidates = vec![
            dirs::home_dir()
                .map(|h| h.join(".bin/helix-zed-headless-arm64"))
                .unwrap_or_default(),
            PathBuf::from("../.bin/helix-zed-headless-arm64"),
            PathBuf::from(".bin/helix-zed-headless-arm64"),
        ];
        candidates
            .into_iter()
            .find(|p| p.exists())
            .ok_or_else(|| anyhow::anyhow!("helix-zed-headless binary not found"))?
    };

    let workdir = std::fs::canonicalize(&args.workdir)?;
    let ws_host = format!("127.0.0.1:{}", args.ws_port);

    // Read model name from env or default
    let model_name = std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());
    let model_display = std::env::var("LLM_MODEL_DISPLAY").unwrap_or_else(|_| model_name.clone());

    // Bootstrap Zed user data dir with DeepSeek settings
    let user_data_dir = tempfile::tempdir()?;
    zed::ensure_zed_settings(user_data_dir.path(), &api_key, &model_name, &model_display)?;

    let session_id = format!("ses_nex-zed-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap());

    tracing::info!("Starting nex-zed server");
    tracing::info!("  Binary:     {}", bin_path.display());
    tracing::info!("  Workdir:    {}", workdir.display());
    tracing::info!("  User data:  {}", user_data_dir.path().display());
    tracing::info!("  Session:    {}", session_id);
    tracing::info!("  HTTP API:   http://127.0.0.1:{}", args.http_port);
    tracing::info!("  WebSocket:  ws://{}", ws_host);

    // Start WebSocket server (Zed connects to us)
    let zed_manager = Arc::new(RwLock::new(ZedManager::new(session_id.clone(), ws_host.clone())));

    let ws_zed_manager = zed_manager.clone();
    let ws_host_clone = ws_host.clone();
    let ws_server = tokio::spawn(async move {
        zed::run_ws_server(&ws_host_clone, ws_zed_manager).await
    });

    // Wait for WebSocket server to be ready
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Launch Zed headless
    zed::launch_zed(&bin_path, &workdir, user_data_dir.path(), &session_id, &ws_host).await?;

    // Build app state and start HTTP server
    let state = Arc::new(AppState::new(zed_manager.clone()));

    let http_server = tokio::spawn({
        let state = state.clone();
        let addr = format!("127.0.0.1:{}", args.http_port);
        async move {
            server::run_http_server(&addr, state).await
        }
    });

    // Wait for both servers — unwrap the JoinHandle, then unwrap the Result
    tokio::select! {
        r = ws_server => r.unwrap()?,
        r = http_server => r.unwrap()?,
    }

    Ok(())
}
