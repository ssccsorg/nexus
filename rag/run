#!/usr/bin/env bash
# run-rag — Python-based RAG launcher
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export RAG_DIR="${RAG_DIR:-${SCRIPT_DIR}}"
export PYTHONPATH="${SCRIPT_DIR}:${PYTHONPATH:-}"

cd "$SCRIPT_DIR"
exec python3 -m runners "$@"
