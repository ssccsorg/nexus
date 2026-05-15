#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../ext/edgequake"
exec ./run.sh "$@"
