#!/usr/bin/env bash
# run-memgraph — Memgraph engine launcher wrapper
set -euo pipefail
cd "$(dirname "$0")/../engines/memgraph"
exec ./run.sh "$@"
