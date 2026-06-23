#!/usr/bin/env bash
set -euo pipefail
#
# run-nex-cf.sh — Check the nexus-gateway-nex-cf CF Worker build.
#
# Usage:
#   scripts/run-nex-cf.sh               # Check + build
#   scripts/run-nex-cf.sh --check        # cargo check only (fast)
#   scripts/run-nex-cf.sh --build        # worker-build (WASM)

cd "$(dirname "$0")/../apps/nex-cf"

MODE="${1:-check}"

case "$MODE" in
    --check|check)
        echo "cargo check (WASM target)..."
        cargo check --target wasm32-unknown-unknown
        ;;
    --test|test)
        echo "cargo test (native)..."
        cargo test --target wasm32-unknown-unknown || true
        ;;
    --build|build)
        echo "worker-build..."
        worker-build --release
        ;;
    *)
        echo "cargo check (WASM target)..."
        cargo check --target wasm32-unknown-unknown
        echo ""
        echo "cargo test (native)..."
        cargo test
        echo ""
        echo "worker-build..."
        worker-build --release
        ;;
esac

echo "nex-cf check complete."
