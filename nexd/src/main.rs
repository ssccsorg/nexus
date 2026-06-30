// ── nexd — Unified daemon for the nex ecosystem ─────────────────────────
//
// nexd maintains the FIH blackboard and orchestrates nex-* applications.
// It is the persistent runtime that provides shared memory, process
// management, and IPC for a swarm of autonomous agents.
//
// Usage:
//   nexd                          # default config, no agent
//   nexd actus                    # spawn actus at startup
//   nexd ./my-agent --flag value  # spawn custom agent
//
// Environment variables:
//   NEXD_SOCKET_PATH            (default: /tmp/nexd.sock)
//   NEXD_TICK_INTERVAL_MS       (default: 100)
//   NEXD_HEARTBEAT_TTL_SECS     (default: 60)
//   RUST_LOG                    (default: nexd=info)

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nex::create_blackboard;
use nexus_model::{EvictCapable, IntentCapable, StorageRead};
use nexus_storage_composite::HybridBlackboard;
use proc_daemon::{Config, Daemon, LogLevel, ShutdownHandle};

mod config;
mod handler;
mod manager;
mod server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Parse config before logging (proc-daemon handles tracing) ────
    let cfg = config::NexdConfig::parse();

    // ── proc-daemon config (initializes tracing internally) ──────────
    let daemon_config = Config::builder()
        .name("nexd")
        .log_level(LogLevel::Info)
        .shutdown_timeout(Duration::from_secs(10))?
        .force_shutdown_timeout(Duration::from_secs(15))?
        .kill_timeout(Duration::from_secs(20))?
        .build()?;

    tracing::info!(
        socket_path = %cfg.socket_path.display(),
        agent = ?cfg.agent_command,
        "starting nexd"
    );

    // ── Shared blackboard ────────────────────────────────────────────
    let blackboard: Arc<Mutex<HybridBlackboard>> =
        Arc::new(Mutex::new(create_blackboard()));

    // ── Shared process manager ───────────────────────────────────────
    let process_manager = Arc::new(Mutex::new(manager::ProcessManager::new()));

    // Spawn default agent if configured
    if let Some(ref cmd) = cfg.agent_command {
        let mut pm = process_manager.lock().unwrap();
        let _ = pm.spawn(cmd, &cfg.agent_args);
    }

    // ── Build subsystems and run ─────────────────────────────────────
    Daemon::builder(daemon_config)
        .with_task("ipc", {
            let bb = blackboard.clone();
            let pm = process_manager.clone();
            let cfg = cfg.clone();
            move |shutdown| {
                let bb = bb.clone();
                let pm = pm.clone();
                let cfg = cfg.clone();
                server::run(shutdown, cfg, bb, pm)
            }
        })
        .with_task("scheduler", {
            let bb = blackboard.clone();
            let cfg = cfg.clone();
            move |shutdown| {
                let bb = bb.clone();
                let cfg = cfg.clone();
                scheduler_task(shutdown, bb, cfg)
            }
        })
        .with_task("process-manager", {
            let pm = process_manager.clone();
            move |shutdown| {
                let pm = pm.clone();
                process_manager_task(shutdown, pm)
            }
        })
        .run()
        .await?;

    tracing::info!("nexd stopped");
    Ok(())
}

/// Periodic scheduler: heartbeat monitoring and stale intent eviction.
async fn scheduler_task(
    mut shutdown: ShutdownHandle,
    blackboard: Arc<Mutex<HybridBlackboard>>,
    config: config::NexdConfig,
) -> proc_daemon::Result<()> {
    let tick_interval = Duration::from_millis(config.tick_interval_ms);
    let heartbeat_ttl = Duration::from_secs(config.heartbeat_ttl_secs);

    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                tracing::info!("scheduler shutting down");
                break;
            }
            () = tokio::time::sleep(tick_interval) => {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let bb = match blackboard.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        tracing::error!("blackboard lock poisoned in scheduler");
                        break;
                    }
                };

                let state = bb.read_state();

                // Check heartbeat TTL — release stale claims
                for intent in &state.intents {
                    if let Some(ref worker) = intent.worker
                        && let Some(hb_secs) = intent.last_heartbeat_at
                    {
                        let elapsed = now_secs.saturating_sub(hb_secs);
                        if elapsed > heartbeat_ttl.as_secs() {
                            tracing::info!(
                                intent_id = %intent.id,
                                worker = %worker,
                                elapsed_secs = elapsed,
                                "releasing stale claim"
                            );
                            let _ = bb.release_intent(&intent.id.to_string(), worker);
                        }
                    }
                }

                // Evict stale unclaimed intents
                if config.unclaimed_intent_ttl_secs > 0 {
                    let _ = bb.evict_stale_intents(config.unclaimed_intent_ttl_secs);
                }
            }
        }
    }

    Ok(())
}

/// Process manager task: reap exited children periodically.
async fn process_manager_task(
    mut shutdown: ShutdownHandle,
    process_manager: Arc<Mutex<manager::ProcessManager>>,
) -> proc_daemon::Result<()> {
    let reap_interval = Duration::from_secs(5);

    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                tracing::info!("process manager shutting down");
                {
                    let mut pm = process_manager.lock().unwrap();
                    pm.shutdown_sync();
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                {
                    let mut pm = process_manager.lock().unwrap();
                    pm.try_reap();
                }
                break;
            }
            () = tokio::time::sleep(reap_interval) => {
                let mut pm = process_manager.lock().unwrap();
                pm.try_reap();
            }
        }
    }

    Ok(())
}
