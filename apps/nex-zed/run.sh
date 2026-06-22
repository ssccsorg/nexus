#!/usr/bin/env bash
# run.sh — nex-zed test suite
#
# Categories:
#   run.sh                  # default: WebSocket handshake test (quick)
#   run.sh --apps           # full nex-zed validation (binary + WS + agent init)
#   run.sh --ws-only        # WebSocket handshake only (no Zed)
#   run.sh --conn-check     # start Zed, wait for WebSocket connect, verify, cleanup
#   run.sh --help           # this help
#
# Prerequisites:
#   .bin/helix-zed-headless-arm64 — pre-built binary
#   apps/nex-zed/chat.py          — WebSocket server
#
# Environment:
#   DEEPSEEK_API_KEY  — set for --apps test that verifies agent_ready + model init

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN="$PROJECT_DIR/.bin/helix-zed-headless-arm64"
CHAT_PY="$SCRIPT_DIR/chat.py"
LOG="/tmp/nex-zed-headless.log"
WS_LOG="/tmp/nex-zed-websocket.log"

# ── Colors ────────────────────────────────────────────────────────────
PASS="\033[92m✓\033[0m"
FAIL="\033[91m✗\033[0m"
INFO="\033[96m==>\033[0m"
WARN="\033[93m⚠\033[0m"
BOLD="\033[1m"
DIM="\033[2m"
END="\033[0m"

# ── Helpers ───────────────────────────────────────────────────────────

pass() { echo -e "  ${PASS} $*"; }
fail() { echo -e "  ${FAIL} $*"; exit 1; }
info() { echo -e "${INFO} $*"; }
warn() { echo -e "${WARN} $*" >&2; }
step() { echo -e "\n${INFO} ${BOLD}$*${END}"; }

cleanup() {
    pkill -f "helix-zed-headless" 2>/dev/null || true
    pkill -f "chat.py" 2>/dev/null || true
    sleep 1
}

check_binary() {
    if [ ! -f "$BIN" ]; then
        fail "Binary not found at $BIN"
    fi
    BIN_SIZE=$(ls -lh "$BIN" | awk '{print $5}')
    pass "Binary found: $BIN ($BIN_SIZE)"
}

# ── Test: WebSocket handshake only ───────────────────────────────────

test_ws_handshake() {
    step "Test: WebSocket handshake"

    # Start chat.py as WebSocket server only
    python3 "$CHAT_PY" --no-zed --api-key "test-dummy" > "$WS_LOG" 2>&1 &
    WS_PID=$!
    sleep 2

    if ! kill -0 $WS_PID 2>/dev/null; then
        fail "WebSocket server failed to start"
    fi
    pass "WebSocket server started (PID: $WS_PID)"

    # Verify port is listening
    if lsof -iTCP:8080 -sTCP:LISTEN 2>/dev/null | grep -q LISTEN; then
        pass "Port 8080 is listening"
    else
        fail "Port 8080 not listening"
    fi

    kill $WS_PID 2>/dev/null || true
    pass "WebSocket server stopped cleanly"
}

# ── Test: Binary integrity ───────────────────────────────────────────

test_binary() {
    step "Test: Binary integrity"

    check_binary

    # Check it's an executable Mach-O binary
    FILE_TYPE=$(file "$BIN" 2>/dev/null || echo "")
    if echo "$FILE_TYPE" | grep -q "Mach-O"; then
        pass "Binary is valid Mach-O executable"
    else
        warn "Binary type: $FILE_TYPE"
        pass "Binary exists and is executable"
    fi
}

# ── Test: Connection check (start Zed, wait for WS connect, verify, stop) ──

