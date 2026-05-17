#!/usr/bin/env bash
set -euo pipefail
#
# playbooks/run.sh — Run all consumer playbooks and verify output.
#
# Starts the gateway server, runs each consumer implementation,
# then stops the server. Each consumer prints its lifecycle trace.
#
# Usage:
#   ./playbooks/run.sh               # Run all playbooks
#   ./playbooks/run.sh --consumers   # Only HTTP consumers (Python + Node)
#   ./playbooks/run.sh --agents      # Only Rust privileged agent
#

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PASSED=0
FAILED=0

run_consumer() {
    local name="$1"
    local cmd="$2"
    echo ""
    echo "═══════════════════════════════════════════════════"
    echo "  [$name]"
    echo "═══════════════════════════════════════════════════"
    if eval "$cmd"; then
        echo "  ✓ $name: PASSED"
        PASSED=$((PASSED + 1))
    else
        echo "  ✗ $name: FAILED (exit code $?)"
        FAILED=$((FAILED + 1))
    fi
}

cleanup() {
    echo ""
    echo "=== Stopping gateway server ==="
    if [ -n "${SERVER_PID:-}" ]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

MODE="${1:-all}"

# ── Start gateway server ────────────────────────────────────────────

if [ "$MODE" = "all" ] || [ "$MODE" = "--consumers" ]; then
    echo "=== Starting gateway server ==="
    "$SCRIPT_DIR/../scripts/run-gateway.sh" &
    SERVER_PID=$!
    sleep 2
    echo "Server PID: $SERVER_PID"

    if curl -sf http://localhost:3000/api/v1/fih/state > /dev/null 2>&1; then
        echo "  Server ready at http://localhost:3000"
    else
        echo "  ERROR: Server failed to start"
        exit 1
    fi
fi

# ── Run consumers ───────────────────────────────────────────────────

if [ "$MODE" = "all" ] || [ "$MODE" = "--consumers" ]; then
    echo ""
    echo "═══ HTTP Consumers (external agents) ═══"

    if command -v python3 &>/dev/null; then
        run_consumer "Python Agent" "python3 '$SCRIPT_DIR/consumers/python_agent.py' 2>&1"
    else
        echo "  SKIP: python3 not found"
    fi

    if command -v node &>/dev/null; then
        run_consumer "Node.js Agent" "node '$SCRIPT_DIR/consumers/node_agent.mjs' 2>&1"
    else
        echo "  SKIP: node not found"
    fi
fi

if [ "$MODE" = "all" ] || [ "$MODE" = "--agents" ]; then
    echo ""
    echo "═══ Rust Privileged Agent ═══"
    run_consumer "Rust Agent" "cd '$SCRIPT_DIR/agents' && cargo run --quiet 2>&1"
fi

# ── Summary ─────────────────────────────────────────────────────────

echo ""
echo "═══════════════════════════════════════════════════"
echo "  Playbook Results: $PASSED passed, $FAILED failed"
echo "═══════════════════════════════════════════════════"

[ "$FAILED" -eq 0 ]
