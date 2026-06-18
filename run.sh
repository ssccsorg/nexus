#!/usr/bin/env bash
set -euo pipefail
#
# nexus — Unified CI runner
#
# By default runs everything (core checks + gateway + playbooks).
# Sub-commands for focused tasks.
#
# Usage:
#   ./run.sh               # Everything (default)
#   ./run.sh --core        # Core checks only (nex, storage/*)
#   ./run.sh --gateway     # Gateway layer checks (api, nex-cf, serde-proxy)
#   ./run.sh --playbooks   # Consumer playbooks only
#

cd "$(dirname "$0")"

case "${1:-}" in
    --core)
        shift
        exec ./scripts/run-core.sh "$@"
        ;;
    --gateway)
        shift
        exec ./scripts/run-gateway.sh "$@"
        ;;
    --playbooks)
        exec ./playbooks/run.sh
        ;;
    --help|-h)
        echo "Usage: $0 [OPTION]"
        echo "  (no arg)      Core + gateway + playbooks [default]"
        echo "  --core        Core checks only (nex, storage/*)"
        echo "  --gateway     Gateway layer checks (api, nex-cf, serde-proxy)"
        echo "  --playbooks   Consumer playbooks only"
        ;;
    "")
        # Default: run everything
        echo "=== Core ==="
        ./scripts/run-core.sh
        echo ""
        echo "=== Gateway ==="
        ./scripts/run-gateway.sh
        echo ""
        echo "=== Playbooks ==="
        ./playbooks/run.sh
        ;;
    *)
        echo "Unknown: $1"
        echo "Usage: $0 [--core|--gateway|--playbooks]"
        exit 1
        ;;
esac
