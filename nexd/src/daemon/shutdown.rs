use crate::daemon::error::{Error, Result};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    Signal(i32),
    Requested,
    Error,
    Forced,
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signal(s) => write!(f, "Signal({s})"),
            Self::Requested => write!(f, "Requested"),
            Self::Error => write!(f, "Error"),
            Self::Forced => write!(f, "Forced"),
        }
    }
}

#[derive(Clone)]
pub struct ShutdownHandle {
    rx: Arc<tokio::sync::Mutex<broadcast::Receiver<ShutdownReason>>>,
}

impl ShutdownHandle {
    pub fn new(rx: broadcast::Receiver<ShutdownReason>) -> Self {
        Self {
            rx: Arc::new(tokio::sync::Mutex::new(rx)),
        }
    }

    pub async fn cancelled(&mut self) {
        let mut rx = self.rx.lock().await;
        let _ = rx.recv().await;
    }

    pub fn is_shutdown(&self) -> bool {
        if let Ok(mut rx) = self.rx.try_lock() {
            matches!(
                rx.try_recv(),
                Ok(_) | Err(broadcast::error::TryRecvError::Closed)
            )
        } else {
            false
        }
    }
}

pub struct ShutdownCoordinator {
    inner: Arc<ShutdownInner>,
}

#[allow(dead_code)]
struct ShutdownInner {
    initiated: AtomicBool,
    reason: std::sync::Mutex<Option<ShutdownReason>>,
    tx: broadcast::Sender<ShutdownReason>,
    graceful_timeout_ms: u64,
    force_timeout_ms: u64,
    kill_timeout_ms: u64,
}

impl ShutdownCoordinator {
    pub fn new(graceful_ms: u64, force_ms: u64, kill_ms: u64) -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            inner: Arc::new(ShutdownInner {
                initiated: AtomicBool::new(false),
                reason: std::sync::Mutex::new(None),
                tx,
                graceful_timeout_ms: graceful_ms,
                force_timeout_ms: force_ms,
                kill_timeout_ms: kill_ms,
            }),
        }
    }

    pub fn create_handle(&self) -> ShutdownHandle {
        ShutdownHandle::new(self.inner.tx.subscribe())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ShutdownReason> {
        self.inner.tx.subscribe()
    }

    pub fn initiate_shutdown(&self, reason: ShutdownReason) -> bool {
        if self
            .inner
            .initiated
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            *self.inner.reason.lock().unwrap() = Some(reason);
            let _ = self.inner.tx.send(reason);
            info!("Shutdown initiated: {reason}");
            true
        } else {
            false
        }
    }

    pub fn is_shutdown(&self) -> bool {
        self.inner.initiated.load(Ordering::Relaxed)
    }

    pub async fn wait_for_shutdown(&self) -> Result<()> {
        if !self.is_shutdown() {
            return Err(Error::invalid_state("Shutdown not initiated"));
        }
        // Simple delay to allow subsystems to react
        let timeout = Duration::from_millis(self.inner.graceful_timeout_ms);
        tokio::time::sleep(timeout).await;
        Ok(())
    }
}
