#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../ext/graphiti"
exec ./run.sh "$@"