test_connection() {
    step "Test: Zed WebSocket connection"

    check_binary

    cleanup

    # Start WebSocket server
    python3 "$CHAT_PY" --no-zed --api-key "test-dummy" > "$WS_LOG" 2>&1 &
    WS_PID=$!
    sleep 2

    if ! kill -0 $WS_PID 2>/dev/null; then
        fail "WebSocket server failed to start"
    fi
    pass "WebSocket server started (PID: $WS_PID)"

    # Clean log
    rm -f "$LOG"

    # Start Zed headless
    export ZED_EXTERNAL_SYNC_ENABLED=true
    export ZED_WEBSOCKET_SYNC_ENABLED=true
    export ZED_HELIX_URL="127.0.0.1:8080"
    export ZED_HELIX_TOKEN="test-token"
    export HELIX_SESSION_ID="ses_nex-zed-test-001"
    export ZED_STATELESS=1

    "$BIN" --headless --allow-multiple-instances > "$LOG" 2>&1 &
    ZED_PID=$!
    pass "Zed started (PID: $ZED_PID)"

    # Wait for connection
    echo -n "  Waiting 15s for WebSocket connect..."
    for i in $(seq 1 15); do
        sleep 1
        echo -n "."
        if grep -q "WebSocket connected" "$LOG" 2>/dev/null; then
            echo ""
            pass "WebSocket connected (HTTP 101)"
            break
        fi
        if ! kill -0 $ZED_PID 2>/dev/null; then
            echo ""
            fail "Zed process died during startup"
        fi
    done

    # Verify connection
    if grep -q "WebSocket connected" "$LOG" 2>/dev/null; then
        pass "WebSocket handshake verified"
    else
        echo ""
        warn "Zed log tail:"
        tail -10 "$LOG"
        fail "WebSocket connection not established within 15s"
    fi

    # Check ping/pong
    if grep -q "Sent test ping" "$LOG" 2>/dev/null; then
        pass "Ping sent"
    fi

    # Cleanup
    kill $ZED_PID 2>/dev/null || true
    kill $WS_PID 2>/dev/null || true
    pass "Processes cleaned up"
}

# ── Test: Full apps validation ────────────────────────────────────────

test_apps() {
    step "Test: Full nex-zed validation"

    # Need API key for this test
    if [ -z "${DEEPSEEK_API_KEY:-${LLM_API_KEY:-}}" ]; then
        warn "No API key set — skipping agent initialization check"
        info "Set DEEPSEEK_API_KEY or LLM_API_KEY to verify agent_ready + model init"
    fi

    check_binary
    cleanup

    # Run nex-zed in no-chat mode (server only)
    "$SCRIPT_DIR/nex-zed" --workdir "$PROJECT_DIR" --no-build --api-key "test-dummy" --no-chat > "$WS_LOG" 2>&1 &
    NEX_PID=$!
    sleep 2

    if ! kill -0 $NEX_PID 2>/dev/null; then
        fail "nex-zed failed to start"
    fi
    pass "nex-zed started (PID: $NEX_PID)"

    # Wait for WebSocket connect
    echo -n "  Waiting 20s for Zed to initialize..."
    for i in $(seq 1 20); do
        sleep 1
        echo -n "."
        if grep -q "WebSocket connected" "$LOG" 2>/dev/null; then
            echo ""
            pass "Zed connected to WebSocket"
            break
        fi
    done

    if ! grep -q "WebSocket connected" "$LOG" 2>/dev/null; then
        echo ""
        warn "Zed log:"
        tail -15 "$LOG"
        fail "Zed did not connect within 20s"
    fi

    # Check agent_ready if API key is available
    if [ -n "${DEEPSEEK_API_KEY:-${LLM_API_KEY:-}}" ]; then
        echo -n "  Waiting 10s for agent_ready..."
        for i in $(seq 1 10); do
            sleep 1
            echo -n "."
            if grep -q "agent_ready" "$LOG" 2>/dev/null; then
                echo ""
                pass "Agent ready signal received"
                break
            fi
        done
    fi

    # Cleanup
    kill $NEX_PID 2>/dev/null || true
    cleanup
    pass "Cleanup complete"
}

# ── Main ──────────────────────────────────────────────────────────────

show_help() {
    cat <<EOF
Usage: $0 [OPTIONS]

nex-zed test suite

Categories:
  (default)       WebSocket handshake test (quick, no Zed)
  --apps          Full nex-zed validation (binary + WS + agent init)
  --ws-only       WebSocket server test only
  --conn-check    Start Zed, wait for WS connect, verify, cleanup
  --help          Show this help
EOF
    exit 0
}

MODE="${1:-default}"

case "$MODE" in
    --apps)
        test_binary
        test_ws_handshake
        test_connection
        test_apps
        ;;
    --ws-only)
        test_ws_handshake
        ;;
    --conn-check)
        test_binary
        test_connection
        ;;
    --help|-h)
        show_help
        ;;
    *)
        test_ws_handshake
        ;;
esac

echo ""
info "${BOLD}All tests passed.${END}"
