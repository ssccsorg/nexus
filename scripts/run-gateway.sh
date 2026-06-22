#!/usr/bin/env bash
set -euo pipefail
#
# run-gateway.sh — Gateway layer checks
#
# Verifies all gateway components:
#   apps/nex-api         — FIH Blackboard HTTP API (Rust)
#   apps/nex-cf          — Cloudflare Worker (Rust/WASM)
#   libs/serde-proxy     — Serialization proxy (Rust)
#
# Usage:
#   scripts/run-gateway.sh                # All gateway checks
#   scripts/run-gateway.sh --api          # Only apps/nex-api
#   scripts/run-gateway.sh --nex-cf       # Only apps/nex-cf
#   scripts/run-gateway.sh --serde        # Only libs/serde-proxy

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

MODE="${1:-all}"

run_api() {
    echo "=== apps/nex-api (cargo test) ==="
    (cd apps/nex-api && cargo test)
}

run_nex_cf() {
    echo "=== apps/nex-cf (cargo check + test) ==="
    ./scripts/run-nex-cf.sh --check-and-test
}

run_serde() {
    echo "=== libs/serde-proxy (cargo test) ==="
    (cd libs/serde-proxy && cargo test)
}

case "$MODE" in
    --api|api)
        run_api
        ;;
    --nex-cf|nex-cf)
        run_nex_cf
        ;;
    --serde|serde)
        run_serde
        ;;
    *)
        echo "Gateway layer checks"
        echo ""
        run_api
        echo ""
        run_nex_cf
        echo ""
        run_serde
        echo ""
        echo "All gateway checks passed."
        ;;
esac
