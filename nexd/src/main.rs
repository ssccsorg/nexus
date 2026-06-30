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
use tracing_subscriber::EnvFilter;

use crate::rt::{Daemon, DaemonConfig, ShutdownHandle};

mod config;
mod handler;
mod manager;
mod server;

mod rt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── Initialize logging ───────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("nexd=info")),
        )
        .init();

    // ── Parse config ─────────────────────────────────────────────────
    let cfg = config::NexdConfig::parse();
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
        if let Err(e) = pm.spawn(cmd, &cfg.agent_args) {
            tracing::error!(command = %cmd, error = %e, "failed to spawn startup agent");
        }
    }

    // ── Daemon config ────────────────────────────────────────────────
    let daemon_config = DaemonConfig {
        name: "nexd".into(),
        shutdown_timeout: Duration::from_secs(10),
    };

    // ── Build subsystems and run ─────────────────────────────────────
    Daemon::new(daemon_config)
        .with_task("ipc", {
            let bb = blackboard.clone();
            let pm = process_manager.clone();
            let cfg = cfg.clone();
            move |shutdown| async move {
                server::run(shutdown, cfg, bb, pm).await;
            }
        })
        .with_task("scheduler", {
            let bb = blackboard.clone();
            let cfg = cfg.clone();
            move |shutdown| async move {
                scheduler_task(shutdown, bb, cfg).await;
            }
        })
        .with_task("process-manager", {
            let pm = process_manager.clone();
            move |shutdown| async move {
                process_manager_task(shutdown, pm).await;
            }
        })
        .run()
        .await;

    tracing::info!("nexd stopped");
    Ok(())
}

/// Periodic scheduler: heartbeat monitoring and stale intent eviction.
async fn scheduler_task(
    mut shutdown: ShutdownHandle,
    blackboard: Arc<Mutex<HybridBlackboard>>,
    config: config::NexdConfig,
) {
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

                let bb = match blackboard.try_lock() {
                    Ok(g) => g,
                    Err(std::sync::TryLockError::WouldBlock) => continue,
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        tracing::error!("blackboard lock poisoned");
                        break;
                    }
                };

                let state = bb.read_state();

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

                if config.unclaimed_intent_ttl_secs > 0 {
                    let _ = bb.evict_stale_intents(config.unclaimed_intent_ttl_secs);
                }
            }
        }
    }
}

/// Process manager task: reap exited children periodically.
async fn process_manager_task(
    mut shutdown: ShutdownHandle,
    process_manager: Arc<Mutex<manager::ProcessManager>>,
) {
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
}
