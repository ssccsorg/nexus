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

TUNNEL_CONFIG="${RAG_DIR}/config/tunnel-config-edgequake.yml"
API_HEALTH_URL="${API_HEALTH_URL:-http://127.0.0.1:8080/health}"
TIMEOUT_SEC=120

log_info()  { echo "[INFO]  $*"; }
log_warn()  { echo "[WARN]  $*"; }
log_error() { echo "[ERROR] $*"; }

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
  log_info "Checking for leftover containers..."

  local compose_dir
  compose_dir="$(dirname "$COMPOSE_FILE")"

  # If containers for this project are already up, tear them down gracefully
  if docker compose -f "$COMPOSE_FILE" ps -q 2>/dev/null | grep -q .; then
    log_warn "Existing EdgeQuake containers are running. Stopping them..."
    (cd "$compose_dir" && docker compose -f "$(basename "$COMPOSE_FILE")" down)
  fi

  # Remove any orphan containers with the same names to avoid conflicts
  for container in edgequake-postgres edgequake-api edgequake-frontend; do
    if docker ps -a --format '{{.Names}}' | grep -q "^${container}$"; then
      log_warn "Found conflicting container: ${container}. Removing it."
      docker rm -f "$container" 2>/dev/null || true
    fi
  done
}

########################################
# 3. START EDGEQUAKE STACK (LOCAL BUILD)
########################################
start_stack() {
  local compose_dir compose_basename
  compose_dir="$(dirname "$COMPOSE_FILE")"
  compose_basename="$(basename "$COMPOSE_FILE")"

  log_info "Starting EdgeQuake stack (local build)."
  log_info "Compose directory: ${compose_dir}"
  log_info "Compose file:      ${compose_basename}"

  # Check for Rust toolchain (needed for build)
  if ! command -v cargo &>/dev/null && ! command -v rustc &>/dev/null; then
    log_warn "Rust toolchain not detected. Docker will handle the build, but you may"
    log_warn "want to install it via https://rustup.rs for faster rebuilds."
  fi

  (cd "$compose_dir" && docker compose -f "$compose_basename" up -d --build)
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
# 5. CLEANUP
########################################
cleanup() {
  log_info "Shutting down..."
  if [ -n "${TUNNEL_PID:-}" ] && kill -0 "$TUNNEL_PID" 2>/dev/null; then
    log_info "Stopping tunnel..."
    kill "$TUNNEL_PID" 2>/dev/null || true
    wait "$TUNNEL_PID" 2>/dev/null || true
  fi
  log_info "Stopping Docker stack..."
  (cd "$(dirname "$COMPOSE_FILE")" && docker compose -f "$(basename "$COMPOSE_FILE")" down)
  log_info "All services stopped."
}

trap cleanup EXIT INT TERM

########################################
# MAIN
########################################
main() {
  check_deps
  cleanup_previous
  start_stack
  if ! wait_for_health; then
    log_error "API health check failed. Exiting."
    exit 1
  fi

  start_tunnel &
  TUNNEL_PID=$!
  log_info "Tunnel is running in background (PID: $TUNNEL_PID). Press Ctrl+C to stop."
  wait "$TUNNEL_PID"
  log_info "Tunnel exited."
}

main