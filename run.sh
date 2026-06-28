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
    echo "=== nex-spinwasi-ssccsdocs ==="
    echo "Building..."
    (cd apps/nex-spinwasi-ssccsdocs && spin build 2>&1)
    echo ""
    echo "Starting server..."
    (cd apps/nex-spinwasi-ssccsdocs && spin up --build 2>&1) &
    local SPIN_PID=$!
    sleep 4

    local failed=0
    echo "Testing endpoints..."
    for test in \
        "GET /        : curl -s -o /dev/null -w %{http_code} http://127.0.0.1:3000/" \
        "GET /version : curl -s -o /dev/null -w %{http_code} http://127.0.0.1:3000/version" \
        "POST /ingest: curl -s -o /dev/null -w %{http_code} -X POST http://127.0.0.1:3000/ingest -H content-type:application/json -d '{\"text\":\"hello test\",\"origin\":\"test\"}'" \
        "GET /search : curl -s -o /dev/null -w %{http_code} 'http://127.0.0.1:3000/search?q=hello'" \
        "GET /state  : curl -s -o /dev/null -w %{http_code} http://127.0.0.1:3000/state"
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
    if [ "$failed" -eq 0 ]; then
        echo "nex-spinwasi-ssccsdocs: all 5/5 passed"
    else
        echo "nex-spinwasi-ssccsdocs: some tests FAILED"
        return 1
    fi
}

# ── App suite ─────────────────────────────────────────────────────────────

run_apps() {
    local any_failed=0
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
        kill_port 3000
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
        kill_port 3000
        echo "=== Playbooks ==="
        ./playbooks/run.sh
        ;;
    *)
        echo "Unknown: $1"
        echo "Usage: $0 [--core|--gateway|--apps|--playbooks]"
        exit 1
        ;;
esac
