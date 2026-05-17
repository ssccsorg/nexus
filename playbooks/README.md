# playbooks — Consumer implementations and scenario agents

This directory holds real, runnable consumer implementations that demonstrate
the FIH protocol in action across different languages and privilege levels.

## Structure

```
playbooks/
├── consumers/     — External agents communicating via HTTP/JSON
│   ├── python_agent.py   (pip install, then python3 ...)
│   └── node_agent.mjs    (node ...)
└── agents/        — Internal (privileged) agents with direct crate access
    └── src/main.rs        (cd agents && cargo run)
```

## Usage

### Python consumer
```sh
# Start gateway server
cd gateway/api && cargo run

# In another terminal:
python3 playbooks/consumers/python_agent.py
```

### Node.js consumer
```sh
node playbooks/consumers/node_agent.mjs
```

### Rust privileged agent
```sh
cd playbooks/agents && cargo run
```

## Principle

- `consumers/` — External agents. Only speak HTTP/JSON. No Rust crates.
- `agents/` — Internal agents. Import nexus-graph directly. Have GraphAccess + Cypher.
