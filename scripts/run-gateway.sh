#!/usr/bin/env bash
set -euo pipefail
#
# gateway-api — Start the FIH Blackboard HTTP gateway server.
#
# Usage:
#   scripts/run-gateway.sh              # In-memory, port 3000
#   scripts/run-gateway.sh --db data.db # SQLite persistence
#   scripts/run-gateway.sh --port 8080  # Custom port
#

cd "$(dirname "$0")/../gateway/api"

PORT="3000"
DB=""

while [[ "$#" -gt 0 ]]; do
    case $1 in
        --db) DB="$2"; shift 2 ;;
        --port) PORT="$2"; shift 2 ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
done

echo "gateway-api: starting on port $PORT"

if [ -n "$DB" ]; then
    exec cargo run -- --db "$DB"
else
    exec cargo run
fi
