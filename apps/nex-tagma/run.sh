#!/usr/bin/env bash
set -euo pipefail
#
# nex-tagma — Standard reference implementation runner
#
# Usage:
#   ./run.sh                    # Build + test + bench
#   ./run.sh --test             # Run tests only
#   ./run.sh --bench            # Run benchmark only
#   ./run.sh --help
#

cd "$(dirname "$0")"

case "${1:-}" in
    --test)
        shift
        exec cargo test "$@"
        ;;
    --bench)
        exec cargo run -- bench
        ;;
    --help|-h)
        sed -n '3,10p' "$0"
        exit 0
        ;;
    *)
        echo "--- build ---"
        cargo build 2>&1
        echo "--- test ---"
        cargo test 2>&1
        echo "--- bench ---"
        cargo run -- bench 2>&1
        ;;
esac
