#!/usr/bin/env bash
set -euo pipefail
#
# nexus — Unified CI runner
#
# By default runs everything (core checks + gateway + playbooks).
# Sub-commands for focused tasks.
#
# Usage:
#   ./run.sh               # Everything (default)
#   ./run.sh --core        # Core checks only (nex, storage/*)
#   ./run.sh --gateway     # Gateway layer checks (api, nex-cf, serde-proxy)
#   ./run.sh --playbooks   # Consumer playbooks only
#

cd "$(dirname "$0")"

# ── Port cleanup ──────────────────────────────────────────────────────────
#
# Kill any process holding a given port. Used before playbooks (which start
# gateway-api) to avoid AddrInUse from a stale process.

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
    --spinwasi)
        shift
        echo "=== nex-spinwasi-ssccsdocs ==="
        echo "Building..."
        (cd apps/nex-spinwasi-ssccsdocs && spin build 2>&1)
        echo ""
        echo "Starting server..."
        (cd apps/nex-spinwasi-ssccsdocs && spin up --build 2>&1) &
        SPIN_PID=$!
        sleep 4
        echo ""
        echo "Testing endpoints..."
        echo -n "GET /        : "
        curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/ 2>&1
        echo ""
        echo -n "GET /version : "
        curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/version 2>&1
        echo ""
        echo -n "POST /ingest: "
        curl -s -o /dev/null -w "%{http_code}" -X POST http://127.0.0.1:3000/ingest -H "content-type: application/json" -d '{"text":"hello test","origin":"test"}' 2>&1
        echo ""
        echo -n "GET /search : "
        curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:3000/search?q=hello" 2>&1
        echo ""
        echo -n "GET /state  : "
        curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3000/state 2>&1
        echo ""
        echo ""
        echo "Stopping server..."
        kill "$SPIN_PID" 2>/dev/null || true
        echo "Done."
        ;;
    --playbooks)
        kill_port 3000
        exec ./playbooks/run.sh
        ;;
    --help|-h)
        echo "Usage: $0 [OPTION]"
        echo "  (no arg)      Core + gateway + playbooks [default]"
        echo "  --core        Core checks only (nex, storage/*)"
        echo "  --gateway     Gateway layer checks (api, nex-cf, serde-proxy)"
        echo "  --spinwasi    nex-spinwasi-ssccsdocs Spin build + test"
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
        echo "=== Spin WASI ==="
        $0 --spinwasi
        echo ""
        kill_port 3000
        echo "=== Playbooks ==="
        ./playbooks/run.sh
        ;;
    *)
        echo "Unknown: $1"
        echo "Usage: $0 [--core|--gateway|--spinwasi|--playbooks]"
        exit 1
        ;;
esac
