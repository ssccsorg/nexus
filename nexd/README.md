# nexd — Native daemon / OS layer for the nex ecosystem

`nexd` is the **runtime environment** for `nex-*` applications. It provides process lifecycle management, IPC substrate, device orchestration, and the persistent FIH blackboard service. Where `nex` defines the blackboard logic and storage semantics, `nexd` provides the environment in which that logic executes.

**Conceptual reference**: nexd is to nex-* as iOS is to apps.

## Layer Identity

| Aspect | nexd | nex |
|--------|------|-----|
| Role | **Daemon / OS** — persistent runtime environment | **Blackboard engine** — FIH logic + storage |
| Knows | IPC protocol, process management, device control, OTA | FIH semantics, coordinate indexing, storage I/O |
| Does not know | FIH internal structure, coordinate index layout, storage backend details | Runtime environment, process model, network topology |
| Communication | Unix socket JSON-RPC (wire protocol only) | Internal API consumed by apps via the `nex` crate |
| Deployment | Native daemon (macOS, Linux), embedded (Rem) | Library crate, server binary |

## Status

`nexd` links `nex` as a Rust crate and calls `nex::create_blackboard()`.

### What works

- Unix domain socket IPC with JSON-RPC style protocol (11 methods)
- FIH blackboard service via embedded `HybridBlackboard` (same as `apps/nex-api`)
- Process Manager — spawn, monitor (`try_reap`), kill child `nex-*` agents
- OODA scheduler — heartbeat TTL monitoring, stale intent eviction
- Graceful shutdown — SIGTERM/SIGINT/SIGQUIT handling via embedded daemon framework
- Connection limiting — max 128 concurrent clients via `tokio::sync::Semaphore`
- Single-entity read methods — `read_fact`, `read_intent`, `read_hint`
- Daemon framework embedded as built-in module (`nexd::daemon`)
- 32 integration tests (27 nexd + 5 daemon framework) + 7 CI verification scenarios

### IPC Protocol

Line-delimited JSON-RPC over Unix domain socket (`/tmp/nexd.sock` by default).

| Method | Description |
|--------|-------------|
| `write_fact` / `read_fact` | Submit / read Fact |
| `read_state` | Full board dump |
| `write_intent` / `claim_intent` / `heartbeat_intent` / `release_intent` / `conclude_intent` | Intent lifecycle |
| `write_hint` | Submit Hint |
| `spawn_agent` / `list_agents` / `kill_agent` | Process management |

### Architecture

```text
                External Agents (nex-zed, actus, scripts)
                    │  line-delimited JSON over Unix socket
                    ▼
+----------------------------------------------------+
|                  nexd (daemon)                      |
|  +---------------+  +----------+  +--------------+  |
|  | IPC Server     |  | Scheduler|  | Process     |  |
|  | (JSON-RPC)     |  | (OODA)   |  | Manager     |  |
|  +-------+-------+  +----+-----+  +------+-------+  |
|          │               │               │          |
|  +-------+---------------+-------------+ |          |
|  |    HybridBlackboard (embedded)          |        |
|  |    nex::create_blackboard()           |          |
|  +--------------------------------------+          |
|  +------------------------------------------------+ |
|  |   Built-in daemon runtime (nexd::daemon)       | |
|  | (signal, shutdown, subsystem mgmt, config)     | |
|  +------------------------------------------------+ |
+----------------------------------------------------+
```

### Quickstart

```bash
cargo build --release -p nexd
./target/release/nexd                               # run with default config
./target/release/nexd actus                          # spawn actus at startup
echo '{"id":1,"method":"write_fact","params":{"origin":"test","content":"hello","creator":"alice"}}' | \
  socat - UNIX-CONNECT:/tmp/nexd.sock
```

## Dependencies

- [nex](https://crates.io/crates/nex) — FIH blackboard engine
- [nexus-model](https://crates.io/crates/nexus-model) — FIH primitives
- Tokio — async runtime
- Daemon framework is built-in (`nexd::daemon`, derived from [proc-daemon](https://github.com/jamesgober/proc-daemon))

## License

Apache 2.0
