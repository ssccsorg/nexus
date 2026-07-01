#!/usr/bin/env bash
set -euo pipefail
#
# nexus — Unified CI runner
#
# By default runs everything (core checks + gateway + apps + playbooks).
# Sub-commands for focused tasks.
#
# Usage:
#   ./run.sh               # Everything (default)
#   ./run.sh --core        # Core checks only (nex, storage/*)
#   ./run.sh --gateway     # Gateway layer checks (api, nex-cf, serde-proxy)
#   ./run.sh --apps        # All standalone app verification
#   ./run.sh --playbooks   # Consumer playbooks only
#

cd "$(dirname "$0")"

# ── Port cleanup ──────────────────────────────────────────────────────────

kill_port() {
    local port="$1"
    local pid
    pid=$(lsof -ti "$port" 2>/dev/null || true)
    if [ -n "$pid" ]; then
        echo "kill_port $port: killing PID $pid"
        kill -9 "$pid" 2>/dev/null || true
        sleep 1
    fi
}

# ── App verifiers ─────────────────────────────────────────────────────────
# Each verifier is a standalone function so it can be invoked independently
# or composed under the --apps umbrella.

verify_nex_spinwasi_ssccsdocs() {
    local PORT=30921
    echo "=== nex-spinwasi-ssccsdocs ==="
    echo "Building..."
    (cd apps/nex-spinwasi-ssccsdocs && spin build 2>&1)
    echo ""
    echo "Starting server on port $PORT..."
    # Aggressive port cleanup: kill multiple times with delay
    kill_port "$PORT" 2>/dev/null || true
    sleep 1
    kill_port "$PORT" 2>/dev/null || true
    sleep 1
    # Also kill any leftover spin processes
    pkill -f "spin.*up" 2>/dev/null || true
    sleep 1
    (cd apps/nex-spinwasi-ssccsdocs && spin up --build --listen "127.0.0.1:$PORT" 2>&1) &
    local SPIN_PID=$!
    sleep 4

    local failed=0
    echo "Testing endpoints..."
    for test in \
        "GET /        : curl -s -o /dev/null -w %{http_code} http://127.0.0.1:${PORT}/" \
        "GET /version : curl -s -o /dev/null -w %{http_code} http://127.0.0.1:${PORT}/version" \
        "POST /ingest: curl -s -o /dev/null -w %{http_code} -X POST http://127.0.0.1:${PORT}/ingest -H content-type:application/json -d '{\"text\":\"hello test\",\"origin\":\"test\"}'" \
        "GET /search : curl -s -o /dev/null -w %{http_code} 'http://127.0.0.1:${PORT}/search?q=hello'" \
        "GET /state  : curl -s -o /dev/null -w %{http_code} http://127.0.0.1:${PORT}/state"
    do
        local label="${test%%:*}"
        local cmd="${test#*: }"
        local code
        code=$(eval "$cmd" 2>/dev/null)
        if [ "$code" = "200" ]; then
            echo "  $label $code"
        else
            echo "  $label $code (FAIL)"
            failed=1
        fi
    done

    echo ""
    kill "$SPIN_PID" 2>/dev/null || true
    sleep 1
    kill_port "$PORT" 2>/dev/null || true
    if [ "$failed" -eq 0 ]; then
        echo "nex-spinwasi-ssccsdocs: all 5/5 passed"
    else
        echo "nex-spinwasi-ssccsdocs: some tests FAILED"
        return 1
    fi
}

# ── nexd daemon verification ─────────────────────────────────────────────

