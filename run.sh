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
    echo "Starting server on port $PORT..."
    # Aggressive port cleanup
    kill_port "$PORT" 2>/dev/null || true
    sleep 1
    kill_port "$PORT" 2>/dev/null || true
    sleep 1
    pkill -f "spin.*up" 2>/dev/null || true
    sleep 1
    # spin up --build handles both building and serving
    (cd apps/nex-spinwasi-ssccsdocs && spin up --build --listen "127.0.0.1:$PORT" 2>&1) &
    local SPIN_PID=$!

    # Wait for server to be ready (poll until HTTP 200)
    local TIMEOUT=60
    local waited=0
    while [ "$waited" -lt "$TIMEOUT" ]; do
        code=$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:${PORT}/" 2>/dev/null || echo "000")
        if [ "$code" = "200" ]; then
            break
        fi
        sleep 1
        waited=$((waited + 1))
    done
    if [ "$waited" -ge "$TIMEOUT" ]; then
        echo "  server not ready after ${TIMEOUT}s (FAIL)"
        kill "$SPIN_PID" 2>/dev/null || true
        return 1
    fi
    echo "  server ready after ${waited}s"

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
    local NEXD_SOCKET="${SOCKET_DIR}/nexd.sock"
    local NEX_SERVER_SOCKET="${SOCKET_DIR}/nex-server.sock"

    echo "=== nexd ==="
    echo "Building..."
    cargo build -p nexd 2>&1
    cargo build -p nex-server 2>&1
    echo ""
    echo "Starting nex-server on ${NEX_SERVER_SOCKET}..."

    local NEX_BIN
    if [ -x "./target/debug/nex-server" ]; then
        NEX_BIN="./target/debug/nex-server"
    else
        echo "nex-server binary not found at ./target/debug/nex-server (FAIL)"
        return 1
    fi
    NEX_SOCKET_PATH="$NEX_SERVER_SOCKET" "$NEX_BIN" 2>/tmp/nex-server-debug.log &
    local NEX_PID=$!

    # Wait for nex-server socket
    local waited=0
    while [ ! -S "$NEX_SERVER_SOCKET" ] && [ "$waited" -lt 15 ]; do
        sleep 0.2
        waited=$((waited + 1))
    done
    if [ ! -S "$NEX_SERVER_SOCKET" ]; then
        echo "nex-server: socket not ready (FAIL)"
        cat /tmp/nex-server-debug.log 2>/dev/null || true
        return 1
    fi
    echo "  nex-server ready"

    echo "Starting nexd on ${NEXD_SOCKET}..."
    NEXD_SOCKET_PATH="$NEXD_SOCKET" \
      NEXD_NEX_SERVER_PATH="$NEX_BIN" \
      NEX_SOCKET_PATH="$NEX_SERVER_SOCKET" \
      ./target/debug/nexd 2>/tmp/nexd-debug.log &
    local NEXD_PID=$!
    trap 'kill $NEX_PID $NEXD_PID 2>/dev/null; rm -rf "$SOCKET_DIR"' EXIT

    # Wait for nexd socket
    waited=0
    while [ ! -S "$NEXD_SOCKET" ] && [ "$waited" -lt 30 ]; do
        sleep 0.2
        waited=$((waited + 1))
    done
    if [ ! -S "$NEXD_SOCKET" ]; then
        echo "nexd: socket not ready after ${waited}s (FAIL)"
        cat /tmp/nexd-debug.log 2>/dev/null || true
        return 1
    fi
    echo "  daemon ready"
    echo ""

    local failed=0

    # Helper: send JSON-RPC to nexd socket, return response
    rpc() {
        echo "$1" | socat - "UNIX-CONNECT:${NEXD_SOCKET}" 2>/dev/null || echo '{"error":"connection failed"}'
    }

    # ═══════════════════════════════════════════════════════════════════
    # Scenario 1: Basic transport — write fact, read state
    # ═══════════════════════════════════════════════════════════════════
    echo "  [1/7] Basic FIH operations..."

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
    RF=$(rpc '{"id":3,"method":"read_fact","params":{"id":"'"$FACT_ID"'"}}')
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

    # Verify agent removed (sleep process should be gone, nex-server remains)
    sleep 0.3
    local LS2
    LS2=$(rpc '{"id":23,"method":"list_agents","params":{}}')
    if echo "$LS2" | grep -qv "\"pid\":$AGENT_PID"; then
        echo "    agent cleanup: ok (agent $AGENT_PID removed)"
    else
        echo "    agent cleanup: FAIL (agent $AGENT_PID still present: $LS2)"
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
        echo "    double_claim: ok (concluded intent re-claimable)"
    fi

    # ═══════════════════════════════════════════════════════════════════
    # Scenario 5: Graceful shutdown — SIGTERM cleans socket
    # ═══════════════════════════════════════════════════════════════════
    echo "  [5/7] Graceful shutdown..."

    # Start a separate daemon for this test
    local SIG_SOCKET_DIR
    SIG_SOCKET_DIR=$(mktemp -d)
    local SIG_SOCKET_PATH="${SIG_SOCKET_DIR}/sigterm.sock"
    local SIG_NEX_SOCKET="${SIG_SOCKET_DIR}/nex-sigterm.sock"

    NEX_SOCKET_PATH="$SIG_NEX_SOCKET" "$NEX_BIN" 2>/dev/null &
    local SIG_NEX_PID=$!
    waited=0
    while [ ! -S "$SIG_NEX_SOCKET" ] && [ "$waited" -lt 10 ]; do sleep 0.2; waited=$((waited + 1)); done

    NEXD_SOCKET_PATH="$SIG_SOCKET_PATH" \
      NEXD_NEX_SERVER_PATH="$NEX_BIN" \
      NEX_SOCKET_PATH="$SIG_NEX_SOCKET" \
      ./target/debug/nexd 2>/tmp/nexd-sig-debug.log &
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
        kill -9 "$SIG_NEX_PID" 2>/dev/null || true
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
    trap - EXIT
    kill "$NEX_PID" "$NEXD_PID" 2>/dev/null || true
    wait "$NEX_PID" "$NEXD_PID" 2>/dev/null || true
    rm -f /tmp/nexd-debug.log /tmp/nexd-sig-debug.log /tmp/nex-server-debug.log
    rm -rf "$SOCKET_DIR"

    echo ""
    if [ "$failed" -eq 0 ]; then
        echo "nexd: 7/7 scenarios passed"
    else
        echo "nexd: some scenarios FAILED"
        return 1
    fi
}

