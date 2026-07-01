use std::sync::{Arc, Mutex};
use std::time::Duration;

use nex::create_blackboard;
use nexus_model::{EvictCapable, IntentCapable, StorageRead};
use nexus_storage_composite::HybridBlackboard;


#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("nexd=info")),
        )
        .init();

    let cfg = nexd::config::NexdConfig::parse();
    tracing::info!(socket_path = %cfg.socket_path.display(), agent = ?cfg.agent_command, "starting nexd");

    let blackboard: Arc<Mutex<HybridBlackboard>> = Arc::new(Mutex::new(create_blackboard()));
    let process_manager = Arc::new(Mutex::new(nexd::manager::ProcessManager::new()));

    if let Some(ref cmd) = cfg.agent_command {
        let mut pm = process_manager.lock().unwrap();
        if let Err(e) = pm.spawn(cmd, &cfg.agent_args) {
            tracing::error!(command = %cmd, error = %e, "failed to spawn startup agent");
        }
    }

    let daemon_config = nexd::daemon::Config {
        name: "nexd".into(),
        log_level: nexd::daemon::LogLevel::Info,
        shutdown_timeout: Duration::from_secs(10),
        force_shutdown_timeout: Duration::from_secs(15),
        kill_timeout: Duration::from_secs(20),
    };

    nexd::daemon::Daemon::new(daemon_config)
        .with_task("ipc", {
            let bb = blackboard.clone();
            let pm = process_manager.clone();
            let cfg = cfg.clone();
            move |shutdown| async move {
                let _ = nexd::server::run(shutdown, cfg, bb, pm).await;
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
        .await
        .expect("daemon run failed");

    tracing::info!("nexd stopped");
}

async fn scheduler_task(
    mut shutdown: nexd::daemon::ShutdownHandle,
    blackboard: Arc<Mutex<HybridBlackboard>>,
    config: nexd::config::NexdConfig,
) {
    let tick_interval = Duration::from_millis(config.tick_interval_ms);
    let heartbeat_ttl = Duration::from_secs(config.heartbeat_ttl_secs);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => { tracing::info!("scheduler stopping"); break; }
            () = tokio::time::sleep(tick_interval) => {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
                let bb = match blackboard.try_lock() {
                    Ok(g) => g,
                    Err(std::sync::TryLockError::WouldBlock) => continue,
                    Err(_) => { tracing::error!("lock poisoned"); break; }
                };
                let state = bb.read_state();
                for intent in &state.intents {
                    if let Some(ref worker) = intent.worker && let Some(hb_secs) = intent.last_heartbeat_at {
                        if now_secs.saturating_sub(hb_secs) > heartbeat_ttl.as_secs() {
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

async fn process_manager_task(
    mut shutdown: nexd::daemon::ShutdownHandle,
    process_manager: Arc<Mutex<nexd::manager::ProcessManager>>,
) {
    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                tracing::info!("process manager stopping");
                { let mut pm = process_manager.lock().unwrap(); pm.shutdown_sync(); }
                tokio::time::sleep(Duration::from_millis(100)).await;
                { let mut pm = process_manager.lock().unwrap(); pm.try_reap(); }
                break;
            }
            () = tokio::time::sleep(Duration::from_secs(5)) => {
                let mut pm = process_manager.lock().unwrap(); pm.try_reap();
            }
        }
    }
}
