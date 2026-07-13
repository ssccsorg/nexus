#!/usr/bin/env bash
set -euo pipefail
#
# nex-tagma — Standard reference implementation runner
#
# Manages the tagma-core subtree dependency and runs build/test/bench.
#
# Usage:
#   ./run.sh                    # Build + test + bench (default)
#   ./run.sh --test             # Run tests only
#   ./run.sh --bench            # Run benchmark only
#   ./run.sh --refresh-tagma    # Pull latest tagma-core subtree from GitHub
#   ./run.sh --help
#

cd "$(dirname "$0")"

APP_DIR="$(pwd)"
GIT_ROOT="$(cd "$APP_DIR" && git rev-parse --show-toplevel)"

TAGMA_REPO_HTTPS="https://github.com/ssccsorg/tagma.git"
TAGMA_PREFIX="libs/tagma"
TAGMA_BRANCH="main"

# Resolve repo URL: GITHUB_TOKEN for CI (private), HTTPS default (public repo)
# Allows override via TAGMA_REPO env var
if [ -z "${TAGMA_REPO:-}" ]; then
    if [ -n "${GITHUB_TOKEN:-}" ] || [ -n "${GH_TOKEN:-}" ]; then
        token="${GITHUB_TOKEN:-${GH_TOKEN}}"
        TAGMA_REPO="https://x-access-token:${token}@github.com/ssccsorg/tagma.git"
    else
        TAGMA_REPO="$TAGMA_REPO_HTTPS"
    fi
fi

ensure_subtree() {
    if [ ! -d "$GIT_ROOT/$TAGMA_PREFIX/sw/rust/core/src" ]; then
        echo "tagma: adding subtree from $TAGMA_REPO ($TAGMA_BRANCH)..."
        cd "$GIT_ROOT"
        git config user.name "nex-tagma CI" 2>/dev/null || true
        git config user.email "ci@ssccs.org" 2>/dev/null || true
        git subtree add --prefix "$TAGMA_PREFIX" --squash "$TAGMA_REPO" "$TAGMA_BRANCH"
        cd "$APP_DIR"
    fi
}

case "${1:-}" in
    --refresh-tagma)
        echo "tagma: pulling latest subtree from $TAGMA_REPO ($TAGMA_BRANCH)..."
        cd "$GIT_ROOT"
        git config user.name "nex-tagma CI" 2>/dev/null || true
        git config user.email "ci@ssccs.org" 2>/dev/null || true
        git subtree pull --prefix "$TAGMA_PREFIX" --squash "$TAGMA_REPO" "$TAGMA_BRANCH"
        ;;
    --test)
        shift
        exec cargo test "$@"
        ;;
    --bench)
        exec cargo run -- bench
        ;;
    --help|-h)
        sed -n '3,14p' "$0"
        exit 0
        ;;
    *)
        ensure_subtree
        echo "--- build ---"
        cargo build 2>&1
        echo "--- test ---"
        cargo test 2>&1
        echo "--- bench ---"
        cargo run -- bench 2>&1
        ;;
esac
