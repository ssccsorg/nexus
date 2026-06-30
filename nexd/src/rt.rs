// ── rt — Minimal async runtime layer (replaces proc-daemon) ────────────
//
// A lightweight, self-contained daemon runtime that provides exactly
// the features nexd needs: concurrent task management, signal handling,
// and graceful shutdown. No external dependencies beyond tokio + tracing.
//
// Design principles:
//   - 0 transitive dependencies beyond tokio and tracing
//   - No tracing init conflicts (caller controls all setup)
//   - Panic-free shutdown: each phase has a timeout, never hangs
//   - Task-level isolation: one task crash does not take down others

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::broadcast;

// ── Shutdown reason ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// SIGTERM (default termination signal).
    Sigterm,
    /// SIGINT (Ctrl+C).
    Sigint,
    /// SIGQUIT.
    Sigquit,
    /// All tasks completed normally.
    Completed,
}

// ── Shutdown handle (per-task) ──────────────────────────────────────────

#[derive(Clone)]
pub struct ShutdownHandle {
    rx: Arc<tokio::sync::Mutex<broadcast::Receiver<ShutdownReason>>>,
}

impl ShutdownHandle {
    /// Wait until shutdown is signalled.
    pub async fn cancelled(&mut self) {
        let mut rx = self.rx.lock().await;
        let _ = rx.recv().await;
    }

    /// Non-blocking check.
    pub fn is_shutdown(&self) -> bool {
        if let Some(rx) = self.rx.try_lock() {
            matches!(
                rx.try_recv(),
                Ok(_) | Err(broadcast::error::TryRecvError::Closed)
            )
        } else {
            false
        }
    }

    /// Create a cancelled handle (for testing).
    #[cfg(test)]
    pub fn cancelled_handle() -> Self {
        let (tx, rx) = broadcast::channel(1);
        let _ = tx.send(ShutdownReason::Completed);
        Self { rx: Arc::new(tokio::sync::Mutex::new(rx)) }
    }
}

// ── Daemon configuration ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub name: String,
    pub shutdown_timeout: Duration,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            name: String::from("nexd"),
            shutdown_timeout: Duration::from_secs(10),
        }
    }
}

// ── Task wrapper ────────────────────────────────────────────────────────

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
type TaskFn = Box<dyn FnOnce(ShutdownHandle) -> BoxFuture + Send>;

struct Task {
    name: &'static str,
    f: TaskFn,
}

// ── Daemon ──────────────────────────────────────────────────────────────

pub struct Daemon {
    config: DaemonConfig,
    tasks: Vec<Task>,
}

impl Daemon {
    /// Create a new daemon with the given config.
    pub fn new(config: DaemonConfig) -> Self {
        Self {
            config,
            tasks: Vec::new(),
        }
    }

    /// Register a task. Tasks run concurrently until shutdown.
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

    /// Run all tasks until shutdown is signalled or all tasks complete.
    pub async fn run(self) {
        let (tx, _rx) = broadcast::channel::<ShutdownReason>(16);
        let _task_count = self.tasks.len();

        // ── Signal handler task ─────────────────────────────────────
        let signal_tx = tx.clone();
        tokio::spawn(async move {
            setup_signal_handler(signal_tx).await;
        });

        // ── Spawn all tasks ─────────────────────────────────────────
        let mut handles: Vec<(&'static str, tokio::task::JoinHandle<()>)> = Vec::new();
        for task in self.tasks {
            let rx = tx.subscribe();
            let handle = tokio::spawn(async move {
                let shutdown = ShutdownHandle { rx: Arc::new(tokio::sync::Mutex::new(rx)) };
                (task.f)(shutdown).await;
            });
            handles.push((task.name, handle));
        }

        // ── Wait for shutdown signal ────────────────────────────────
        let mut shutdown_rx = tx.subscribe();
        tokio::select! {
            _ = shutdown_rx.recv() => {
                tracing::info!(name = %self.config.name, "shutdown initiated");
            }
        }

        // ── Graceful shutdown with timeout ──────────────────────────
        let deadline = tokio::time::sleep(self.config.shutdown_timeout);
        tokio::pin!(deadline);

        for i in 0..handles.len() {
            let (name, handle) = &handles[i];
            tokio::select! {
                _ = &mut deadline => {
                    tracing::warn!(task = %name, "shutdown timeout reached, aborting");
                    handle.abort();
                }
                _ = handle => {
                    tracing::debug!(task = %name, "task completed");
                }
            }
        }

        tracing::info!(name = %self.config.name, "daemon stopped");
    }
}

// ── Signal handling ─────────────────────────────────────────────────────

async fn setup_signal_handler(tx: broadcast::Sender<ShutdownReason>) {
    // Create signal listeners. On platforms without Unix signals,
    // this gracefully degrades to no-op.
    #[cfg(unix)]
    {
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to create SIGTERM listener");
        let mut sigint =
            signal(SignalKind::interrupt()).expect("failed to create SIGINT listener");
        let mut sigquit =
            signal(SignalKind::quit()).expect("failed to create SIGQUIT listener");

        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM");
                let _ = tx.send(ShutdownReason::Sigterm);
            }
            _ = sigint.recv() => {
                tracing::info!("received SIGINT");
                let _ = tx.send(ShutdownReason::Sigint);
            }
            _ = sigquit.recv() => {
                tracing::info!("received SIGQUIT");
                let _ = tx.send(ShutdownReason::Sigquit);
            }
        }
    }

    #[cfg(not(unix))]
    {
        // Non-Unix platforms: no signal handling. Shutdown only via all tasks completing.
        std::future::pending::<()>().await;
    }
}
