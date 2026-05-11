#!/usr/bin/env bash
# graph-run — Graph engine launcher
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export GRAPH_DIR="${GRAPH_DIR:-${SCRIPT_DIR}}"
export PYTHONPATH="${SCRIPT_DIR}:${PYTHONPATH:-}"

cd "$SCRIPT_DIR"
exec python3 -m runners "$@"
