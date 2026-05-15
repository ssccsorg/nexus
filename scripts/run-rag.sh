#!/usr/bin/env bash
# run-rag — RAG engine launcher wrapper
set -euo pipefail
cd "$(dirname "$0")/../engines/rag"
exec ./run.sh "$@"
