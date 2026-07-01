use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::signal::unix::{SignalKind, signal};
use tracing::info;
use crate::daemon::shutdown::{ShutdownCoordinator, ShutdownReason};
use crate::daemon::error::Error;

pub struct SignalHandler {
    coordinator: Arc<ShutdownCoordinator>,
    _stopped: Arc<AtomicBool>,
}

impl SignalHandler {
    pub fn new(coordinator: Arc<ShutdownCoordinator>) -> Self {
        Self { coordinator, _stopped: Arc::new(AtomicBool::new(false)) }
    }

    pub async fn handle_signals(&self) -> Result<(), Error> {
        let mut sigterm = signal(SignalKind::terminate()).map_err(|e| Error::signal(e.to_string()))?;
        let mut sigint = signal(SignalKind::interrupt()).map_err(|e| Error::signal(e.to_string()))?;
        let mut sigquit = signal(SignalKind::quit()).map_err(|e| Error::signal(e.to_string()))?;
        info!("Unix signal handlers registered");
        tokio::select! {
            _ = sigterm.recv() => { self.coordinator.initiate_shutdown(ShutdownReason::Signal(15)); }
            _ = sigint.recv() => { self.coordinator.initiate_shutdown(ShutdownReason::Signal(2)); }
            _ = sigquit.recv() => { self.coordinator.initiate_shutdown(ShutdownReason::Signal(3)); }
        }
        Ok(())
    }

    pub fn stop(&self) {}
}
