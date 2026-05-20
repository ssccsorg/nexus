# playbooks — Consumer implementations and scenario agents

This directory holds real, runnable consumer implementations that demonstrate
the FIH protocol in action across different languages and privilege levels.

## Who runs the gateway server?

| Context | Owner | Command |
|---------|-------|---------|
| Local dev | Developer | `./scripts/run-gateway.sh` |
| CI | `playbooks/run.sh` | Starts/stops automatically |
| Production | systemd / Docker | `cargo run` via supervisor |

## Structure

```text
playbooks/
├── consumers/     — External agents communicating via HTTP/JSON
│   ├── python_agent.py   (pip install, then python3 ...)
│   └── node_agent.mjs    (node ...)
├── agents/        — Internal (privileged) agents with direct crate access
│   └── src/main.rs        (cd agents && cargo run)
├── run.sh         — CI orchestrator (starts server, runs all, stops)
└── README.md
```

## Usage

### 1. Start the gateway (in terminal 1)

```sh
./scripts/run-gateway.sh
```

### 2. Run a consumer (in terminal 2)

```sh
python3 playbooks/consumers/python_agent.py
node    playbooks/consumers/node_agent.mjs
```

### 3. Run privileged agent (self-contained, no server needed)

```sh
cd playbooks/agents && cargo run
```

### 4. Run all (CI mode, starts/stops server automatically)

```sh
./playbooks/run.sh
```

## Principle

- `consumers/` — External agents. Only speak HTTP/JSON. No Rust crates.
- `agents/` — Internal agents. Import nexus-graph directly. Have GraphAccess + Cypher.
