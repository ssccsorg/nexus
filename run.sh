#!/usr/bin/env bash
set -euo pipefail
#
# nexus — Unified CI runner
#
# Comprehensive entry point for all local operations.
# Usage:
#   ./run.sh              # Core checks (default)
#   ./run.sh --core       # Core checks (fmt + check + clippy + test)
#   ./run.sh --gateway    # Start gateway server
#   ./run.sh --playbooks  # Run all consumer playbooks (starts/stops server)
#   ./run.sh --all        # Everything
#

cd "$(dirname "$0")"

case "${1:---core}" in
    --core|--check|--test)
        exec ./scripts/run-core.sh "$@"
        ;;
    --gateway)
        exec ./scripts/run-gateway.sh
        ;;
    --playbooks)
        exec ./playbooks/run.sh
        ;;
    --all)
        echo "=== Core ==="
        ./scripts/run-core.sh
        echo ""
        echo "=== Playbooks ==="
        ./playbooks/run.sh
        ;;
    --help|-h)
        echo "Usage: $0 [OPTION]"
        echo "  --core       Core checks (fmt + check + clippy + test) [default]"
        echo "  --gateway    Start gateway API server"
        echo "  --playbooks  Run consumer playbooks (Python, Node.js, Rust)"
        echo "  --all        Run everything"
        ;;
    *)
        echo "Unknown: $1"
        echo "Usage: $0 [--core|--gateway|--playbooks|--all]"
        exit 1
        ;;
esac
