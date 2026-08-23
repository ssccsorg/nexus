# Wire protocol: nexd and nex-server JSON-RPC

The nexd daemon and the nex-server blackboard process communicate over
Unix domain sockets using line-delimited JSON-RPC 2.0. This document is
the contract for that surface (issue #138, Phase 2a).

## Transport

Both endpoints listen on Unix domain sockets and exchange one JSON
object per line.

| Endpoint | Socket | Default path | Environment |
|----------|--------|--------------|-------------|
| nex-server (blackboard) | `NEX_SOCKET_PATH` | `/tmp/nex-server.sock` | set by the spawning nexd |
| nexd (supervisor) | `NEXD_SOCKET_PATH` | `/tmp/nexd.sock` | set by the operator |

nexd spawns nex-server as a managed child, waits for the nex-server
socket, and forwards FIH calls through the nex-client SDK. nexd
supervises nex-server: a crashed instance is respawned with its original
command, and shutdown sends SIGTERM with a bounded wait before force
kill (issue #146).

## Message shape

Messages follow JSON-RPC 2.0 semantics (id, result, error, standard
codes) but elide the `jsonrpc` version field for line economy:

```json
{"id": 1, "method": "read_state", "params": {}}
{"id": 1, "result": {...}}
{"id": 1, "error": {"code": -32001, "message": "not found: ..."}}
```

Notification-only requests (no `id`) are not used by nexd or
nex-server; every request receives a response.

## Error codes

| Code | Meaning |
|------|---------|
| `-32602` | Invalid params (JSON parse or required field missing) |
| `-32601` | Method not found |
| `-32000` | Internal error |
| `-32001` | Not found |
| `-32002` | Conflict (for example duplicate fact id with different content hash) |
| `-32003` | Forbidden (for example intent without a fact reference) |

## nex-server methods

The blackboard surface. `id` values are strings (canonical CoordId).

| Method | Params | Result |
|--------|--------|--------|
| `write_fact` | `{origin, content, creator}` | `{"id": "<coord-id>"}` |
| `read_state` | `{}` | `{facts: [...], intents: [...], hints: [...]}` |
| `read_state_structure` | `{}` | state structure with empty content and descriptions |
| `read_fact` | `{id}` | fact object |
| `read_intent` | `{id}` | intent object |
| `read_hint` | `{id}` | hint object |
| `write_intent` | `{from_facts: [id], description, creator}` | `{"id": "<coord-id>"}` |
| `claim_intent` | `{id, agent}` | `"ok"` |
| `heartbeat_intent` | `{id, agent}` | `"ok"` |
| `release_intent` | `{id, agent}` | `"ok"` |
| `conclude_intent` | `{id, result}` | `{"fact": {...}}` |
| `write_hint` | `{id, content, creator}` | `"ok"` |

`content` and `result` accept either a JSON string (stored as
`text/plain`) or any JSON value (stored as `application/json`).

## nexd methods

nexd serves its own socket. FIH methods are forwarded verbatim to
nex-server through nex-client; the supervision methods are handled
locally.

| Method | Params | Result |
|--------|--------|--------|
| FIH methods | as nex-server above | forwarded |
| `spawn_agent` | `{command, args?}` | `{"pid": n, "command": "..."}` |
| `list_agents` | `{}` | `{"agents": [{"pid": n, "command": "..."}]}` |
| `kill_agent` | `{pid}` | `"ok"` |

`list_agents` includes the supervised nex-server child alongside any
spawned agents.

## Stability

This contract is the boundary that lets nexd remain a pure supervisor
with no compile-time dependency on the blackboard engine. Changes to a
method name, parameter, or error code are breaking and must update this
document together with the nex-client SDK.
