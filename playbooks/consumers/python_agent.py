#!/usr/bin/env python3
"""
Python agent: FIH Blackboard consumer via HTTP.

Demonstrates how a Python-based agent (simulating a research script,
data pipeline, or monitoring daemon) communicates with the Blackboard
through the gateway API. The agent never imports Rust crates directly
-- it only speaks HTTP/JSON.

Usage:
    pip install requests
    python gateway/api/examples/python_agent.py

Requires:
    - gateway server running on localhost:3000
      (cd gateway/api && cargo run)
"""

import json
import sys
from urllib.request import Request, urlopen
from urllib.error import HTTPError

API = "http://localhost:3000/api/v1/fih"


def post(path, body):
    """POST JSON to path, return parsed response."""
    url = f"{API}{path}"
    data = json.dumps(body).encode()
    req = Request(url, data=data, headers={"Content-Type": "application/json"})
    try:
        with urlopen(req) as resp:
            return json.loads(resp.read())
    except HTTPError as e:
        return {"_status": e.code, "_error": e.read().decode()}


def get(path):
    """GET path, return parsed response."""
    url = f"{API}{path}"
    with urlopen(url) as resp:
        return json.loads(resp.read())


def main():
    print("=== Python Agent: FIH Lifecycle via HTTP ===")

    # Step 1: Submit a fact (e.g., sensor reading or research observation)
    print("\n1. Submitting fact...")
    result = post("/facts", {
        "origin": "python-sensor",
        "content": {"temperature": 42.5, "unit": "C", "sector": 7},
        "creator": "py-agent"
    })
    fact_id = result["id"]
    print(f"   Fact ID: {fact_id}")

    # Step 2: Read state to confirm
    state = get("/state")
    print(f"   Facts in board: {len(state['facts'])}")

    # Step 3: Submit an intent grounded in the fact
    print("\n2. Submitting intent...")
    result = post("/intents", {
        "from_facts": [fact_id],
        "description": "Analyze temperature anomaly in sector 7",
        "creator": "py-agent"
    })
    intent_id = result["id"]
    print(f"   Intent ID: {intent_id}")

    # Step 4: Claim the intent
    print("\n3. Claiming intent...")
    post(f"/intents/{intent_id}/claim", {"agent": "py-agent"})
    print("   Claimed")

    # Step 5: Heartbeat
    print("\n4. Heartbeat...")
    post(f"/intents/{intent_id}/heartbeat", {"agent": "py-agent"})
    print("   OK")

    # Step 6: Conclude
    print("\n5. Concluding intent...")
    result = post(f"/intents/{intent_id}/conclude", {
        "result": {"finding": "Temperature spike due to cooling failure in sector 7", "severity": "high"}
    })
    new_fact_id = result["fact"]["id"]
    print(f"   New fact: {new_fact_id}")

    # Step 7: Read hints
    print("\n6. Submitting hint...")
    post("/hints", {
        "content": "Priority: inspect cooling system in sectors 5-9",
        "creator": "human-operator"
    })

    # Step 8: Final state
    state = get("/state")
    print(f"\n=== Final Board State ===")
    print(f"   Facts:   {len(state['facts'])}")
    print(f"   Intents: {len(state['intents'])}")
    print(f"   Hints:   {len(state['hints'])}")
    print("\nPython agent: FIH lifecycle complete via HTTP/JSON")


if __name__ == "__main__":
    main()
