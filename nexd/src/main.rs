// ── nexd — Unified daemon for the nex ecosystem ─────────────────────────
//
// nexd is the pure OS/supervisor layer. It spawns nex-server as a child
// process and communicates via NexClient (JSON-RPC over Unix socket).
// No nex crate dependency at compile time.
//
// Usage:
//   nexd                              # default config, spawn nex-server
//   nexd actus                        # spawn actus at startup
//   nexd --nex-server-path /path/to/nex-server  # custom nex-server binary
//
// Environment variables:
//   NEXD_SOCKET_PATH            (default: /tmp/nexd.sock)
//   NEX_SOCKET_PATH             (default: /tmp/nex-server.sock)
//   NEXD_NEX_SERVER_PATH        (default: nex-server)
//   NEXD_TICK_INTERVAL_MS       (default: 100)
//   NEXD_HEARTBEAT_TTL_SECS     (default: 60)
//   RUST_LOG                    (default: nexd=info)

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nex_client::NexClient;
use nexd::daemon::{Config, Daemon, LogLevel, ShutdownHandle};

#[tokio::main]
async fn main() -> nexd::daemon::Result<()> {
    let cfg = nexd::config::NexdConfig::parse();

    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nexd=info")),
        )
        .try_init();

    tracing::info!(
        socket_path = %cfg.socket_path.display(),
        nex_server_path = %cfg.nex_server_path,
        agent = ?cfg.agent_command,
        "starting nexd"
    );

    // ── Spawn nex-server as child process ──────────────────────────
    let process_manager = Arc::new(Mutex::new(nexd::manager::ProcessManager::new()));

    let nex_server_socket = cfg.nex_server_socket.clone();
    {
        let mut pm = process_manager.lock().unwrap();
        // nex-server is supervised: crash is detected by try_reap and it
        // is respawned with its original command (#146).
        if let Err(e) = pm.spawn_managed(&cfg.nex_server_path, &[], true) {
            tracing::error!(path = %cfg.nex_server_path, error = %e, "failed to spawn nex-server");
        }
    }

    // Wait for nex-server socket to be ready
    let waited = wait_for_socket(&nex_server_socket).await;
    if waited >= 60 {
        tracing::error!("nex-server socket not ready after 60s");
        return Ok(());
    }
    tracing::info!(path = %nex_server_socket, waited_secs = waited, "nex-server ready");

    // Connect via NexClient
    let _client = NexClient::connect(&nex_server_socket)
        .await
        .map_err(|e| nexd::daemon::Error::config(format!("nex-client connect: {e}")))?;

    // Spawn startup agent if configured
    if let Some(ref cmd) = cfg.agent_command {
        let mut pm = process_manager.lock().unwrap();
        if let Err(e) = pm.spawn(cmd, &cfg.agent_args) {
            tracing::error!(command = %cmd, error = %e, "failed to spawn startup agent");
        }
    }

    let daemon_config = Config::builder()
        .name("nexd")
        .log_level(LogLevel::Info)
        .shutdown_timeout(Duration::from_secs(10))?
        .force_shutdown_timeout(Duration::from_secs(15))?
        .kill_timeout(Duration::from_secs(20))?
        .build()?;

    Daemon::builder(daemon_config)
        .with_task("ipc", {
            let pm = process_manager.clone();
            let cfg = cfg.clone();
            move |shutdown| {
                let pm = pm.clone();
                let cfg = cfg.clone();
                async move {
                    let _ = nexd::server::run(shutdown, cfg, pm).await;
                    Ok(())
                }
            }
        })
        .with_task("scheduler", {
            let cfg = cfg.clone();
            move |shutdown| {
                let cfg = cfg.clone();
                async move {
                    scheduler_task(shutdown, cfg).await;
                    Ok(())
                }
            }
        })
        .with_task("process-manager", {
            let pm = process_manager.clone();
            move |shutdown| {
                let pm = pm.clone();
                async move {
                    process_manager_task(shutdown, pm).await;
                    Ok(())
                }
            }
        })
        .run()
        .await?;

    tracing::info!("nexd stopped");
    Ok(())
}

/// Wait for nex-server Unix socket to appear. Returns seconds waited.
async fn wait_for_socket(path: &str) -> u64 {
    let mut waited = 0u64;
    while waited < 60 {
        if std::path::Path::new(path).exists() {
            return waited;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        waited += 1;
    }
    waited
}

async fn scheduler_task(mut shutdown: ShutdownHandle, config: nexd::config::NexdConfig) {
    let tick_interval = Duration::from_millis(config.tick_interval_ms);
    let heartbeat_ttl = Duration::from_secs(config.heartbeat_ttl_secs);
    let socket = config.nex_server_socket.clone();

    loop {
        tokio::select! {
            () = shutdown.cancelled() => { tracing::info!("scheduler stopping"); break; }
            () = tokio::time::sleep(tick_interval) => {
                let Ok(mut client) = NexClient::connect(&socket).await else { continue; };
                // Structured read only: the heartbeat check needs ids,
                // workers, and timestamps, not content materialization.
                let Ok(state) = client.read_state_light().await else { continue; };

                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

                // Check heartbeat TTL — release stale claims
                if let Some(intents) = state["intents"].as_array() {
                    for intent in intents {
                        let worker = match intent["worker"].as_str() {
                            Some(w) if !w.is_empty() => w,
                            _ => continue,
                        };
                        let last_hb = match intent["last_heartbeat_at"].as_u64() {
                            Some(hb) => hb,
                            _ => continue,
                        };
                        let id = match intent["id"].as_str() {
                            Some(id) => id,
                            _ => continue,
                        };
                        if now_secs.saturating_sub(last_hb) > heartbeat_ttl.as_secs() {
                            let _ = client.call("release_intent", serde_json::json!({
                                "id": id, "agent": worker,
                            })).await;
                        }
                    }
                }
            }
        }
    }
}

async fn process_manager_task(
    mut shutdown: ShutdownHandle,
    process_manager: Arc<Mutex<nexd::manager::ProcessManager>>,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                tracing::info!("process manager stopping");
                // SIGTERM to children (nex-server shuts down gracefully),
                // then poll for exit with short locks so IPC handlers are
                // not starved for the whole shutdown window; force-kill
                // the remainder after the deadline (#146).
                {
                    let mut pm = process_manager.lock().unwrap();
                    pm.request_shutdown_children();
                }
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                loop {
                    let all_exited = {
                        let mut pm = process_manager.lock().unwrap();
                        let exited = pm.all_children_exited();
                        pm.try_reap();
                        exited
                    };
                    if all_exited || tokio::time::Instant::now() >= deadline {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                {
                    let mut pm = process_manager.lock().unwrap();
                    pm.force_kill_children();
                }
                break;
            }
            () = tokio::time::sleep(Duration::from_secs(1)) => {
                let mut pm = process_manager.lock().unwrap(); pm.try_reap();
            }
        }
    }
}
