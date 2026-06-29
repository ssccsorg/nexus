# nexd — Unified daemon for the nex ecosystem

`nexd` is the persistent runtime that maintains the FIH blackboard (Fact/Intent/Hint) and orchestrates the execution of all `nex-*` applications. It provides shared memory, process management, and IPC for a swarm of autonomous agents.

Conceptual reference: **nexd is to nex-* as iOS is to apps.**

## Quickstart

Build and run `nexd`:

```bash
cargo build --release -p nexd
./target/release/nexd
```

Connect and write a Fact using `socat`:

```bash
echo '{"id":1,"method":"write_fact","params":{"origin":"test","content":"Hello nexd","creator":"alice"}}' | \
  socat - UNIX-CONNECT:/tmp/nexd.sock
```

Read the full board state:

```bash
echo '{"id":2,"method":"read_state","params":{}}' | \
  socat - UNIX-CONNECT:/tmp/nexd.sock
```

## Usage

```text
nexd                          # run with default config
nexd actus                    # spawn actus at startup
nexd ./my-agent --flag value  # spawn custom agent
```

### Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NEXD_SOCKET_PATH` | `/tmp/nexd.sock` | Unix domain socket path |
| `NEXD_TICK_INTERVAL_MS` | `100` | Scheduler tick interval |
| `NEXD_HEARTBEAT_TTL_SECS` | `60` | Heartbeat timeout for claimed intents |
| `NEXD_UNCLAIMED_INTENT_TTL_SECS` | `3600` | Stale intent eviction timeout |
| `RUST_LOG` | `nexd=info` | Log level filter |

## IPC Protocol

Line-delimited JSON-RPC 2.0 over Unix domain socket.

### Methods

| Method | Params | Description |
|--------|--------|-------------|
| `write_fact` | `{origin, content, creator}` | Submit a new Fact |
| `read_state` | `{}` | Read full board state |
| `write_intent` | `{from_facts, description, creator}` | Submit a new Intent |
| `claim_intent` | `{id, agent}` | Claim an Intent for processing |
| `heartbeat_intent` | `{id, agent}` | Heartbeat for a claimed Intent |
| `release_intent` | `{id, agent}` | Release a claimed Intent |
| `conclude_intent` | `{id, result}` | Conclude an Intent with a result |
| `write_hint` | `{content, creator}` | Submit a new Hint |
| `spawn_agent` | `{command, args}` | Spawn a child process |
| `list_agents` | `{}` | List managed child processes |
| `kill_agent` | `{pid}` | Kill a child process |

### Request format

```json
{"id":1,"method":"write_fact","params":{"origin":"test","content":"hello","creator":"alice"}}
```

### Success response

```json
{"id":1,"result":{"id":"abc123..."}}
```

### Error response

```json
{"id":1,"error":{"code":-32000,"message":"not found"}}
```

## Python client example

```python
import socket, json

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect("/tmp/nexd.sock")

def rpc(method, params={}):
    req = json.dumps({"id": 1, "method": method, "params": params})
    sock.sendall((req + "\n").encode())
    resp = sock.recv(4096).decode()
    return json.loads(resp)

# Write a fact
result = rpc("write_fact", {"origin":"test","content":"Hello from Python","creator":"py"})
print("Fact ID:", result["result"]["id"])

# Read state
state = rpc("read_state")
print(f"Facts: {len(state['result']['facts'])}, Intents: {len(state['result']['intents'])}")

sock.close()
```

## Architecture

```text
┌──────────────────────────────────────────────────┐
│                    nexd                           │
├──────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌────────────────┐  │
│  │ Blackboard│  │  Process  │  │  IPC Server    │  │
│  │ (nex::Hy- │  │  Manager  │  │ (Unix socket)  │  │
│  │ bridBlack-│  │          │  │                │  │
│  │ board)    │  │          │  │                │  │
│  └──────────┘  └──────────┘  └────────────────┘  │
│  ┌──────────────────────────────────────────────┐ │
│  │       proc-daemon framework                  │ │
│  │  (SubsystemManager, ShutdownHandle, Config)  │ │
│  └──────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────┘
```

## Project structure

```text
nexd/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs      # Entry point — builds and runs the daemon
    ├── lib.rs       # Library exports for testing
    ├── config.rs    # Daemon configuration
    ├── handler.rs   # JSON-RPC method dispatch
    ├── manager.rs   # Child process lifecycle management
    └── server.rs    # Unix socket listener
```

## Dependencies

- [proc-daemon](https://github.com/jamesgober/proc-daemon) — production daemon framework
- [nex](https://crates.io/crates/nex) — FIH blackboard storage engine
- [nexus-model](https://crates.io/crates/nexus-model) — FIH primitives (Fact, Intent, Hint)
- Tokio — async runtime

## License

Apache 2.0
