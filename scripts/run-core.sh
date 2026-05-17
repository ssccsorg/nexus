#!/usr/bin/env bash
set -euo pipefail
#
# nexus-core — Local CI runner (legacy wrapper)
#
# Delegates to root run.sh --core.
# Kept for backward compatibility; new usage should use the root run.sh.
#
# Usage:
#   scripts/run-core.sh         # Same as ./run.sh --core
#   scripts/run-core.sh --test  # Same as ./run.sh --core (flags ignored)
#

cd "$(dirname "$0")/.."

MODE="${1:-all}"

case $MODE in
    --check|--clippy|--test|all)
        exec ./run.sh --core
        ;;
    *)
        echo "Usage: $0 [--check|--clippy|--test]"
        exit 1
        ;;
esac
