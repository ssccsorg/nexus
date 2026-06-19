// ── nex-zed: neXus instance with an ACP surface for the Zed editor ──────
//
// A standalone binary that embeds ACP (Agent Client Protocol) as one of its
// communication surfaces. To the Zed editor, it appears as a custom agent
// server spawned as a child process with piped stdin/stdout/stderr.
//
// This crate is a thin wrapper around `acp_bridge` (ext/acp-bridge), adding
// neXus FIH blackboard integration on top of the base ACP agent functionality.
//
// Architecture:
//   Zed Editor
//     └── spawns child process (ACP stdio)
//          └── nex-zed (this binary)
//               ├── acp_bridge crate (ACP engine, LLM, tools)
//               └── neXus FIH integration (Phase 2+)

use acp_bridge::acp;
use acp_bridge::a2a;
use acp_bridge::bench;
use acp_bridge::client;
use acp_bridge::config::{AgentConfig, ConfigFile};
use acp_bridge::engine::{self, AppState, Notification};
use acp_bridge::hardware;
use acp_bridge::llm;
use acp_bridge::protocol::{AcpError, JsonRpcRequest};
use clap::Parser;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

// ── Run mode ──────────────────────────────────────────────────────────

enum RunMode {
    Acp,
    A2a,
    Client,
    Bench,
}

// ── CLI args ──────────────────────────────────────────────────────────

#[derive(Parser, Debug, Clone)]
#[command(name = "nex-zed", version, about)]
struct Args {
    /// Path to neXus daemon Unix socket (unused until FIH phase).
    #[arg(long, default_value = "/var/run/nexus.sock")]
    nexus_socket: String,

    /// Enable verbose logging (sets RUST_LOG=debug).
    #[arg(long, short = 'v')]
    verbose: bool,

    /// Path to config.toml (optional; env vars used otherwise).
    #[arg(long)]
    config: Option<String>,

    /// Run in A2A HTTP server mode.
    #[arg(long)]
    a2a: bool,

    /// Run in client mode (spawn external ACP agent).
    #[arg(long)]
    client: bool,

    /// Run benchmark.
    #[arg(long)]
    bench: bool,
}

// ── .env loader ────────────────────────────────────────────────────────

/// Load `.env` file from the binary's parent directory into environment.
/// Does not override existing vars. Silently ignores missing `.env`.
fn load_dotenv() {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    let content = match std::fs::read_to_string(&env_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        // Split on first '='
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            let val = line[eq + 1..].trim().trim_matches('"');
            // Only set if not already present (env vars take precedence)
            if std::env::var(key).is_err() {
                // SAFETY: called at the very top of main(), single-threaded
                unsafe { std::env::set_var(key, val); }
            }
        }
    }
}

