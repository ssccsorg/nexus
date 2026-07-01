#!/usr/bin/env python3
"""
nexd Python client example.

Connects to nexd's Unix domain socket, writes a Fact, reads the board state,
and spawns a test agent.

Usage:
    python examples/client.py [socket_path]
"""

import json
import socket
import sys

SOCKET_PATH = "/tmp/nexd.sock"


class NexdClient:
    """Simple JSON-RPC client for nexd."""

    def __init__(self, socket_path: str = SOCKET_PATH):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(socket_path)
        self._req_id = 0

    def call(self, method: str, params: dict = None) -> dict:
        self._req_id += 1
        req = json.dumps({
            "id": self._req_id,
            "method": method,
            "params": params or {},
        })
        self.sock.sendall((req + "\n").encode())
        resp = self.sock.recv(65536).decode()
        return json.loads(resp)

    def close(self):
        self.sock.close()


def main():
    socket_path = sys.argv[1] if len(sys.argv) > 1 else SOCKET_PATH
    client = NexdClient(socket_path)

    # 1. Write a Fact
    print("=== write_fact ===")
    result = client.call("write_fact", {
        "origin": "python-client",
        "content": "Hello from nex ecosystem",
        "creator": "demo",
    })
    print(json.dumps(result, indent=2))
    fact_id = result["result"]["id"]

    # 2. Write an Intent referencing the Fact
    print("\n=== write_intent ===")
    result = client.call("write_intent", {
        "from_facts": [fact_id],
        "description": "Process the greeting fact",
        "creator": "demo",
    })
    print(json.dumps(result, indent=2))
    intent_id = result["result"]["id"]

    # 3. Read board state
    print("\n=== read_state ===")
    result = client.call("read_state")
    state = result["result"]
    print(f"  Facts: {len(state['facts'])}")
    print(f"  Intents: {len(state['intents'])}")
    print(f"  Hints: {len(state['hints'])}")

    # 4. Write a Hint
    print("\n=== write_hint ===")
    result = client.call("write_hint", {
        "content": "This is a hint for agents",
        "creator": "demo",
    })
    print(json.dumps(result, indent=2))

    # 5. Spawn a test agent (echo)
    print("\n=== spawn_agent ===")
    result = client.call("spawn_agent", {
        "command": "echo",
        "args": ["nexd agent test"],
    })
    print(json.dumps(result, indent=2))

    # 6. List agents
    print("\n=== list_agents ===")
    result = client.call("list_agents")
    print(json.dumps(result, indent=2))

    client.close()
    print("\nDone.")


if __name__ == "__main__":
    main()