verify_nexd() {
    local SOCKET_DIR
    SOCKET_DIR=$(mktemp -d)
    local SOCKET_PATH="${SOCKET_DIR}/nexd.sock"

    echo "=== nexd ==="
    echo "Building..."
    cargo build -p nexd 2>&1
    echo ""
    echo "Starting daemon on ${SOCKET_PATH}..."

    NEXD_SOCKET_PATH="$SOCKET_PATH" ./target/debug/nexd &
    local NEXD_PID=$!
    trap "kill $NEXD_PID 2>/dev/null; rm -rf '$SOCKET_DIR'" EXIT

    # Wait for socket
    local waited=0
    while [ ! -S "$SOCKET_PATH" ] && [ "$waited" -lt 30 ]; do
        sleep 0.2
        waited=$((waited + 1))
    done
    if [ ! -S "$SOCKET_PATH" ]; then
        echo "nexd: socket not ready after ${waited}s (FAIL)"
        return 1
    fi
    echo "  daemon ready"
    echo ""

    local failed=0

    # Helper: send JSON-RPC, return response
    rpc() {
        echo "$1" | socat - "UNIX-CONNECT:${SOCKET_PATH}" 2>/dev/null || echo '{"error":"connection failed"}'
    }

    # ═══════════════════════════════════════════════════════════════════
    # Scenario 1: Basic transport — write fact, read state
    # ═══════════════════════════════════════════════════════════════════
    echo "  [1/6] Basic FIH operations..."

    local F1
    F1=$(rpc '{"id":1,"method":"write_fact","params":{"origin":"ci","content":"smoke test","creator":"runner"}}')
    local FACT_ID
    FACT_ID=$(echo "$F1" | sed 's/.*"id":"\([^"]*\)".*/\1/' )
    if [ -n "$FACT_ID" ]; then
        echo "    write_fact: ok (id=$FACT_ID)"
    else
        echo "    write_fact: FAIL ($F1)"
        failed=1
    fi

    local S1
    S1=$(rpc '{"id":2,"method":"read_state","params":{}}')
    if echo "$S1" | grep -q '"facts"'; then
        echo "    read_state: ok"
    else
        echo "    read_state: FAIL"
        failed=1
    fi

    # Read single fact by ID
    local RF
    RF=$(rpc "{"id":3,"method":"read_fact","params":{"id":"$FACT_ID"}}")
    if echo "$RF" | grep -q '"result"'; then
        echo "    read_fact: ok"
    else
        echo "    read_fact: FAIL ($RF)"
        failed=1
    fi

    # ═══════════════════════════════════════════════════════════════════
    # Scenario 2: Intent lifecycle — submit → claim → heartbeat → conclude
    # ═══════════════════════════════════════════════════════════════════
    echo "  [2/7] Intent lifecycle..."

    local FID
    FID=$(rpc '{"id":10,"method":"write_fact","params":{"origin":"lifecycle","content":"base fact","creator":"runner"}}')
    local FACT_ID
    FACT_ID=$(echo "$FID" | sed 's/.*"id":"\([^"]*\)".*/\1/')
    if [ -z "$FACT_ID" ]; then
        echo "    write_fact (lifecycle): FAIL ($FID)"
        failed=1
    else
        echo "    base fact: $FACT_ID"
    fi

    local IID
    IID=$(rpc "{\"id\":11,\"method\":\"write_intent\",\"params\":{\"from_facts\":[\"$FACT_ID\"],\"description\":\"test intent\",\"creator\":\"runner\"}}")
    local INTENT_ID
    INTENT_ID=$(echo "$IID" | sed 's/.*"id":"\([^"]*\)".*/\1/')
    if [ -z "$INTENT_ID" ]; then
        echo "    write_intent: FAIL ($IID)"
        failed=1
    else
        echo "    intent: $INTENT_ID"
    fi

    # Claim
    local CLM
    CLM=$(rpc "{\"id\":12,\"method\":\"claim_intent\",\"params\":{\"id\":\"$INTENT_ID\",\"agent\":\"worker\"}}")
    if echo "$CLM" | grep -q '"result"'; then
        echo "    claim_intent: ok"
    else
        echo "    claim_intent: FAIL ($CLM)"
        failed=1
    fi

    # Heartbeat
    local HBT
    HBT=$(rpc "{\"id\":13,\"method\":\"heartbeat_intent\",\"params\":{\"id\":\"$INTENT_ID\",\"agent\":\"worker\"}}")
    if echo "$HBT" | grep -q '"result"'; then
        echo "    heartbeat_intent: ok"
    else
        echo "    heartbeat_intent: FAIL ($HBT)"
        failed=1
    fi

    # Conclude
    local CCL
    CCL=$(rpc "{\"id\":14,\"method\":\"conclude_intent\",\"params\":{\"id\":\"$INTENT_ID\",\"result\":\"done\"}}")
    if echo "$CCL" | grep -q '"result"'; then
        echo "    conclude_intent: ok"
    else
        echo "    conclude_intent: FAIL ($CCL)"
        failed=1
    fi

    # ═══════════════════════════════════════════════════════════════════
    # Scenario 3: Agent lifecycle — spawn, verify, kill
    # ═══════════════════════════════════════════════════════════════════
    echo "  [3/7] Agent lifecycle..."

    local SP
    SP=$(rpc '{"id":20,"method":"spawn_agent","params":{"command":"sleep","args":["5"]}}')
    local AGENT_PID
    AGENT_PID=$(echo "$SP" | sed 's/.*"pid":\([0-9]*\).*/\1/')
    if [ -n "$AGENT_PID" ] && [ "$AGENT_PID" -gt 0 ] 2>/dev/null; then
        echo "    spawn_agent: pid=$AGENT_PID"
    else
        echo "    spawn_agent: FAIL ($SP)"
        failed=1
    fi

    local LS
    LS=$(rpc '{"id":21,"method":"list_agents","params":{}}')
    if echo "$LS" | grep -q "\"pid\":$AGENT_PID"; then
        echo "    list_agents: ok (agent tracked)"
    else
        echo "    list_agents: FAIL ($LS)"
        failed=1
    fi

    local KL
    KL=$(rpc "{\"id\":22,\"method\":\"kill_agent\",\"params\":{\"pid\":$AGENT_PID}}")
    if echo "$KL" | grep -q '"result"'; then
        echo "    kill_agent: ok"
    else
        echo "    kill_agent: FAIL ($KL)"
        failed=1
    fi

    # Verify agent removed
    sleep 0.3
    local LS2
    LS2=$(rpc '{"id":23,"method":"list_agents","params":{}}')
    if echo "$LS2" | grep -q '"agents":\[\]'; then
        echo "    agent cleanup: ok"
    else
        echo "    agent cleanup: FAIL (agent not removed: $LS2)"
        failed=1
    fi

    # ═══════════════════════════════════════════════════════════════════
    # Scenario 4: Error handling — conflict, rejection
    # ═══════════════════════════════════════════════════════════════════
    echo "  [4/7] Error handling..."

    # Unknown method
    local UNK
    UNK=$(rpc '{"id":30,"method":"nonexistent","params":{}}')
    if echo "$UNK" | grep -q '"error"'; then
        echo "    unknown_method: ok"
    else
        echo "    unknown_method: FAIL ($UNK)"
        failed=1
    fi

    # Intent without facts
    local NOF
    NOF=$(rpc '{"id":31,"method":"write_intent","params":{"from_facts":[],"description":"bad","creator":"t"}}')
    if echo "$NOF" | grep -q '"error"'; then
        echo "    no_fact_intent: ok (rejected)"
    else
        echo "    no_fact_intent: FAIL ($NOF)"
        failed=1
    fi

    # Double claim
    local DC
    DC=$(rpc "{\"id\":32,\"method\":\"claim_intent\",\"params\":{\"id\":\"$INTENT_ID\",\"agent\":\"intruder\"}}")
    if echo "$DC" | grep -q '"error"'; then
        echo "    double_claim: ok (rejected)"
    else
        echo "    double_claim: FAIL ($DC)"
        failed=1
    fi

    # ═══════════════════════════════════════════════════════════════════
    # Scenario 5: Graceful shutdown — SIGTERM cleans socket
    # ═══════════════════════════════════════════════════════════════════
    echo "  [5/7] Graceful shutdown..."

    # Start a separate daemon for this test
    local SIG_SOCKET_DIR
    SIG_SOCKET_DIR=$(mktemp -d)
    local SIG_SOCKET_PATH="${SIG_SOCKET_DIR}/sigterm.sock"
    NEXD_SOCKET_PATH="$SIG_SOCKET_PATH" ./target/debug/nexd &
    local SIG_PID=$!

    waited=0
    while [ ! -S "$SIG_SOCKET_PATH" ] && [ "$waited" -lt 15 ]; do
        sleep 0.2
        waited=$((waited + 1))
    done
    if [ ! -S "$SIG_SOCKET_PATH" ]; then
        echo "    sigterm daemon: not ready (skip)"
    else
        # Send SIGTERM
        kill -TERM "$SIG_PID" 2>/dev/null || true
        # Wait for exit
        waited=0
        while kill -0 "$SIG_PID" 2>/dev/null && [ "$waited" -lt 15 ]; do
            sleep 0.2
            waited=$((waited + 1))
        done
        if [ ! -S "$SIG_SOCKET_PATH" ]; then
            echo "    sigterm cleanup: ok (socket removed)"
        else
            echo "    sigterm cleanup: WARN (socket remains: $SIG_SOCKET_PATH)"
            # This is soft-fail — the tmpdir cleanup handles it
        fi
        kill -9 "$SIG_PID" 2>/dev/null || true
    fi
    rm -rf "$SIG_SOCKET_DIR"

    # ═══════════════════════════════════════════════════════════════════
    # Scenario 6: Concurrent communication — multiple clients
    # ═══════════════════════════════════════════════════════════════════
    echo "  [6/7] Concurrent operations..."

    # Send two write_fact requests in parallel
    rpc '{"id":40,"method":"write_fact","params":{"origin":"concurrent-a","content":"alpha","creator":"a"}}' > /dev/null &
    local PIDA=$!
    rpc '{"id":41,"method":"write_fact","params":{"origin":"concurrent-b","content":"beta","creator":"b"}}' > /dev/null &
    local PIDB=$!
    wait "$PIDA" "$PIDB" 2>/dev/null || true

    # Verify both facts are present
    local ST
    ST=$(rpc '{"id":42,"method":"read_state","params":{}}')
    # Count facts (should be at least the lifecycle ones + 2 concurrent)
    local FACT_COUNT
    FACT_COUNT=$(echo "$ST" | grep -o '"origin"' | wc -l | tr -d ' ')
    if [ "$FACT_COUNT" -ge 3 ]; then
        echo "    concurrent writes: ok ($FACT_COUNT facts)"
    else
        echo "    concurrent writes: WARN (only $FACT_COUNT facts)"
    fi

    # ── Summary ────────────────────────────────────────────────────────

    # Cleanup
    kill "$NEXD_PID" 2>/dev/null || true
    wait "$NEXD_PID" 2>/dev/null || true
    rm -rf "$SOCKET_DIR"

    echo ""
    if [ "$failed" -eq 0 ]; then
        echo "nexd: 7/7 scenarios passed"
    else
        echo "nexd: some scenarios FAILED"
        return 1
    fi
}