// ── Main ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    load_dotenv();

    let args = Args::parse();

    // Verbose flag sets RUST_LOG=debug if no explicit override
    if args.verbose && std::env::var("RUST_LOG").is_err() {
        // SAFETY: called before any threads are spawned and before tracing init;
        // this is the single-threaded top of main() where set_var is sound.
        unsafe { std::env::set_var("RUST_LOG", "nex_zed=debug,acp_bridge=debug"); }
    }

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nex_zed=info".parse().unwrap()),
        )
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();

    // Determine run mode
    let mode = if args.bench {
        RunMode::Bench
    } else if args.client {
        RunMode::Client
    } else if args.a2a {
        RunMode::A2a
    } else {
        RunMode::Acp
    };

    // Load config
    let config_file = args
        .config
        .as_ref()
        .map(|path| ConfigFile::load(std::path::Path::new(path)));

    // Client mode: spawn external ACP agent
    if let RunMode::Client = mode {
        let agent_config = config_file
            .as_ref()
            .and_then(|f| f.agent_config())
            .or_else(AgentConfig::from_env);

        match agent_config {
            Some(ac) => {
                for line in hardware::detect().report_lines() {
                    info!("{line}");
                }
                client::run_client_mode(&ac).await;
                return;
            }
            None => {
                eprintln!("Error: --client mode requires agent config.");
                eprintln!("Set AGENT_COMMAND env var or add [agent] section to config.toml");
                return;
            }
        }
    }

    // LLM and A2A config
    let (config, a2a_config) = match config_file {
        Some(file) => {
            let a2a_cfg = file.a2a_config();
            (file.into_llm_config(), a2a_cfg)
        }
        None => (llm::LlmConfig::from_env(), a2a::A2aConfig::from_env()),
    };

    // Benchmark mode
    if let RunMode::Bench = mode {
        for line in hardware::detect().report_lines() {
            info!("{line}");
        }
        info!(base_url = %config.base_url, model = %config.model, "Running benchmark");
        let results = bench::run(&config, &bench::default_fixtures()).await;
        bench::print_report(&config, &results);
        return;
    }

    // Startup banner
    let mode_str = match mode {
        RunMode::Acp => "acp",
        RunMode::A2a => "a2a",
        _ => unreachable!(),
    };
    info!(
        instance = "nex-zed",
        version = env!("CARGO_PKG_VERSION"),
        mode = mode_str,
        model = %config.model,
        base_url = %config.base_url,
        max_history_turns = config.max_history_turns,
        max_sessions = config.max_sessions,
        session_idle_timeout_secs = config.session_idle_timeout_secs,
        "Starting nex-zed"
    );

    for line in hardware::detect().report_lines() {
        info!("{line}");
    }

    probe_backend(&config).await;

    // Shared state
    let state = AppState::new(config);

    // Idle session eviction
    let idle_timeout = state.config.session_idle_timeout_secs;
    if idle_timeout > 0 {
        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            let interval = Duration::from_secs(idle_timeout.min(60));
            loop {
                tokio::time::sleep(interval).await;
                state_clone.evict_idle_sessions(idle_timeout);
            }
        });
    }

    // Run in selected mode
    match mode {
        RunMode::Acp => run_acp_loop(state).await,
        RunMode::A2a => {
            if let Err(e) = a2a::serve(state, a2a_config).await {
                error!(error = %e, "A2A server error");
            }
        }
        RunMode::Client | RunMode::Bench => unreachable!(),
    }
}

// ── Backend probing ───────────────────────────────────────────────────

async fn probe_backend(config: &llm::LlmConfig) {
    match llm::probe_backend(config).await {
        Ok(models) if models.is_empty() => {
            info!("Connected to backend (no models listed)");
        }
        Ok(models) => {
            info!(count = models.len(), "Available models:");
            for m in &models {
                info!("  - {m}");
            }
            if !models.iter().any(|m| {
                m.starts_with(&config.model)
                    || config.model.starts_with(m.split(':').next().unwrap_or(""))
            }) {
                warn!(configured = %config.model, "Configured model not found in available models");
            }
        }
        Err(reason) => {
            warn!(
                base_url = %config.base_url,
                error = %reason,
                "Cannot reach backend — will retry on first request"
            );
        }
    }

    // Ollama-specific info (no-op for OpenAI-compatible APIs)
    if let Some(info) = llm::query_model_info(config).await {
        info!(context_length = info.context_length, "Model info from /api/show");
    }
    if let Some(running) = llm::query_running_models(config).await {
        if running.is_empty() {
            warn!(
                model = %config.model,
                "No models loaded in VRAM — first request may be slow"
            );
        } else {
            info!(count = running.len(), "Running models (loaded in VRAM):");
            for m in &running {
                info!("  - {m}");
            }
        }
    }
}

// ── ACP mode ──────────────────────────────────────────────────────────

