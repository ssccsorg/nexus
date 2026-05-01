#!/usr/bin/env bash
set -euo pipefail

########################################
# CONFIGURATION
########################################
RAG_DIR="${RAG_DIR:-rag}"

# EdgeQuake repository root (contains the main docker-compose.yml)
EDGEQUAKE_REPO_DIR="${EDGEQUAKE_REPO_DIR:-${RAG_DIR}/edgequake}"

# Local build compose file (the default compose file in the repo root)
COMPOSE_FILE="${EDGEQUAKE_REPO_DIR}/edgequake/docker/docker-compose.yml"

TUNNEL_CONFIG="${RAG_DIR}/tunnel-config-edgequake.yml"
# Script directory for resolving local file paths
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
API_HEALTH_URL="${API_HEALTH_URL:-http://127.0.0.1:8080/health}"
TIMEOUT_SEC=120

# Refresh mode flag (default false)
REFRESH_MODE="false"

# ── LLM Provider ─────────────────────────────────────────────────────────
# Options: ollama (default), lmstudio (via OpenAI-compatible API)
LLM_PROVIDER="${LLM_PROVIDER:-lmstudio}"
LMSTUDIO_URL="${LMSTUDIO_URL:-http://host.docker.internal:1234}"
LLM_MODEL="${LLM_MODEL:-liquid/lfm2-24b-a2b}"

# ── Workspace ─────────────────────────────────────────────────────────────
# EdgeQuake workspace to sync provider settings into.
WORKSPACE_ID="${WORKSPACE_ID:-default}"

# ── Embedding Provider ────────────────────────────────────────────────────
# When LLM_PROVIDER=lmstudio, embedding also uses LM Studio by default.
EMBEDDING_PROVIDER="${EMBEDDING_PROVIDER:-lmstudio}"
EMBEDDING_MODEL="${EMBEDDING_MODEL:-text-embedding-embeddinggemma-300m-qat}"
# Auto-detected from LM Studio API. Set manually only if detection fails:
# EMBEDDING_DIMENSION=1024 ./run-rag.sh
EMBEDDING_DIMENSION="${EMBEDDING_DIMENSION:-}"

log_info()  { echo "[INFO]  $*"; }
log_warn()  { echo "[WARN]  $*"; }
log_error() { echo "[ERROR] $*"; }

usage() {
  cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --refresh   Completely reset EdgeQuake data (delete all containers AND database volumes).
  --help      Show this message.

Without --refresh, the script preserves existing database and data volumes.
EOF
  exit 0
}

########################################
# 1. PREREQUISITES
########################################
check_deps() {
  local missing=0

  if ! command -v docker &>/dev/null; then
    log_error "Docker is not installed."
    missing=1
  fi

  if ! docker compose version &>/dev/null; then
    log_error "Docker Compose plugin is required. Please install Docker Compose v2."
    missing=1
  fi

  if ! command -v cloudflared &>/dev/null; then
    log_warn "cloudflared not found in PATH. Attempting automatic download..."
    if ! download_cloudflared; then
      log_error "Failed to install cloudflared automatically. Please install it manually."
      missing=1
    fi
  fi

  if [ ! -f "$COMPOSE_FILE" ]; then
    log_error "Compose file '${COMPOSE_FILE}' not found."
    log_error "The EdgeQuake repository should be cloned at ${EDGEQUAKE_REPO_DIR}."
    log_error "Example: git clone https://github.com/raphaelmansuy/edgequake.git ${EDGEQUAKE_REPO_DIR}"
    missing=1
  fi

  if [ ! -f "$TUNNEL_CONFIG" ]; then
    log_error "Tunnel config file '${TUNNEL_CONFIG}' not found."
    missing=1
  fi

  return $missing
}

# ── Probe LM Studio for actual embedding dimension ───────────────────
# Sends a test embedding request and reads the vector length from the
# API response. Avoids hardcoding fragile dimension values.
detect_embedding_dimension() {
  local base_url="$1"
  local model="$2"
  local api_url="${base_url}/v1/embeddings"

  # Use explicit override if provided via env var
  if [ -n "${EMBEDDING_DIMENSION:-}" ]; then
    echo "${EMBEDDING_DIMENSION}"
    return 0
  fi

  log_info "Probing embedding dimension from ${api_url}..."
  local response
  response=$(curl -s -X POST "${api_url}" \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"${model}\",\"input\":\"test\"}" 2>/dev/null) || true

  local dim
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
    log_warn "Could not detect embedding dimension, falling back to 768"
    echo "768"
  fi
}

