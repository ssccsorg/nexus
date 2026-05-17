#!/usr/bin/env bash
set -euo pipefail
#
# nexus — Unified CI runner
#
# By default runs everything (core checks + consumer playbooks).
# Sub-commands for focused tasks.
#
# Usage:
#   ./run.sh              # Everything (default)
#   ./run.sh --core       # Core checks only
#   ./run.sh --playbooks  # Consumer playbooks only
#   ./run.sh --gateway    # Start gateway server only
#

cd "$(dirname "$0")"

case "${1:-}" in
    --core)
        shift
        exec ./scripts/run-core.sh "$@"
        ;;
    --playbooks)
        exec ./playbooks/run.sh
        ;;
    --gateway)
        exec ./scripts/run-gateway.sh
        ;;
    --help|-h)
        echo "Usage: $0 [OPTION]"
        echo "  (no arg)     Core checks + playbooks [default]"
        echo "  --core       Core checks only"
        echo "  --playbooks  Consumer playbooks only"
        echo "  --gateway    Start gateway API server"
        ;;
    "")
        # Default: run everything
        echo "=== Core ==="
        ./scripts/run-core.sh
        echo ""
        echo "=== Playbooks ==="
        ./playbooks/run.sh
        ;;
    *)
        echo "Unknown: $1"
        echo "Usage: $0 [--core|--playbooks|--gateway]"
        exit 1
        ;;
esac
