#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../ext/memgraph"
exec ./run.sh "$@"
