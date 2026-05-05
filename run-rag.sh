#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# run-rag.sh — EdgeQuake + LightRAG unified launcher
# =============================================================================
# Usage:
#   ./run-rag.sh                          # LightRAG (default)
#   ./run-rag.sh --engine lightrag        # LightRAG
#   ./run-rag.sh --engine edgequake       # EdgeQuake (deprecated, Docker)
#   ./run-rag.sh --refresh                # Reset data before start
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RAG_DIR="${RAG_DIR:-${SCRIPT_DIR}/rag}"

# ---- Engine selection -------------------------------------------------------
ENGINE="${ENGINE:-lightrag}"
REFRESH_MODE="false"

# ---- LLM / Embedding (shared) -----------------------------------------------
LLM_PROVIDER="${LLM_PROVIDER:-lmstudio}"
LMSTUDIO_URL="${LMSTUDIO_URL:-http://localhost:1234}"
LLM_MODEL="${LLM_MODEL:-qwen2.5-coder-7b-instruct-mlx}"
EMBEDDING_PROVIDER="${EMBEDDING_PROVIDER:-lmstudio}"
EMBEDDING_MODEL="${EMBEDDING_MODEL:-text-embedding-nomic-embed-text-v1.5}"
EMBEDDING_DIMENSION="${EMBEDDING_DIMENSION:-}"

# ---- LightRAG ---------------------------------------------------------------
LIGHTRAG_VENV="${LIGHTRAG_VENV:-${RAG_DIR}/lightrag-env}"
LIGHTRAG_PYTHON="${LIGHTRAG_VENV}/bin/python3.11"
LIGHTRAG_PORT="${LIGHTRAG_PORT:-9621}"
LIGHTRAG_HOST="${LIGHTRAG_HOST:-0.0.0.0}"
LIGHTRAG_DATA="${LIGHTRAG_DATA:-${RAG_DIR}/lightrag-data}"
LIGHTRAG_TUNNEL="${RAG_DIR}/tunnel-config-lightrag.yml"

# ---- EdgeQuake (deprecated, Docker) -----------------------------------------
EDGEQUAKE_REPO_DIR="${EDGEQUAKE_REPO_DIR:-${RAG_DIR}/edgequake}"
EDGEQUAKE_COMPOSE="${EDGEQUAKE_REPO_DIR}/edgequake/docker/docker-compose.yml"
EDGEQUAKE_PORT="${EDGEQUAKE_PORT:-8080}"
EDGEQUAKE_TUNNEL="${RAG_DIR}/tunnel-config-edgequake.yml"
WORKSPACE_ID="${WORKSPACE_ID:-default}"

# ---- Tunnel -----------------------------------------------------------------
TIMEOUT_SEC="${TIMEOUT_SEC:-120}"

# ---- Helpers ----------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info()  { echo "[INFO]  $*" >&2; }
log_warn()  { echo "[WARN]  $*"; }
log_error() { echo "[ERROR] $*"; }

usage() {
  cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --engine lightrag|edgequake   Select RAG engine (default: lightrag)
  --refresh                     Wipe all data before starting
  --help                        Show this message

Environment:
  LLM_PROVIDER       LLM backend (lmstudio, ollama)  [default: lmstudio]
  LLM_MODEL          Model name                       [default: qwen2.5-coder-7b-instruct-mlx]
  EMBEDDING_MODEL    Embedding model name             [default: text-embedding-nomic-embed-text-v1.5]
  EMBEDDING_DIMENSION  Override auto-detected dim
  LIGHTRAG_PORT      LightRAG server port             [default: 9621]
  ENGINE             Engine override                  [default: lightrag]
EOF
  exit 0
}

# =============================================================================
# Prerequisites
# =============================================================================

check_lm_studio() {
  echo -n "  LM Studio (${LMSTUDIO_URL}) ... "
  if curl -s "${LMSTUDIO_URL}/v1/models" > /dev/null 2>&1; then
    echo "OK"
    return 0
  else
    echo "${RED}NOT REACHABLE"
    echo "    Start LM Studio and load a model first."
    return 1
  fi
}

