#!/usr/bin/env bash
# graph-run — Standalone Memgraph + proxy launcher
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROXY_DIR="$SCRIPT_DIR/memgraph"
PROXY_PORT=7689
MG_PORT=7688
LAB_PORT=3001

cleanup() {
  echo "Shutting down..."
  docker rm -f memgraph-proxy 2>/dev/null || true
  docker rm -f memgraph-nexus 2>/dev/null || true
  exit 0
}
trap cleanup INT TERM

echo "=== Graph Launcher ==="
echo ""

# Memgraph
docker rm -f memgraph-nexus 2>/dev/null || true
docker run -d --name memgraph-nexus --restart unless-stopped \
  -p $MG_PORT:7687 -p $LAB_PORT:3000 -p 7444:7444 \
  memgraph/memgraph-platform >/dev/null
echo "  memgraph-nexus (bolt:localhost:$MG_PORT, lab:$LAB_PORT)"
for i in $(seq 1 15); do
  docker exec memgraph-nexus mgconsole --query "RETURN 1" 2>/dev/null | grep -q "1" && break
  sleep 1
done
echo "  → ready"

# Proxy
docker rm -f memgraph-proxy 2>/dev/null || true
docker build -q -t memgraph-proxy:latest -f "$PROXY_DIR/Dockerfile" "$PROXY_DIR" >/dev/null 2>&1 || true
docker run -d --name memgraph-proxy --restart unless-stopped \
  -p $PROXY_PORT:7689 \
  -e MEMGRAPH_HOST=host.docker.internal \
  -e MEMGRAPH_PORT=$MG_PORT \
  memgraph-proxy:latest >/dev/null
echo "  memgraph-proxy (http://localhost:$PROXY_PORT)"
sleep 2
echo "  → ready"

echo ""
echo "  Press Ctrl+C to stop."
while true; do sleep 1; done