download_cloudflared() {
  local os arch url dest

  case "$(uname -s)" in
    Linux)  os="linux" ;;
    Darwin) os="darwin" ;;
    *)
      log_error "Unsupported OS: $(uname -s)"
      return 1
      ;;
  esac

  case "$(uname -m)" in
    x86_64)  arch="amd64" ;;
    arm64|aarch64) arch="arm64" ;;
    *)
      log_error "Unsupported architecture: $(uname -m)"
      return 1
      ;;
  esac

  dest="/usr/local/bin/cloudflared"
  url="https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-${os}-${arch}"

  if [ -f "$dest" ]; then
    log_info "${dest} already exists. Skipping download."
    return 0
  fi

  log_info "Downloading: ${url}"
  if curl -fsSL "$url" -o cloudflared; then
    chmod +x cloudflared
    sudo mv cloudflared "$dest" 2>/dev/null || {
      log_warn "Cannot write to /usr/local/bin; placing cloudflared in current directory."
      mv cloudflared ./cloudflared
      export PATH="$PWD:$PATH"
    }
    log_info "cloudflared installed successfully."
    return 0
  else
    return 1
  fi
}

########################################
# 2. CLEANUP PREVIOUS CONTAINERS
########################################
cleanup_previous() {
  local compose_dir
  compose_dir="$(dirname "$COMPOSE_FILE")"

  if [ "$REFRESH_MODE" = "true" ]; then
    log_warn "Refresh mode active: removing all containers AND database volumes."
    if docker compose -f "$COMPOSE_FILE" ps -q 2>/dev/null | grep -q .; then
      (cd "$compose_dir" && docker compose -f "$(basename "$COMPOSE_FILE")" down -v)
    fi
    # Also remove any orphan containers (should not be needed after down -v, but safe)
    for container in edgequake-postgres edgequake-api edgequake-frontend; do
      docker rm -f "$container" 2>/dev/null || true
    done
  else
    log_info "Preserving existing data. Removing containers only."
    if docker compose -f "$COMPOSE_FILE" ps -q 2>/dev/null | grep -q .; then
      (cd "$compose_dir" && docker compose -f "$(basename "$COMPOSE_FILE")" down)
    fi
    # Remove any orphan containers (same names) to avoid conflicts, but keep volumes
    for container in edgequake-postgres edgequake-api edgequake-frontend; do
      if docker ps -a --format '{{.Names}}' | grep -q "^${container}$"; then
        log_warn "Found conflicting container: ${container}. Removing it (volume intact)."
        docker rm -f "$container" 2>/dev/null || true
      fi
    done
  fi
}

########################################
# 3. START EDGEQUAKE STACK (LOCAL BUILD)
########################################
start_stack() {
  local compose_dir compose_basename override_file
  compose_dir="$(dirname "$COMPOSE_FILE")"
  compose_basename="$(basename "$COMPOSE_FILE")"
  # Generated inline – adds env vars missing from upstream compose
  override_file="${SCRIPT_DIR}/docker-compose.nexus-af-sync.yml"

  # Generate override on every run so it never drifts
  cat > "${override_file}" <<'EOF'
# Auto-generated by run-rag.sh – adds env vars NOT in upstream compose.
services:
  edgequake:
    environment:
      - EDGEQUAKE_LLM_MODEL=${EDGEQUAKE_LLM_MODEL:-}
      - EDGEQUAKE_EMBEDDING_MODEL=${EDGEQUAKE_EMBEDDING_MODEL:-}
      - EDGEQUAKE_EMBEDDING_BASE_URL=${EDGEQUAKE_EMBEDDING_BASE_URL:-}
      - EDGEQUAKE_EMBEDDING_DIMENSION=${EDGEQUAKE_EMBEDDING_DIMENSION:-}
EOF

  log_info "Starting EdgeQuake stack (local build)."
  log_info "Compose directory: ${compose_dir}"
  log_info "Compose file:      ${compose_basename}"
  log_info "Override file:     ${override_file}"

  # Check for Rust toolchain (needed for build)
  if ! command -v cargo &>/dev/null && ! command -v rustc &>/dev/null; then
    log_warn "Rust toolchain not detected. Docker will handle the build, but you may"
    log_warn "want to install it via https://rustup.rs for faster rebuilds."
  fi

  if [ -f "${override_file}" ]; then
    log_info "Override file:     ${override_file}"
    (cd "$compose_dir" && docker compose \
      -f "$compose_basename" \
      -f "${override_file}" \
      up -d --build)
  else
    (cd "$compose_dir" && docker compose -f "$compose_basename" up -d --build)
  fi
}

wait_for_health() {
  log_info "Waiting for API health check (timeout: ${TIMEOUT_SEC}s)..."
  local start_time
  start_time=$(date +%s)

  while true; do
    if curl -s -o /dev/null -w "%{http_code}" "$API_HEALTH_URL" | grep -q '^2'; then
      log_info "EdgeQuake API is healthy."
      return 0
    fi

    local now
    now=$(date +%s)
    if (( now - start_time > TIMEOUT_SEC )); then
      log_error "API did not respond within ${TIMEOUT_SEC}s."
      log_error "Check logs: cd ${compose_dir} && docker compose -f ${compose_basename} logs"
      return 1
    fi

    sleep 2
  done
}

########################################
# 4. CLOUDFLARE TUNNEL
########################################
start_tunnel() {
  log_info "Starting Cloudflare Tunnel (config: ${TUNNEL_CONFIG})..."
  cloudflared tunnel --config "$TUNNEL_CONFIG" run
}

