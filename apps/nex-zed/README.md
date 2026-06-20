# nex-zed

Headless Zed client + neXus FIH bridge.

## Architecture

```
nex-queen / user
  └── ACP JSON-RPC ──→ nex-zed
                         ├── Zed remote_server (headless)
                         │   └── read / write / grep / LSP / terminal / ...
                         └── FIH ──→ neXus Blackboard ←→ nex-cf (KG)
```

Each `nex-zed` instance is an independent worker:
- Runs a headless Zed server (remote_server) as subprocess
- Listens for ACP JSON-RPC commands via stdin
- Uses all native Zed tools (LSP, Git, terminal, diagnostics, grep, ...)
- Reports progress via FIH blocks (Intent → Fact)

Multiple instances can run in parallel on the same server, each working on
a different task.

## Build

```bash
# Build Zed remote_server first
cd /path/to/zed
cargo build -p remote_server --release

# Build nex-zed
cd /path/to/nexus
cargo build -p nex-zed
```

## Usage

```bash
# Run a nex-zed worker on a project
./target/debug/nex-zed \
  --zed ../zed/target/release/zed-remote-server \
  --workdir /path/to/repo \
  --fih-socket /var/run/nexus.sock

# Or with defaults (auto-detect zed-remote-server, cwd, /var/run/nexus.sock)
./target/debug/nex-zed
```

## Worker Pool (future)

nex-queen will orchestrate multiple nex-zed instances:
- Spawn N workers per project
- Assign Intent blocks from queue
- Collect Fact results
- Route complex tasks (test → review → merge)
