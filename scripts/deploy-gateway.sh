#!/usr/bin/env bash
set -euo pipefail
#
# Run by .github/workflows/deploy.yml.
# Installs npm deps and optionally deploys gateway workers.
#
# The Cloudflare API token is passed via --token (or read from
# CLOUDFLARE_API_TOKEN env var as fallback).  The script exports
# CLOUDFLARE_API_TOKEN before invoking wrangler.
#
# Usage:
#   scripts/deploy-gateway.sh                           # npm ci + wrangler deploy
#   scripts/deploy-gateway.sh --token <token>            # npm ci + wrangler deploy
#   scripts/deploy-gateway.sh --check                    # npm ci only (PR check)

cd "$(dirname "$0")/.."

MODE="deploy"
TOKEN=""

while [ $# -gt 0 ]; do
    case "$1" in
        --check)
            MODE="check"
            shift
            ;;
        --token)
            if [ $# -ge 2 ] && ! [[ "$2" =~ ^-- ]]; then
                TOKEN="$2"
                shift 2
            else
                TOKEN=""
                shift
            fi
            ;;
        *)
            echo "ERROR: Unknown option: $1"
            exit 1
            ;;
    esac
done

deploy_worker() {
    local name="$1"
    local dir="$2"
    echo "=== $name ==="
    (cd "$dir" && npm ci)
    if [ "$MODE" = "deploy" ]; then
        local token="${TOKEN:-${CLOUDFLARE_API_TOKEN:-}}"
        if [ -z "$token" ]; then
            echo "ERROR: Cloudflare API token not provided. Set CF_API_TOKEN secret or pass --token."
            exit 1
        fi
        export CLOUDFLARE_API_TOKEN="$token"
        (cd "$dir" && npx wrangler deploy)
    fi
}

deploy_worker "module-hub" "gateway/module-hub"
deploy_worker "af-sync" "gateway/af-sync"

echo "Gateway deployment complete."
