// SessionServer — serializes access to a single StoreSession via a request queue.
//
// In CF Workers, each request creates an async entry point. Multiple concurrent
// requests must not interleave writes to the same IoBuffer* — CompositeColdStorage
// orchestration (claim_intent CAS two-step, flush_since streaming, etc.) requires
// exclusive access.
//
// SessionServer owns the StoreSession. Requests are submitted via `submit()`
// which returns a oneshot::Receiver. The server processes requests sequentially
// on a dedicated thread (native) or within a single async task (WASM).
//
// Architecture:
//
//   Request A (async) ──┐
//   Request B (async) ──┤──→ queue ──→ SessionServer ──→ StoreSession (sync)
//                        │                                    │
//                        │                         CompositeColdStorage
//                        │                                    │
//                        │                    IoBufferKv/Blob/Object (sync)
//                        │
//                        ←──── responses ─────────────────────┘

use std::sync::mpsc;

use crate::StoreSession;

/// Owns a StoreSession and processes sync jobs sequentially.
///
/// For native targets: spawn a dedicated thread via `spawn()`.
/// For WASM targets: drive manually with `process_one()` in the async task.
///
/// `process_one()` and `run()` work on both targets (WASM `mpsc::channel`
/// is single-threaded but functional within one task).
pub struct SessionServer {
    session: StoreSession,
    rx: mpsc::Receiver<Box<dyn FnOnce(&StoreSession) + Send>>,
}

impl SessionServer {
    /// Create a server for the given project.
    ///
    /// Returns the server handle and a `Sender` for submitting jobs.
    pub fn new(
        project_id: impl Into<String>,
    ) -> (Self, mpsc::Sender<Box<dyn FnOnce(&StoreSession) + Send>>) {
        let (tx, rx) = mpsc::channel::<Box<dyn FnOnce(&StoreSession) + Send>>();
        let session = StoreSession::new(project_id);
        (Self { session, rx }, tx)
    }

    /// Process one job from the queue (blocking, for WASM manual drive).
    ///
    /// Returns `true` if a job was processed, `false` if the channel is closed.
    pub fn process_one(&mut self) -> bool {
        match self.rx.recv() {
            Ok(job) => {
                job(&self.session);
                true
            }
            Err(_) => false,
        }
    }

    /// Block until all jobs are processed (channel closed).
    ///
    /// For native targets: prefer `spawn()` instead of blocking the main thread.
    /// For WASM targets: use `process_one()` in a loop within the async task.
    pub fn run(&mut self) {
        while self.process_one() {}
    }

    /// Submit a closure and block until it completes.
    ///
    /// The closure `f` receives `&StoreSession`. Its return value is
    /// sent back through an internal channel and returned here.
    ///
    /// For WASM targets: this blocks the current task until the job
    /// completes. Only call when `process_one()` is being driven on
    /// the same task or an external thread.
    pub fn submit_sync<T: Send + 'static>(
        tx: &mpsc::Sender<Box<dyn FnOnce(&StoreSession) + Send>>,
        f: impl FnOnce(&StoreSession) -> T + Send + 'static,
    ) -> T {
        let (resp_tx, resp_rx) = mpsc::channel();
        tx.send(Box::new(move |session| {
            let result = f(session);
            let _ = resp_tx.send(result);
        }))
        .expect("SessionServer channel closed");
        resp_rx.recv().expect("SessionServer response channel closed")
    }

    // ── Access for dirty drain ───────────────────────────────────────────

    /// Access the StoreSession directly (only safe when no jobs are in flight).
    pub fn session(&self) -> &StoreSession {
        &self.session
    }

    /// Mutably access the StoreSession (only safe when no jobs are in flight).
    pub fn session_mut(&mut self) -> &mut StoreSession {
        &mut self.session
    }
}

// ── Native-only: threaded spawn ──────────────────────────────────────────

#[cfg(not(target_family = "wasm"))]
impl SessionServer {
    /// Spawn the server on a dedicated thread (native only).
    ///
    /// Returns the `Sender` for submitting jobs.
    pub fn spawn(
        project_id: impl Into<String> + Send + 'static,
    ) -> mpsc::Sender<Box<dyn FnOnce(&StoreSession) + Send>> {
        let (tx, rx) = mpsc::channel::<Box<dyn FnOnce(&StoreSession) + Send>>();
        let session = StoreSession::new(project_id);
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                job(&session);
            }
        });
        tx
    }
}
