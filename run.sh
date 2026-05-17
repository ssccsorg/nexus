#!/usr/bin/env bash
set -euo pipefail
#
# nexus — Unified CI runner
#
# Single entry point for local CI. Delegates to scripts/run-core.sh
# which mirrors .github/workflows/core.yml.
#
# Usage:
#   ./run.sh          # Full check: fmt + clippy + test
#   ./run.sh --check  # Check only
#   ./run.sh --test   # Test only
#

cd "$(dirname "$0")"
exec ./scripts/run-core.sh "$@"
