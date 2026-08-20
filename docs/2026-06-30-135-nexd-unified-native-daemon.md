# 135-nexd: Unified native daemon for the nex ecosystem

## Context

`nexd` is the unified daemon for the nex ecosystem, acting as the operating system for a swarm of autonomous agents. It provides FIH blackboard (shared memory), process management, Unix socket IPC, and lifecycle management for `nex-*` applications. The conceptual reference is that `nexd` is to `nex-*` as iOS is to apps: it provides foundational services that agents consume through a well-defined interface.

The daemon is built on the `proc-daemon` framework v1.1.2, which provides graceful shutdown, signal handling, and subsystem lifecycle management out of the box. The blackboard implementation reuses `nex::create_blackboard()` returning a `HybridBlackboard` — the same pattern used by the HTTP gateway (`apps/nex-api`).

## Key technical decisions

### proc-daemon as foundation

Using proc-daemon provided production-grade graceful shutdown and signal handling immediately. The trade-off is approximately 70 transitive dependencies added to `Cargo.lock` and a tracing initialization conflict: proc-daemon internally calls `tracing_subscriber::init()` after the caller has already done so, producing `"global default trace dispatcher has already been set"`. This was resolved by removing the caller's explicit init and letting proc-daemon handle logging.

### HybridBlackboard reuse

Using `nex::create_blackboard()` directly (wrapped in `Arc<Mutex<HybridBlackboard>>`) avoids duplicating the FIH storage implementation. The concrete type preserves access to `EvictCapable` trait — not possible with `Box<dyn Blackboard>`.

### Process Manager in Phase 1

The Process Manager handles child process lifecycle: spawn via `tokio::process::Command`, reap via `try_wait()`, and sync shutdown via `start_kill()`. The shutdown method is synchronous to avoid holding a `MutexGuard` across an `.await` point — a pattern that caused a `Send` bound failure during initial implementation.

### Scheduler uses try_lock

The OODA scheduler uses `try_lock()` instead of `lock()` on the blackboard mutex. If the lock is contended (e.g., an IPC handler is holding it), the scheduler skips the tick and retries on the next interval. This prevents blocking IPC handlers during OODA cycles.

### Connection semaphore

Each client connection spawns a `tokio::spawn` task. Without limits, a connection storm could exhaust the tokio runtime's task budget. A `tokio::sync::Semaphore` with a limit of 128 concurrent connections prevents this.

### Read methods added after review

Initially only `read_state` (full board dump) was available. A code review identified the need for single-entity reads. `read_fact`, `read_intent`, and `read_hint` were added, each filtering `read_state()` output by ID.

## Code review outcomes

| Comment | Resolution | Commit |
|---------|-----------|--------|
| `dispatch` is async but all handlers are sync | Made `dispatch` sync | `3d0a1f9` |
| Missing single read methods | Added `read_fact`/`read_intent`/`read_hint` | `237ab42` |
| `write_hint` error uses generic -32000 | Changed to `map_error()` | `3d0a1f9` |
| Unbounded connection spawn | Added `Semaphore` (max 128) | `3d0a1f9` |
| Scheduler holds lock during tick | Changed to `try_lock()`, skip if contended | `3d0a1f9` |
| Agent spawn error silently ignored | Added `tracing::error!()` logging | `3d0a1f9` |

## Testing

27 integration tests covering:

- **Transport**: socket creation, request/response roundtrip, concurrent connections (5 clients), request pipelining (3 in-flight), invalid JSON rejection, missing method field, unknown method, large message (50KB), daemon survives client disconnect and partial write
- **Communication**: two agents via blackboard (write Fact → read Intent → conclude), delegation pattern (claim → release → reclaim), three agents sharing single blackboard
- **Lifecycle**: spawn/kill agent, multi-agent management (3 agents), short-lived agent reap, nonexistent command error, nonexistent PID error, graceful shutdown via SIGTERM with socket cleanup
- **Error handling**: intent without facts rejected, double claim conflict, wrong-agent release, conclude without claim behavior (known gap)
- **Read by ID**: `read_fact`, `read_intent`, `read_hint` with found and not-found cases

CI verification (`./run.sh --apps`) runs 7 real scenarios: basic FIH operations, intent lifecycle, agent lifecycle, error handling, graceful shutdown, concurrent operations, read by ID.

## Gaps discovered

| Gap | Severity | Description |
|-----|----------|-------------|
| Claim-before-conclude not enforced | Medium | `HybridBlackboard` (PetgraphStorage) allows concluding an intent without claiming it first |
| Short-lived process reap delay | Low | `try_reap` runs every 5s; agents that exit quickly are zombies for up to 5s |
| No push/subscribe mechanism | Low | Clients must poll `read_state` — no SSE or WebSocket for real-time updates |
| proc-daemon tracing conflict | Resolved | Switched from `init()` to proc-daemon-managed logging |

## What's next

- **Phase 2**: SQLite persistence, WASM plugin runtime via `wasmtime`
- **Phase 2**: Event subscription (push instead of polling)
- **Future**: Replace `proc-daemon` with a lightweight internal `rt.rs` (~200 lines, no extra dependencies) — tracked in branch `137-proc-daemon-replace`
