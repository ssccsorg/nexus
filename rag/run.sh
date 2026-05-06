#!/usr/bin/env bash
# run.sh — RAG launcher (docker compose profiles)
# Usage:
#   ./run.sh              # lightrag (default)
#   ./run.sh edgequake    # edgequake
#   ./run.sh --refresh    # lightrag, reset data
#   ./run.sh edgequake --refresh
set -euo pipefail

cd "$(dirname "$0")"

ENGINE="${1:-lightrag}"
REFRESH=""

for arg in "$@"; do
  case "$arg" in
    --refresh) REFRESH="1" ;;
    lightrag|edgequake) ENGINE="$arg" ;;
  esac
done

echo "=== RAG: $ENGINE ==="

if [ -n "${REFRESH:-}" ]; then
  echo "[refresh] removing containers + volumes"
  docker compose --profile "$ENGINE" down -v
fi

docker compose --profile "$ENGINE" up -d --wait

echo ""
echo "  ready: $ENGINE"
echo ""
docker compose --profile "$ENGINE" ps
