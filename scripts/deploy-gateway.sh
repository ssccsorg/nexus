#!/usr/bin/env bash
set -euo pipefail
#
# Run by .github/workflows/deploy.yml.
# Installs npm deps and optionally deploys gateway workers.
#
# Usage:
#   scripts/deploy-gateway.sh          # npm ci + wrangler deploy
#   scripts/deploy-gateway.sh --check  # npm ci only (PR check)

cd "$(dirname "$0")/.."
MODE="${1:-deploy}"

deploy_worker() {
    local name="$1"
    local dir="$2"
    echo "=== $name ==="
    (cd "$dir" && npm ci)
    if [ "$MODE" = "deploy" ]; then
        (cd "$dir" && npx wrangler deploy)
    fi
}

deploy_worker "module-hub" "gateway/module-hub"
deploy_worker "af-sync" "gateway/af-sync"

echo "Gateway deployment complete."