########################################
# 5. CLEANUP ON EXIT (never removes volumes)
########################################
# ── Sync provider config into the stable default workspace ─────────
# The default workspace has a hardcoded UUID that never changes across
# cycles or DB resets. Direct PUT avoids fragile slug-to-UUID resolution.
sync_workspace_config() {
  local ws_id="$1"
  local api_base="http://127.0.0.1:8080/api/v1"
  # Stable UUID for the built-in default workspace
  local default_ws_uuid="00000000-0000-0000-0000-000000000003"

  local payload
  payload=$(cat <<EOF
{
  "llm_provider": "openai",
  "llm_model": "${LLM_MODEL}",
  "embedding_provider": "${EMBEDDING_PROVIDER}",
  "embedding_model": "${EMBEDDING_MODEL}",
  "embedding_dimension": ${EMBEDDING_DIMENSION:-768}
}
EOF
)

  local http_code
  http_code=$(curl -s -o /dev/null -w "%{http_code}" -X PUT \
    "${api_base}/workspaces/${default_ws_uuid}" \
    -H "Content-Type: application/json" \
    -d "${payload}" 2>/dev/null) || true

  if [ "$http_code" = "200" ] || [ "$http_code" = "204" ]; then
    log_info "Default workspace updated: provider settings synced."
  else
    log_warn "Workspace update returned HTTP ${http_code}"
  fi
}

cleanup() {
  log_info "Shutting down gracefully (data preserved)..."
  if [ -n "${TUNNEL_PID:-}" ] && kill -0 "$TUNNEL_PID" 2>/dev/null; then
    log_info "Stopping tunnel..."
    kill "$TUNNEL_PID" 2>/dev/null || true
    wait "$TUNNEL_PID" 2>/dev/null || true
  fi
  log_info "Stopping Docker containers (volumes are kept)."
  (cd "$(dirname "$COMPOSE_FILE")" && docker compose -f "$(basename "$COMPOSE_FILE")" down)
  log_info "All services stopped."
}

trap cleanup EXIT INT TERM

########################################
# MAIN
########################################
main() {
  # Parse arguments
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --refresh)
        REFRESH_MODE="true"
        shift
        ;;
      --help)
        usage
        ;;
      *)
        log_error "Unknown option: $1"
        usage
        ;;
    esac
  done

  check_deps

  # ── Bypass: LM Studio via OpenAI-compatible provider ───────────────
  # LM Studio exposes an OpenAI-compatible API, so we use the openai
  # provider with a custom base_url. No subtree edits needed.
  if [ "$LLM_PROVIDER" = "lmstudio" ]; then
    log_info "LLM Provider: LM Studio (${LMSTUDIO_URL})"
    log_info "LLM Model: ${LLM_MODEL}"
    export EDGEQUAKE_LLM_PROVIDER=openai
    export EDGEQUAKE_LLM_MODEL="${LLM_MODEL}"
    export OPENAI_BASE_URL="${LMSTUDIO_URL}/v1"
    export OPENAI_API_KEY=not-needed
  else
    log_info "LLM Provider: Ollama"
  fi

  # Embedding: same bypass logic when using LM Studio
  if [ "$EMBEDDING_PROVIDER" = "lmstudio" ] || [ "$LLM_PROVIDER" = "lmstudio" ]; then
    local detected_dim
    detected_dim=$(detect_embedding_dimension "${LMSTUDIO_URL}" "${EMBEDDING_MODEL}")
    export EDGEQUAKE_EMBEDDING_PROVIDER=openai
    export EDGEQUAKE_EMBEDDING_MODEL="${EMBEDDING_MODEL}"
    export EDGEQUAKE_EMBEDDING_BASE_URL="${LMSTUDIO_URL}/v1"
    export EDGEQUAKE_EMBEDDING_DIMENSION="${detected_dim}"
    log_info "Embedding: LM Studio / ${EMBEDDING_MODEL} (dim: ${detected_dim})"
  else
    export EDGEQUAKE_EMBEDDING_PROVIDER="${EMBEDDING_PROVIDER}"
    export EDGEQUAKE_EMBEDDING_MODEL="${EMBEDDING_MODEL}"
    log_info "Embedding: ${EMBEDDING_PROVIDER} / ${EMBEDDING_MODEL}"
  fi

  cleanup_previous
  start_stack
  if ! wait_for_health; then
    log_error "API health check failed. Exiting."
    exit 1
  fi

  # Sync provider settings into the workspace so the frontend sees them.
  log_info "Syncing workspace '${WORKSPACE_ID}' provider config..."
  sync_workspace_config "${WORKSPACE_ID}"

  start_tunnel &
  TUNNEL_PID=$!
  log_info "Tunnel is running in background (PID: $TUNNEL_PID). Press Ctrl+C to stop."
  wait "$TUNNEL_PID"
  log_info "Tunnel exited."
}

main "$@"
