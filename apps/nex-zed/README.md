# nex-zed

A neXus instance embedding ACP (Agent Client Protocol) as one of its
communication surfaces for the [Zed editor](https://zed.dev).

## Architecture

```
Zed Editor
  └── spawns child process (ACP stdio)
       └── nex-zed (agentic neXus instance)
            ├── ACP surface (inbound from Zed)
            ├── ACP surface (outbound to Zed)
            └── FIH surface (neXus blackboard)
```

This is a neXus instance — not a bridge, not an orchestrator. It is
one of many peers on the blackboard whose ACP surface happens to face Zed.

## Build

```bash
cargo build -p nex-zed
```

## Usage

Register in Zed settings:

```json
{
  "agent_servers": {
    "nexus-zed-dev": {
      "command": {
        "command": "/path/to/nex-zed/target/debug/nex-zed"
      }
    }
  }
}
```

In Zed, open command palette and select "agent: select" → "nexus-zed-dev".

## CLI

```
Usage: nex-zed [OPTIONS]

Options:
      --nexus-socket <PATH>  Path to neXus daemon Unix socket [default: /var/run/nexus.sock]
  -v, --verbose              Enable verbose logging
      --log-level <LEVEL>    Log level filter [default: info]
  -h, --help                 Print help
  -V, --version              Print version
```

## Implementation Status

- [x] Phase 1: ACP stdio server (echo mode)
- [ ] Phase 2: FIH blackboard connection
- [ ] Phase 3: Bidirectional tool forwarding
- [ ] Phase 4: Deployment packaging

## References

- Design document: https://docs.ssccs.org/projects/nexus/apps/zed.llms.md
- GitHub issue: https://github.com/ssccsorg/nexus/issues/72
