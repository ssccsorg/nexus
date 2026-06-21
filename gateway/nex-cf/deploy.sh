#!/usr/bin/env bash
set -euo pipefail
#
# deploy.sh — Deploy nexus-gateway-nex-cf to Cloudflare Workers
#
# Usage:
#   ./deploy.sh             # Production deploy
#   ./deploy.sh --preview   # Preview deploy (test R2 buckets)
#   ./deploy.sh --check     # WASM check only, no deploy
#
# Automatically runs worker-build before wrangler deploy.

cd "$(dirname "$0")"

MODE="${1:-production}"

case "$MODE" in
    --check|check)
        echo "=== WASM check ==="
        cargo check --target wasm32-unknown-unknown
        echo "OK"
        exit 0
        ;;
    --preview|preview)
        echo "=== worker-build (release) ==="
        cargo install -q worker-build --version 0.8.4
        worker-build --release
        echo ""
        echo "=== wrangler deploy (preview) ==="
        wrangler deploy --env preview
        echo ""
        echo "Deployed to preview.cf.nexgate.ssccs.org"
        ;;
    *)
        echo "=== worker-build (release) ==="
        cargo install -q worker-build --version 0.8.4
        worker-build --release
        echo ""
        echo "=== wrangler deploy (production) ==="
        wrangler deploy
        echo ""
        echo "Deployed to cf.nexgate.ssccs.org"
        ;;
esac
