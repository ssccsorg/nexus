// SessionServer — serializes access to a StoreSession via a request queue.
//
// Multiple concurrent requests must not interleave writes to the same
// AsyncStore* — CompositeColdStorage orchestration (claim_intent CAS two-step,
// flush_since streaming, etc.) requires exclusive access.
//
// SessionServer owns the StoreSession. Requests are submitted as closures;
// the server processes them sequentially on a dedicated thread (native) or
// via manual `process_one()` drive (WASM).
//
// Architecture:
//
//   Request A (async) ──┐
//   Request B (async) ──┤──→ queue ──→ SessionServer<S> ──→ S (sync)
//                        │                                     │
//                        │                          CompositeColdStorage
//                        │                                     │
//                        │                     AsyncStoreKv/Blob/Object (sync)
//                        │
//                        ←──── responses ──────────────────────┘

use std::sync::mpsc;

use nexus_model::SessionExecute;

/// A job submitted to the SessionServer queue.
type Job<S> = Box<dyn FnOnce(&S) + Send>;

/// Owns a `StoreSession` and processes sync jobs sequentially.
///
/// Generic over `S: SessionExecute` — any session backend works.
///
/// # Native usage
///
/// ```ignore
/// let (mut server, _tx) = SessionServer::new(session);
/// std::thread::spawn(move || server.run());
/// // or use SessionServer::spawn() for a single-call setup
/// ```
///
/// # WASM usage
///
/// ```ignore
/// let session = /* ... */;
/// let server = SessionServer::for_wasm(session);
/// // In the async task:
/// server.process_one();  // or iterate in a loop
/// ```
pub struct SessionServer<S: SessionExecute> {
    session: S,
    rx: mpsc::Receiver<Job<S>>,
}

/// A handle to submit jobs to a `SessionServer`. Clonable for multi-threaded access.
#[derive(Clone)]
pub struct SessionHandle<S: SessionExecute> {
    tx: mpsc::Sender<Job<S>>,
}

impl<S: SessionExecute> SessionHandle<S> {
    /// Submit a closure to the server for immediate execution.
    ///
    /// Blocks the calling thread until the job completes and returns the result.
    /// On WASM, the caller must drive `process_one()` concurrently.
    pub fn submit<T: Send + 'static>(&self, f: impl FnOnce(&S) -> T + Send + 'static) -> T {
        let (resp_tx, resp_rx) = mpsc::channel();
        self.tx
            .send(Box::new(move |session| {
                let _ = resp_tx.send(f(session));
            }))
            .expect("SessionServer channel closed");
        resp_rx
            .recv()
            .expect("SessionServer response channel closed")
    }
}

impl<S: SessionExecute> SessionServer<S> {
    /// Create a server wrapping an existing session, returning both server and handle.
    pub fn new(session: S) -> (Self, SessionHandle<S>) {
        let (tx, rx) = mpsc::channel::<Job<S>>();
        (Self { session, rx }, SessionHandle { tx })
    }

    /// Create a server for WASM targets — returns only the server, no handle.
    /// Use `process_one()` manually within the async task.
    pub fn for_wasm(session: S) -> Self {
        let (_, rx) = mpsc::channel::<Job<S>>();
        Self { session, rx }
    }

    /// Process one job from the queue (blocking). Returns `true` if a job ran.
    pub fn process_one(&mut self) -> bool {
        match self.rx.recv() {
            Ok(job) => {
                job(&self.session);
                true
            }
            Err(_) => false,
        }
    }

    /// Run until the channel closes (all handles dropped).
    pub fn run(&mut self) {
        while self.process_one() {}
    }

    // ── Access ───────────────────────────────────────────────────────────

    /// Access the session when no jobs are in flight.
    pub fn session(&self) -> &S {
        &self.session
    }

    /// Mutable access to session when no jobs are in flight.
    pub fn session_mut(&mut self) -> &mut S {
        &mut self.session
    }
}

// ── Native-only: threaded spawn ──────────────────────────────────────────

#[cfg(not(target_family = "wasm"))]
impl<S: SessionExecute + Send + 'static> SessionServer<S> {
    /// Spawn the server on a dedicated thread.
    ///
    /// Returns a `SessionHandle` for submitting jobs from any thread.
    pub fn spawn(session: S) -> SessionHandle<S> {
        let (tx, rx) = mpsc::channel::<Job<S>>();
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                job(&session);
            }
        });
        SessionHandle { tx }
    }
}
