// ── Daemon — concurrent task manager with graceful shutdown ────────────
//
// Simplified from proc-daemon: spawns tasks, handles signals, shuts down.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tracing::info;

use crate::daemon::daemon_config::Config;
use crate::daemon::error::Result;
use crate::daemon::shutdown::{ShutdownCoordinator, ShutdownHandle};
use crate::daemon::signal::SignalHandler;

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
type TaskFn = Box<dyn FnOnce(ShutdownHandle) -> BoxFuture + Send>;

struct Task {
    name: &'static str,
    f: TaskFn,
}

/// The main daemon runtime.
pub struct Daemon {
    config: Config,
    tasks: Vec<Task>,
}

impl Daemon {
    /// Create a new daemon with the given config.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            tasks: Vec::new(),
        }
    }

    /// Register a task to run concurrently during the daemon's lifetime.
    pub fn with_task<F, Fut>(mut self, name: &'static str, f: F) -> Self
    where
        F: FnOnce(ShutdownHandle) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.tasks.push(Task {
            name,
            f: Box::new(|shutdown| Box::pin(f(shutdown))),
        });
        self
    }

    /// Run all tasks until shutdown is signalled.
    pub async fn run(self) -> Result<()> {
        let coordinator = Arc::new(ShutdownCoordinator::new(
            self.config.shutdown_timeout.as_millis() as u64,
            self.config.force_shutdown_timeout.as_millis() as u64,
            self.config.kill_timeout.as_millis() as u64,
        ));

        // ── Signal handler ─────────────────────────────────────────
        let sig_coord = Arc::clone(&coordinator);
        let signal_handler = SignalHandler::new(sig_coord);
        tokio::spawn(async move {
            let _ = signal_handler.handle_signals().await;
        });

        // ── Spawn all tasks ────────────────────────────────────────
        let mut handles: Vec<(&'static str, tokio::task::JoinHandle<()>)> = Vec::new();
        for task in self.tasks {
            let rx = coordinator.subscribe();
            let handle = tokio::spawn(async move {
                let shutdown = ShutdownHandle::new(rx);
                (task.f)(shutdown).await;
            });
            handles.push((task.name, handle));
        }

        info!("Daemon started, {} task(s) running", handles.len());

        // ── Wait for shutdown ──────────────────────────────────────
        coordinator.wait_for_shutdown().await.ok();

        // ── Graceful shutdown with timeout ─────────────────────────
        for (name, handle) in handles {
            match tokio::time::timeout(self.config.shutdown_timeout, handle).await {
                Ok(Ok(())) => info!(task = %name, "completed"),
                Ok(Err(e)) => info!(task = %name, "panicked: {e}"),
                Err(_) => info!(task = %name, "timeout, aborting"),
            }
        }

        info!("Daemon stopped");
        Ok(())
    }
}
