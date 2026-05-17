#!/usr/bin/env bash
set -euo pipefail
#
# nexus — Unified CI runner
#
# Single entry point for ALL CI workflows. Run locally to reproduce CI checks
# exactly. Each step mirrors its corresponding .github/workflows/*.yml.
#
# Usage:
#   ./run.sh                # Full: core + docker + gateway (no deploy, no sync)
#   ./run.sh --core         # Rust: fmt → check → clippy → test
#   ./run.sh --docker       # Docker: build LightRAG, Graphiti, Memgraph images
#   ./run.sh --gateway      # npm ci for module-hub and af-sync
#   ./run.sh --sync         # Docs sync from R2 (needs AWS credentials)
#   ./run.sh --all          # Everything including sync
#

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export SCRIPT_DIR
PASSED_ARGS=()

while [[ "$#" -gt 0 ]]; do
    case $1 in
        --core)    MODE_CORE=true ;;
        --docker)  MODE_DOCKER=true ;;
        --gateway) MODE_GATEWAY=true ;;
        --sync)    MODE_SYNC=true ;;
        --all)     MODE_CORE=true; MODE_DOCKER=true; MODE_GATEWAY=true; MODE_SYNC=true ;;
        *)         PASSED_ARGS+=("$1") ;;
    esac
    shift
done

# Default: core + docker + gateway (no sync, needs creds)
if [[ -z "${MODE_CORE:-}" && -z "${MODE_DOCKER:-}" && -z "${MODE_GATEWAY:-}" && -z "${MODE_SYNC:-}" ]]; then
    MODE_CORE=true
    MODE_DOCKER=true
    MODE_GATEWAY=true
fi

# ── Core Rust checks (mirrors .github/workflows/core.yml) ─────────────────

run_core() {
    echo ""
    echo "═══ core: Rust workspace ═══"
    cd "$SCRIPT_DIR/core"

    echo "--- fmt ---"
    cargo fmt

    echo "--- check ---"
    cargo check -p nexus-graph
    cargo check

    echo "--- clippy ---"
    cargo clippy -- -D warnings

    echo "--- test ---"
    cargo test -p nexus-graph -- --nocapture 2>&1
    echo "core: all checks passed"
}

# ── Docker builds (mirrors .github/workflows/build.yml) ──────────────────

run_docker() {
    echo ""
    echo "═══ docker: container images ═══"

    echo "--- lightrag ---"
    docker build -t lightrag-nexus -f "$SCRIPT_DIR/ext/lightrag/Dockerfile" "$SCRIPT_DIR/ext/lightrag"

    echo "--- graphiti ---"
    docker build -t graphiti-nexus -f "$SCRIPT_DIR/ext/graphiti/Dockerfile" "$SCRIPT_DIR/ext/graphiti"

    echo "--- memgraph-proxy ---"
    docker build -t memgraph-proxy -f "$SCRIPT_DIR/ext/memgraph/Dockerfile" "$SCRIPT_DIR/ext/memgraph"

    echo "docker: all images built"
}

# ── Gateway npm ci (mirrors .github/workflows/deploy.yml) ────────────────

run_gateway() {
    echo ""
    echo "═══ gateway: npm dependencies ═══"

    if command -v node &>/dev/null; then
        echo "--- module-hub ---"
        cd "$SCRIPT_DIR/gateway/module-hub"
        npm ci

        echo "--- af-sync ---"
        cd "$SCRIPT_DIR/gateway/af-sync"
        npm ci

        echo "gateway: npm dependencies installed"
    else
        echo "node not found — skipping gateway checks"
    fi
}

# ── Docs sync (mirrors .github/workflows/sync-docs.yml) ─────────────────

run_sync() {
    echo ""
    echo "═══ sync: R2 to local ═══"

    if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
        echo "AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY required for sync"
        echo "skipping sync"
        return
    fi

    local sync_root="${SYNC_ROOT:-/tmp/ssccs}"
    echo "--- aws s3 sync ---"
    aws s3 sync "s3://ssccs-nexus-af/ssccs" "$sync_root" \
        --endpoint-url "${CFR2_USR_API_HOST:-https://fda2afbec0a19b01c76d98e81f45be41.r2.cloudflarestorage.com}" \
        --delete

    echo "--- fetch_ssccs.py ---"
    python3 "$SCRIPT_DIR/scripts/fetch_ssccs.py" --sync-root "$sync_root" --all

    echo "sync: complete"
}

# ── Dispatch ─────────────────────────────────────────────────────────────

if [[ "${MODE_CORE:-}" == "true" ]]; then run_core; fi
if [[ "${MODE_DOCKER:-}" == "true" ]]; then run_docker; fi
if [[ "${MODE_GATEWAY:-}" == "true" ]]; then run_gateway; fi
if [[ "${MODE_SYNC:-}" == "true" ]]; then run_sync; fi

echo ""
echo "All checks passed."