check_cloudflared() {
  if command -v cloudflared &>/dev/null; then
    return 0
  fi
  log_warn "cloudflared not found. Downloading..."
  local os arch url dest
  case "$(uname -s)" in
    Linux)  os="linux" ;;
    Darwin) os="darwin" ;;
    *) log_error "Unsupported OS"; return 1 ;;
  esac
  case "$(uname -m)" in
    x86_64)  arch="amd64" ;;
    arm64|aarch64) arch="arm64" ;;
    *) log_error "Unsupported arch"; return 1 ;;
  esac
  dest="/usr/local/bin/cloudflared"
  url="https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-${os}-${arch}"
  if [ -f "$dest" ]; then
    log_info "${dest} already exists."
    return 0
  fi
  curl -fsSL "$url" -o cloudflared && chmod +x cloudflared
  sudo mv cloudflared "$dest" 2>/dev/null || {
    mv cloudflared ./cloudflared
    export PATH="$PWD:$PATH"
  }
  log_info "cloudflared installed."
}

detect_embedding_dimension() {
  local base_url="$1" model="$2"
  if [ -n "${EMBEDDING_DIMENSION:-}" ]; then
    echo "${EMBEDDING_DIMENSION}"
    return 0
  fi
  log_info "Probing embedding dimension for ${model}..."
  local response dim
  response=$(curl -s -X POST "${base_url}/v1/embeddings" \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"${model}\",\"input\":\"test\"}" 2>/dev/null) || true
  dim=$(echo "$response" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    vec = data['data'][0]['embedding']
    print(len(vec))
except Exception:
    print('')" 2>/dev/null) || true
  if [ -n "$dim" ]; then
    log_info "Detected embedding dimension: ${dim}"
    echo "${dim}"
  else
    log_warn "Could not detect dimension, falling back to 768"
    echo "768"
  fi
}

# =============================================================================
# LightRAG engine
# =============================================================================

check_lightrag() {
  echo -n "  Python venv (${LIGHTRAG_VENV}) ... "
  if [ -f "${LIGHTRAG_PYTHON}" ]; then
    echo "OK"
    return 0
  else
    echo "${RED}NOT FOUND"
    echo "    Create the venv:"
    echo "      python3.11 -m venv ${LIGHTRAG_VENV}"
    echo "      ${LIGHTRAG_VENV}/bin/pip install \"lightrag-hku[api]\""
    return 1
  fi
}

start_lightrag() {
  local detected_dim
  detected_dim=$(detect_embedding_dimension "${LMSTUDIO_URL}" "${EMBEDDING_MODEL}")

  mkdir -p "${LIGHTRAG_DATA}" "${RAG_DIR}/logs"

  export LLM_BINDING=openai
  export LLM_BINDING_HOST="${LMSTUDIO_URL}/v1"
  export LLM_BINDING_API_KEY=lm-studio
  export CHAT_MODEL="${LLM_MODEL}"

  export EMBEDDING_BINDING=openai
  export EMBEDDING_BINDING_HOST="${LMSTUDIO_URL}/v1"
  export EMBEDDING_BINDING_API_KEY=lm-studio
  export EMBEDDING_MODEL="${EMBEDDING_MODEL}"
  export EMBEDDING_DIM="${detected_dim}"

  export WORKING_DIR="${LIGHTRAG_DATA}"
  export HOST="${LIGHTRAG_HOST}"
  export PORT="${LIGHTRAG_PORT}"

  log_info "Starting LightRAG on ${LIGHTRAG_HOST}:${LIGHTRAG_PORT}"
  log_info "  LLM    : ${CHAT_MODEL}"
  log_info "  Embed  : ${EMBEDDING_MODEL} (${detected_dim}d)"
  log_info "  Data   : ${LIGHTRAG_DATA}"

  exec "${LIGHTRAG_PYTHON}" -m lightrag.api.lightrag_server \
    --host "${LIGHTRAG_HOST}" \
    --port "${LIGHTRAG_PORT}" \
    --working-dir "${LIGHTRAG_DATA}" \
    --llm-binding openai \
    --embedding-binding openai \
    --log-level INFO
}

# =============================================================================
# EdgeQuake engine (deprecated, Docker)
# =============================================================================

check_edgequake() {
  if ! command -v docker &>/dev/null; then
    log_error "Docker is required for EdgeQuake."
    return 1
  fi
  if ! docker compose version &>/dev/null; then
    log_error "Docker Compose v2 is required."
    return 1
  fi
  if [ ! -f "$EDGEQUAKE_COMPOSE" ]; then
    log_error "EdgeQuake compose file not found: ${EDGEQUAKE_COMPOSE}"
    return 1
  fi
  return 0
}

start_edgequake() {
  local detected_dim
  detected_dim=$(detect_embedding_dimension "${LMSTUDIO_URL}" "${EMBEDDING_MODEL}")

  # LM Studio via OpenAI-compatible provider
  export EDGEQUAKE_LLM_PROVIDER=openai
  export EDGEQUAKE_LLM_MODEL="${LLM_MODEL}"
  export OPENAI_BASE_URL="${LMSTUDIO_URL}/v1"
  export OPENAI_API_KEY=not-needed

  export EDGEQUAKE_EMBEDDING_PROVIDER=openai
  export EDGEQUAKE_EMBEDDING_MODEL="${EMBEDDING_MODEL}"
  export EDGEQUAKE_EMBEDDING_BASE_URL="${LMSTUDIO_URL}/v1"
  export EDGEQUAKE_EMBEDDING_DIMENSION="${detected_dim}"

  local compose_dir
  compose_dir="$(dirname "$EDGEQUAKE_COMPOSE")"

  if [ "$REFRESH_MODE" = "true" ]; then
    log_warn "Refresh mode: removing containers and volumes."
    (cd "$compose_dir" && docker compose -f "$(basename "$EDGEQUAKE_COMPOSE")" down -v)
  else
    (cd "$compose_dir" && docker compose -f "$(basename "$EDGEQUAKE_COMPOSE")" down 2>/dev/null || true)
  fi

  log_info "Starting EdgeQuake (Docker)..."
  (cd "$compose_dir" && docker compose -f "$(basename "$EDGEQUAKE_COMPOSE")" up -d --build)

  # Wait for health
  local api_url="http://127.0.0.1:${EDGEQUAKE_PORT}/health"
  log_info "Waiting for EdgeQuake health check..."
  local start_time
  start_time=$(date +%s)
  while true; do
    if curl -s -o /dev/null -w "%{http_code}" "$api_url" | grep -q '^2'; then
      log_info "EdgeQuake API is healthy."
      break
    fi
    if (( $(date +%s) - start_time > TIMEOUT_SEC )); then
      log_error "EdgeQuake did not become healthy within ${TIMEOUT_SEC}s."
      exit 1
    fi
    sleep 2
  done
}

# =============================================================================
# Tunnel (shared)
# =============================================================================

start_tunnel() {
  local config="$1"
  log_info "Starting Cloudflare Tunnel: ${config}"
  cloudflared tunnel --config "$config" run
}

# =============================================================================
# Cleanup
# =============================================================================

cleanup() {
  log_info "Shutting down..."
  kill_previous
  log_info "All services stopped."
}

kill_previous() {
  log_info "Stopping any existing services..."

  # Kill existing LightRAG server
  if lsof -ti ":${LIGHTRAG_PORT}" > /dev/null 2>&1; then
    log_info "  Killing LightRAG on port ${LIGHTRAG_PORT}..."
    lsof -ti ":${LIGHTRAG_PORT}" | xargs kill -9 2>/dev/null || true
  fi

  # Kill existing cloudflared tunnels
  if pgrep -f "cloudflared.*tunnel" > /dev/null 2>&1; then
    log_info "  Killing cloudflared tunnels..."
    pkill -9 -f "cloudflared.*tunnel" 2>/dev/null || true
  fi

  # Kill existing EdgeQuake containers
  if [ "$ENGINE" = "edgequake" ]; then
    local compose_dir
    compose_dir="$(dirname "$EDGEQUAKE_COMPOSE")"
    (cd "$compose_dir" && docker compose -f "$(basename "$EDGEQUAKE_COMPOSE")" down 2>/dev/null || true)
  fi

  sleep 1
  log_info "Cleanup complete."
}

trap cleanup EXIT INT TERM

# =============================================================================
# Main
# =============================================================================

main() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --engine) ENGINE="$2"; shift 2 ;;
      --refresh) REFRESH_MODE="true"; shift ;;
      --help) usage ;;
      *) log_error "Unknown option: $1"; usage ;;
    esac
  done

  echo ""
  echo "${CYAN}============================================================"
  echo "${CYAN}  RAG Launcher — Engine: ${ENGINE}"
  echo "${CYAN}============================================================"
  echo ""

  kill_previous

  # Shared checks
  check_lm_studio || exit 1
  check_cloudflared || exit 1

  case "$ENGINE" in
    lightrag)
      check_lightrag || exit 1

      if [ "$REFRESH_MODE" = "true" ]; then
        log_warn "Refresh: clearing LightRAG data at ${LIGHTRAG_DATA}"
        rm -rf "${LIGHTRAG_DATA}"
      fi

      mkdir -p "${RAG_DIR}/logs"
      start_lightrag > "${RAG_DIR}/logs/lightrag-server.log" 2>&1 &
      LIGHTRAG_PID=$!

      log_info "Waiting for LightRAG health check..."
      local start_time
      start_time=$(date +%s)
      while true; do
        if curl -s "http://127.0.0.1:${LIGHTRAG_PORT}/health" | grep -q '"healthy"'; then
          log_info "LightRAG server healthy."
          break
        fi
        if (( $(date +%s) - start_time > TIMEOUT_SEC )); then
          log_error "LightRAG did not become healthy within ${TIMEOUT_SEC}s."
          exit 1
        fi
        sleep 2
      done

      log_info "Starting Cloudflare Tunnel..."
      start_tunnel "$LIGHTRAG_TUNNEL" 2>&1 &
      TUNNEL_PID=$!

      # Wait for tunnel to register (cloudflared prints "Registered tunnel connection")
      local tunnel_ready=0
      local tunnel_wait=0
      while (( tunnel_wait < 30 )); do
        if curl -s "http://127.0.0.1:${LIGHTRAG_PORT}/health" | grep -q '"healthy"'; then
          tunnel_ready=1
          break
        fi
        sleep 1
        ((tunnel_wait++))
      done

      echo ""
      echo "============================================================"
      echo "  Server is ready to accept connections"
      echo "============================================================"
      echo ""
      echo "  Local:    http://127.0.0.1:${LIGHTRAG_PORT}"
      echo "  Web UI:   http://127.0.0.1:${LIGHTRAG_PORT}/webui"
      echo "  API docs: http://127.0.0.1:${LIGHTRAG_PORT}/docs"
      echo "  Public:   https://rag-api.nexus.ssccs.org"
      echo "  Logs:     ${RAG_DIR}/logs/"
      echo ""

      wait "$TUNNEL_PID"
      ;;

    edgequake)
      check_edgequake || exit 1
      start_edgequake
      mkdir -p "${RAG_DIR}/logs"
      log_info "Starting Cloudflare Tunnel..."
      start_tunnel "$EDGEQUAKE_TUNNEL" 2>&1 &
      TUNNEL_PID=$!

      # Wait for tunnel to register
      sleep 6

      echo ""
      echo "============================================================"
      echo "  Server is ready to accept connections"
      echo "============================================================"
      echo ""
      echo "  Local:    http://127.0.0.1:${EDGEQUAKE_PORT}"
      echo "  Public:   https://rag-api.nexus.ssccs.org"
      echo "  Logs:     ${RAG_DIR}/logs/"
      echo ""
      wait "$TUNNEL_PID"
      ;;

    *)
      log_error "Unknown engine: ${ENGINE}. Use lightrag or edgequake."
      exit 1
      ;;
  esac
}

main "$@"