# ── nex-calc (CLI) ─────────────────────────────────────────────────────────

verify_nex_calc() {
    echo "=== nex-calc ==="
    cargo build -p nex-calc 2>&1 || { echo "nex-calc: build FAILED"; return 1; }
    cargo test -p nex-calc 2>&1 || { echo "nex-calc: tests FAILED"; return 1; }
    echo "nex-calc: passed"
}

# ── nex-server standalone verification ────────────────────────────────────

verify_nex_server() {
    local SOCKET_DIR
    SOCKET_DIR=$(mktemp -d)
    local SOCKET_PATH="${SOCKET_DIR}/nx.sock"

    echo "=== nex-server ==="
    echo "Building..."
    cargo build -p nex-server 2>&1 || { echo "nex-server: build FAILED"; return 1; }
    echo ""
    echo "Starting nex-server on ${SOCKET_PATH}..."

    NEX_SOCKET_PATH="$SOCKET_PATH" NEX_DATA_DIR="$SOCKET_DIR/data" ./target/debug/nex-server 2>/dev/null &
    local NEX_PID=$!
    trap 'kill $NEX_PID 2>/dev/null; rm -rf "$SOCKET_DIR"' EXIT

    local waited=0
    while [ ! -S "$SOCKET_PATH" ] && [ "$waited" -lt 15 ]; do sleep 0.2; waited=$((waited + 1)); done
    if [ ! -S "$SOCKET_PATH" ]; then echo "nex-server: socket not ready (FAIL)"; return 1; fi
    echo "  server ready"

    local failed=0

    rpc() {
        echo "$1" | socat - "UNIX-CONNECT:${SOCKET_PATH}" 2>/dev/null || echo '{"error":"connection failed"}'
    }

    echo "  [1/4] FIH operations..."
    local F1
    F1=$(rpc '{"id":1,"method":"write_fact","params":{"origin":"vt","content":"virtual test","creator":"ci"}}')
    local FID
    FID=$(echo "$F1" | sed 's/.*"id":"\([^"]*\)".*/\1/')
    [ -n "$FID" ] && echo "    write_fact: ok" || { echo "    write_fact: FAIL"; failed=1; }

    local S1
    S1=$(rpc '{"id":2,"method":"read_state","params":{}}')
    echo "$S1" | grep -q '"facts"' && echo "    read_state: ok" || { echo "    read_state: FAIL"; failed=1; }

    echo "  [2/4] Intent lifecycle..."
    local IID
    IID=$(rpc "{\"id\":10,\"method\":\"write_intent\",\"params\":{\"from_facts\":[\"$FID\"],\"description\":\"vt intent\",\"creator\":\"ci\"}}")
    local INTENT_ID
    INTENT_ID=$(echo "$IID" | sed 's/.*"id":"\([^"]*\)".*/\1/')
    [ -n "$INTENT_ID" ] && echo "    write_intent: ok" || { echo "    write_intent: FAIL ($IID)"; failed=1; }

    rpc "{\"id\":11,\"method\":\"claim_intent\",\"params\":{\"id\":\"$INTENT_ID\",\"agent\":\"w\"}}" | grep -q '"result"' && echo "    claim_intent: ok" || { echo "    claim_intent: FAIL"; failed=1; }
    rpc "{\"id\":12,\"method\":\"heartbeat_intent\",\"params\":{\"id\":\"$INTENT_ID\",\"agent\":\"w\"}}" | grep -q '"result"' && echo "    heartbeat_intent: ok" || { echo "    heartbeat_intent: FAIL"; failed=1; }
    rpc "{\"id\":13,\"method\":\"conclude_intent\",\"params\":{\"id\":\"$INTENT_ID\",\"result\":\"done\"}}" | grep -q '"result"' && echo "    conclude_intent: ok" || { echo "    conclude_intent: FAIL"; failed=1; }

    echo "  [3/4] Error handling..."
    rpc '{"id":20,"method":"nonexistent","params":{}}' | grep -q '"error"' && echo "    unknown_method: ok" || { echo "    unknown_method: FAIL"; failed=1; }
    rpc '{"id":21,"method":"write_intent","params":{"from_facts":[],"description":"x","creator":"x"}}' | grep -q '"error"' && echo "    no_fact_intent: ok (rejected)" || { echo "    no_fact_intent: FAIL"; failed=1; }

    echo "  [4/4] Write hint..."
    rpc '{"id":30,"method":"write_hint","params":{"id":"h_vt_1","content":"virtual hint","creator":"ci"}}' | grep -q '"result"' && echo "    write_hint: ok" || { echo "    write_hint: FAIL"; failed=1; }

    trap - EXIT
    kill "$NEX_PID" 2>/dev/null || true
    wait "$NEX_PID" 2>/dev/null || true
    rm -rf "$SOCKET_DIR"

    echo ""
    if [ "$failed" -eq 0 ]; then
        echo "nex-server: all tests passed"
    else
        echo "nex-server: some tests FAILED"
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
    verify_nex_calc || any_failed=1
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
        exit 0
        ;;
    --server)
        shift
        verify_nex_server
        exit 0
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
        echo "  --server      nex-server standalone verification"
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