async fn run_acp_loop(state: Arc<AppState>) {
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    loop {
        tokio::select! {
            line_result = lines.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        let trimmed = line.trim().to_string();
                        if trimmed.is_empty() { continue; }

                        let msg: JsonRpcRequest = match serde_json::from_str(&trimmed) {
                            Ok(m) => m,
                            Err(e) => {
                                debug!(error = %e, "Skipping invalid JSON-RPC line");
                                continue;
                            }
                        };

                        let id_opt = msg.id;
                        let method = msg.method.as_str();
                        let params = msg.params.clone().unwrap_or(json!({}));
                        debug!(?id_opt, method, "Received message");

                        // JSON-RPC 2.0 notification (no id)
                        let id = match id_opt {
                            Some(id) => id,
                            None => {
                                match method {
                                    "session/cancel" => {
                                        info!(
                                            session_id = %params.get("sessionId").and_then(|v| v.as_str()).unwrap_or(""),
                                            "Received session/cancel notification"
                                        );
                                    }
                                    _ => debug!(method, "Ignoring unknown notification"),
                                }
                                continue;
                            }
                        };

                        match method {
                            "initialize" => {
                                let result = engine::initialize(&state.config);
                                acp::send_response(id, result);
                            }
                            "session/new" => {
                                let raw_cwd = params.get("cwd").and_then(|v| v.as_str()).unwrap_or("/tmp");
                                match engine::session_new(&state, raw_cwd) {
                                    Ok(session_id) => acp::send_response(id, json!({"sessionId": session_id})),
                                    Err(e) => acp::send_error(id, e.code(), &e.to_string()),
                                }
                            }
                            "session/prompt" => handle_acp_prompt(id, &params, &state).await,
                            "session/end" => {
                                let session_id = params.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");
                                if session_id.is_empty() {
                                    acp::send_error(id, AcpError::MissingParam { field: "sessionId".into() }.code(), "Missing sessionId");
                                } else {
                                    match engine::session_end(&state, session_id) {
                                        Ok(()) => acp::send_response(id, json!({"status": "ended"})),
                                        Err(e) => acp::send_error(id, e.code(), &e.to_string()),
                                    }
                                }
                            }
                            "session/load" | "session/resume" => {
                                let err = AcpError::MethodNotFound { method: method.to_string() };
                                acp::send_error(id, err.code(), &err.to_string());
                            }
                            "session/set_mode" => {
                                let err = AcpError::MethodNotFound { method: method.to_string() };
                                acp::send_error(id, err.code(), &err.to_string());
                            }
                            _ => {
                                let err = AcpError::MethodNotFound { method: method.to_string() };
                                acp::send_error(id, err.code(), &err.to_string());
                            }
                        }
                    }
                    Ok(None) => {
                        info!("stdin closed, shutting down gracefully");
                        break;
                    }
                    Err(e) => {
                        error!(error = %e, "Error reading stdin");
                        break;
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received shutdown signal, exiting");
                break;
            }
        }
    }

    let n = state.cleanup();
    if n > 0 {
        info!(sessions = n, "Cleaned up sessions on exit");
    }
}

/// Handle session/prompt — stream engine notifications to ACP stdout.
async fn handle_acp_prompt(id: u64, params: &Value, state: &Arc<AppState>) {
    let session_id = match params.get("sessionId").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            let err = AcpError::MissingParam { field: "sessionId".into() };
            acp::send_error(id, err.code(), &err.to_string());
            return;
        }
    };

    let prompt_value = params.get("prompt").cloned().unwrap_or(Value::Null);
    let raw_user_text = engine::extract_user_text_from_prompt(&prompt_value);
    let (user_text, _sender_context) = engine::strip_sender_context(&raw_user_text);
    let user_images = engine::extract_user_images_from_prompt(&prompt_value);

    if user_text.trim().is_empty() && user_images.is_empty() {
        let err = AcpError::MissingParam {
            field: "prompt (expected non-empty text or image content)".into(),
        };
        acp::send_error(id, err.code(), &err.to_string());
        return;
    }

    let (notify_tx, mut notify_rx) = mpsc::unbounded_channel::<Notification>();

    let state_clone = Arc::clone(state);
    let sid = session_id.clone();
    let handle = tokio::spawn(async move {
        engine::session_prompt(&state_clone, &sid, &user_text, &user_images, Some(notify_tx)).await
    });

    while let Some(notif) = notify_rx.recv().await {
        match notif {
            Notification::Thinking => acp::notify_thinking(),
            Notification::ToolStart(name) => acp::notify_tool_start(&name),
            Notification::ToolDone(name, status) => acp::notify_tool_done(&name, &status),
            Notification::TextChunk(text) => acp::notify_text(&text),
        }
    }

    let result = handle.await.unwrap_or_else(|_| engine::PromptResult {
        status: "failed".into(),
        text: "Internal error".into(),
        error: None,
    });

    if let Some(err) = &result.error {
        acp::send_error(id, err.code(), &err.to_string());
    } else {
        acp::send_response(
            id,
            json!({"status": result.status, "text": result.text}),
        );
    }
}
