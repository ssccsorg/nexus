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

cd "$(dirname "$0")/../.."

TAGMA_REPO_SSH="git@github.com:ssccsorg/tagma.git"
TAGMA_PREFIX="libs/tagma"
TAGMA_BRANCH="1-tagma-core-rust"

# Resolve repo URL: GITHUB_TOKEN for CI, SSH for local
# Allows override via TAGMA_REPO env var
if [ -z "${TAGMA_REPO:-}" ]; then
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        TAGMA_REPO="https://x-access-token:${GITHUB_TOKEN}@github.com/ssccsorg/tagma.git"
    elif [ -n "${GH_TOKEN:-}" ]; then
        TAGMA_REPO="https://x-access-token:${GH_TOKEN}@github.com/ssccsorg/tagma.git"
    else
        TAGMA_REPO="$TAGMA_REPO_SSH"
    fi
fi

ensure_subtree() {
    if [ ! -d "$TAGMA_PREFIX/sw/rust/core/src" ]; then
        echo "tagma: adding subtree from $TAGMA_REPO ($TAGMA_BRANCH)..."
        git subtree add --prefix "$TAGMA_PREFIX" --squash "$TAGMA_REPO" "$TAGMA_BRANCH"
    fi
}

case "${1:-}" in
    --refresh-tagma)
        echo "tagma: pulling latest subtree from $TAGMA_REPO ($TAGMA_BRANCH)..."
        git subtree pull --prefix "$TAGMA_PREFIX" --squash "$TAGMA_REPO" "$TAGMA_BRANCH"
        ;;
    --test)
        shift
        exec cargo test -p nex-tagma "$@"
        ;;
    --bench)
        exec cargo run -p nex-tagma -- bench
        ;;
    --help|-h)
        sed -n '3,14p' "$0"
        exit 0
        ;;
    *)
        ensure_subtree
        echo "--- build ---"
        cargo build -p nex-tagma 2>&1
        echo "--- test ---"
        cargo test -p nex-tagma 2>&1
        echo "--- bench ---"
        cargo run -p nex-tagma -- bench 2>&1
        ;;
esac