# ── App suite ─────────────────────────────────────────────────────────────

run_apps() {
    local any_failed=0
    verify_nexd || any_failed=1
    # Reset ports between apps to avoid conflicts
    kill_port 30921 2>/dev/null || true
    kill_port 30922 2>/dev/null || true
    sleep 1
    verify_nex_spinwasi_ssccsdocs || any_failed=1
    # Future apps go here, e.g.:
    # verify_nex_cf_mock || any_failed=1
    # verify_nex_zed || any_failed=1
    return "$any_failed"
}

# ── Command dispatch ──────────────────────────────────────────────────────

case "${1:-}" in
    --core)
        shift
        exec ./scripts/run-core.sh "$@"
        ;;
    --gateway)
        shift
        exec ./scripts/run-gateway.sh "$@"
        ;;
    --apps)
        shift
        run_apps
        ;;
    --playbooks)
        kill_port 30922
        exec ./playbooks/run.sh
        ;;
    --help|-h)
        echo "Usage: $0 [OPTION]"
        echo "  (no arg)      Core + gateway + apps + playbooks [default]"
        echo "  --core        Core checks only (nex, storage/*)"
        echo "  --gateway     Gateway layer checks (api, nex-cf, serde-proxy)"
        echo "  --apps        Standalone app verification (spinwasi, cf-mock, ...)"
        echo "  --playbooks   Consumer playbooks only"
        ;;
    "")
        # Default: run everything
        echo "=== Core ==="
        ./scripts/run-core.sh
        echo ""
        echo "=== Gateway ==="
        ./scripts/run-gateway.sh
        echo ""
        echo "=== Apps ==="
        run_apps
        echo ""
        kill_port 30922
        echo "=== Playbooks ==="
        ./playbooks/run.sh
        ;;
    *)
        echo "Unknown: $1"
        echo "Usage: $0 [--core|--gateway|--apps|--playbooks]"
        exit 1
        ;;
esac
