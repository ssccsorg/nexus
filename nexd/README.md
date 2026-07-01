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
| Deployment | Native daemon (macOS, Linux), embedded (Rem) | Library crate, standalone server binary (future) |

## Current Status (Phase 1 MVP)

> **Note**: Phase 1 embeds `nex` as a Rust crate at compile time (`nex::create_blackboard()`). This is a pragmatic shortcut. The target architecture separates `nex` as an independent process — see Roadmap.

### What works

- Unix domain socket IPC with JSON-RPC style protocol (11 methods)
- FIH blackboard service via embedded `HybridBlackboard` (same as `apps/nex-api`)
- Process Manager — spawn, monitor (`try_reap`), kill child `nex-*` agents
- OODA scheduler — heartbeat TTL monitoring, stale intent eviction
- Graceful shutdown — SIGTERM/SIGINT/SIGQUIT handling via `proc-daemon`
- Connection limiting — max 128 concurrent clients via `tokio::sync::Semaphore`
- Single-entity read methods — `read_fact`, `read_intent`, `read_hint`
- 27 integration tests + 7 CI verification scenarios

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
|  |    HybridBlackboard (Phase 1: embedded) |        |
|  |    nex::create_blackboard()           |          |
|  +--------------------------------------+          |
|  +--------------------------------------+          |
|  |   proc-daemon framework              |          |
|  | (signal, shutdown, subsystem mgmt)   |          |
|  +--------------------------------------+          |
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

## Roadmap

### Phase 2: nex as independent blackboard server (tracked in #138)

Extract `nex` from embedded crate to standalone process:

```
nexd (daemon)                              nex (standalone server)
  ├── process manager (spawn nex)               └── FIH blackboard
  ├── IPC router (socket ─► nex)                └── storage layer
  ├── no nex crate dependency                    └── Unix socket server
  └── knows only wire protocol
```

- `nexd` loses `nex` crate dependency. All FIH knowledge removed from nexd source.
- `nex` becomes a standalone binary with its own Unix socket.
- `nexd` spawns and manages the `nex` process.
- Wire protocol becomes the **contract** between the two layers.

### Phase 3: proc-daemon → built-in daemon runtime (#138)

- Copy proc-daemon core source into `nexd/src/daemon/`.
- Strip unnecessary modules (memory pools, metrics, profiling, crossbeam, dashmap).
- Remove ~70 transitive dependencies.
- Iteratively refactor toward nexd-optimized minimal daemon runtime.
- Target: sub-5MB static binary for embedded deployment.

### Phase 4: Embedded / Rem support

- USB Gadget Mode (Mass Storage + CDC ACM).
- OTA update mechanism.
- Power management (battery, USB suspend).
- P2P network sync between Rem devices.

### Phase 5: nexd as orchestration hub

- Manage multiple `nex` instances (sharding, replication).
- Route IPC between remote daemons (multi-node FIH sync).
- Health dashboard and metrics.

## Issue Map

| Issue | Title | Status |
|-------|-------|--------|
| #135 | nexd Phase 1 MVP | ✅ Merged |
| #138 | nex as isolated server, nexd as pure OS | 🔲 Epic (next) |
| #137 | proc-daemon → built-in rt.rs | 🔲 Branch exists |
| #139 | FIH coordinate system formalization | 🔲 Epic (nex side) |

## Dependencies

- [proc-daemon](https://github.com/jamesgober/proc-daemon) — daemon framework (to be replaced in Phase 3)
- [nex](https://crates.io/crates/nex) — FIH blackboard engine (to be extracted in Phase 2)
- [nexus-model](https://crates.io/crates/nexus-model) — FIH primitives
- Tokio — async runtime

## License

Apache 2.0
