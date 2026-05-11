#!/usr/bin/env bash
# nexus-run — Unified launcher for all Nexus services
# Orchestrates: LightRAG, Memgraph + proxy, tunnel, data import
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RAG_DIR="$SCRIPT_DIR/rag"
GRAPH_DIR="$SCRIPT_DIR/graph"
TUNNEL_ID="fe6dbde1-4bf1-4096-8f15-f8cc8fb16b87"
TUNNEL_CONFIG="$RAG_DIR/lightrag/tunnel-config.yml"

# Colors
CYAN='\033[0;36m'
NC='\033[0m'

cleanup() {
    echo ""
    echo -e "${CYAN}Shutting down all services...${NC}"
    # Stop engines (order: proxy first, then memgraph, then lightrag)
    docker rm -f memgraph-proxy 2>/dev/null || true
    docker rm -f memgraph-nexus 2>/dev/null || true
    docker rm -f lightrag-nexus 2>/dev/null || true
    # Stop tunnel
    pkill -f "cloudflared tunnel.*$TUNNEL_ID" 2>/dev/null || true
    echo -e "${CYAN}All services stopped.${NC}"
}

trap cleanup EXIT INT TERM

echo -e "${CYAN}============================================================${NC}"
echo -e "${CYAN}  Nexus — Unified Launcher${NC}"
echo -e "${CYAN}============================================================${NC}"
echo ""

# ---------------------------------------------------------------------------
# Pre-flight
# ---------------------------------------------------------------------------
echo "[1/7] Checking prerequisites..."
if ! command -v docker &>/dev/null; then echo "[ERROR] docker required"; exit 1; fi
if ! command -v cloudflared &>/dev/null; then echo "[ERROR] cloudflared required"; exit 1; fi
echo "  docker: ok"
echo "  cloudflared: ok"
echo ""

# ---------------------------------------------------------------------------
# Stop any existing services
# ---------------------------------------------------------------------------
echo "[2/7] Cleaning up existing services..."
docker rm -f memgraph-proxy memgraph-nexus lightrag-nexus 2>/dev/null || true
pkill -f "cloudflared tunnel.*$TUNNEL_ID" 2>/dev/null || true
echo "  done"
echo ""

# ---------------------------------------------------------------------------
# Start Memgraph
# ---------------------------------------------------------------------------
echo "[3/7] Starting Memgraph..."
docker run -d --name memgraph-nexus --restart unless-stopped \
  -p 7688:7687 -p 3001:3000 -p 7444:7444 \
  memgraph/memgraph-platform >/dev/null
echo "  memgraph-nexus (bolt:7688, lab:3001)"
# Wait for Memgraph
for i in $(seq 1 15); do
  if docker exec memgraph-nexus mgconsole --query "RETURN 1" 2>/dev/null | grep -q "1"; then
    break
  fi
  sleep 1
done
echo "  → ready"
echo ""

# ---------------------------------------------------------------------------
# Start Memgraph HTTP proxy
# ---------------------------------------------------------------------------
echo "[4/7] Starting Memgraph HTTP proxy..."
docker build -q -t memgraph-proxy:latest -f "$GRAPH_DIR/memgraph/Dockerfile" "$GRAPH_DIR/memgraph" >/dev/null
docker run -d --name memgraph-proxy --restart unless-stopped \
  -p 7689:7689 \
  -e MEMGRAPH_HOST=host.docker.internal \
  -e MEMGRAPH_PORT=7688 \
  -e PROXY_PORT=7689 \
  memgraph-proxy:latest >/dev/null
echo "  memgraph-proxy (http:7689)"
sleep 2
echo "  → ready"
echo ""

# ---------------------------------------------------------------------------
# Start LightRAG
# ---------------------------------------------------------------------------
echo "[5/7] Starting LightRAG..."
docker build -q -t lightrag-nexus:local -f "$RAG_DIR/lightrag/Dockerfile" "$RAG_DIR/lightrag" 2>/dev/null || true
# Use pre-built image if local build fails
LIGHTRAG_IMAGE="${LIGHTRAG_IMAGE:-lightrag-nexus:local}"
export LMSTUDIO_URL="${LMSTUDIO_URL:-http://host.docker.internal:1234}"
export LLM_MODEL="${LLM_MODEL:-}"
export EMBEDDING_MODEL="${EMBEDDING_MODEL:-}"
export EMBEDDING_DIM="${EMBEDDING_DIM:-768}"

docker run -d --name lightrag-nexus --restart unless-stopped \
  --add-host host.docker.internal:host-gateway \
  -p 9621:9621 \
  -v "$RAG_DIR/lightrag/data:/work/data" \
  -e HOST=0.0.0.0 -e PORT=9621 \
  -e "LLM_BINDING_HOST=${LMSTUDIO_URL}/v1" \
  -e "CHAT_MODEL=${LLM_MODEL}" \
  -e "EMBEDDING_BINDING_HOST=${LMSTUDIO_URL}/v1" \
  -e "EMBEDDING_MODEL=${EMBEDDING_MODEL}" \
  -e "EMBEDDING_DIM=${EMBEDDING_DIM}" \
  -e WORKING_DIR=/work/data \
  lightrag-nexus:local \
  --host 0.0.0.0 --port 9621 --working-dir /work/data \
  --workspace default --llm-binding openai --embedding-binding openai \
  --log-level INFO >/dev/null 2>&1 &
echo "  lightrag-nexus (api:9621)"
# Wait for LightRAG
for i in $(seq 1 30); do
  if curl -sf http://localhost:9621/health 2>/dev/null | grep -q "healthy"; then
    echo "  → ready"
    break
  fi
  sleep 2
done
echo ""

# ---------------------------------------------------------------------------
# Import LightRAG data → Memgraph
# ---------------------------------------------------------------------------
echo "[6/7] Importing LightRAG data into Memgraph..."
if [ -f "$GRAPH_DIR/memgraph/import_lightrag.py" ]; then
  python3 "$GRAPH_DIR/memgraph/import_lightrag.py" 2>&1
fi
echo ""

# ---------------------------------------------------------------------------
# Start unified tunnel
# ---------------------------------------------------------------------------
echo "[7/7] Starting Cloudflare Tunnel..."
cloudflared tunnel --config "$TUNNEL_CONFIG" run \
  --credentials-file "$HOME/.cloudflared/$TUNNEL_ID.json" \
  "$TUNNEL_ID" >/dev/null 2>&1 &
TUNNEL_PID=$!
sleep 4
echo "  tunnel (pid:$TUNNEL_PID)"
echo ""

# ---------------------------------------------------------------------------
# Ready
# ---------------------------------------------------------------------------
echo -e "${CYAN}============================================================${NC}"
echo -e "${CYAN}  All services ready${NC}"
echo -e "${CYAN}============================================================${NC}"
echo ""
echo "  LightRAG API:    http://localhost:9621"
echo "  Memgraph Proxy:  http://localhost:7689"
echo "  Memgraph Lab:    http://localhost:3001"
echo "  Tunnel config:   $TUNNEL_CONFIG"
echo ""
echo "  Press Ctrl+C to stop all services."
echo ""

# Block until Ctrl+C
while true; do sleep 1; done
