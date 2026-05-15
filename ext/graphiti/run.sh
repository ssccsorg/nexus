#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXT_DIR="$(dirname "$SCRIPT_DIR")"
export EXT_DIR
export PYTHONPATH="${EXT_DIR}:${PYTHONPATH:-}"
exec python3 -m _runner --engine "$(basename "$SCRIPT_DIR")" "$@"
