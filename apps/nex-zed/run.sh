#!/usr/bin/env bash
# nex-zed: Helix headless Zed WebSocket integration test
#
# 1. Starts a WebSocket test server (acts as Helix API)
# 2. Launches Helix Zed in --headless mode
# 3. Zed connects to the WebSocket server automatically
# 4. Shows interactive prompt for sending commands to Zed
#
# Usage: ./apps/nex-zed/run.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN="/Users/blackgene/Documents/ssccs-nexus/.bin/helix-zed-headless-arm64"
WS_SERVER="$SCRIPT_DIR/ws_test_server.py"
LOG="/tmp/nex-zed-headless.log"

echo "=== nex-zed: Helix headless Zed WebSocket test ==="
echo "Binary: $BIN ($(ls -lh "$BIN" | awk '{print $5}'))"
echo ""

# Kill any existing processes
pkill -f "helix-zed-headless" 2>/dev/null || true
sleep 1

# Start WebSocket test server in background
echo "[1/3] Starting WebSocket test server on ws://127.0.0.1:8080 ..."
python3 "$WS_SERVER" &
WS_PID=$!
sleep 2

# Check server is up
if ! kill -0 $WS_PID 2>/dev/null; then
    echo "FAIL: WebSocket server died"
    exit 1
fi
echo "      PID: $WS_PID"
echo ""

# Start headless Zed
echo "[2/3] Starting Helix headless Zed..."
export ZED_EXTERNAL_SYNC_ENABLED=true
export ZED_WEBSOCKET_SYNC_ENABLED=true
export ZED_HELIX_URL="127.0.0.1:8080"
export ZED_HELIX_TOKEN="test-token"
export HELIX_SESSION_ID="ses_nex-zed-test-001"
export ZED_STATELESS=1
export RUST_LOG=info

"$BIN" --headless --allow-multiple-instances > "$LOG" 2>&1 &
ZED_PID=$!
echo "      PID: $ZED_PID"
echo ""

# Wait for startup
echo "[3/3] Waiting 15s for Zed to initialize and connect..."
sleep 15

# Check if process is alive
if ! kill -0 $ZED_PID 2>/dev/null; then
    echo "FAIL: Zed process died"
    echo "=== Last 30 lines of log ==="
    tail -30 "$LOG"
    echo ""
    echo "=== Listening ports ==="
    lsof -iTCP -sTCP:LISTEN -P 2>/dev/null | head -20 || true
    kill $WS_PID 2>/dev/null || true
    exit 1
fi

echo ""
echo "=== Zed is running (PID: $ZED_PID) ==="
echo "=== WebSocket test server is running (PID: $WS_PID) ==="
echo ""
echo "Check the WebSocket server output above for connection status."
echo "WebSocket server is in interactive mode \u2014 type commands there."
echo ""
echo "To stop: kill $ZED_PID $WS_PID"
echo ""

# Wait for either process to exit
wait $ZED_PID 2>/dev/null
echo "Zed process exited."

# Cleanup
kill $WS_PID 2>/dev/null || true
echo "Done."
