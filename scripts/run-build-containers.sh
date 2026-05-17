#!/usr/bin/env bash
set -euo pipefail
#
# Run by .github/workflows/build.yml.
# Builds Docker images for all ext/ containers.
#
# Usage:
#   scripts/run-build-containers.sh

cd "$(dirname "$0")/.."

echo "=== Building LightRAG ==="
docker build -t lightrag-nexus -f ext/lightrag/Dockerfile ext/lightrag

echo "=== Building Graphiti ==="
docker build -t graphiti-nexus -f ext/graphiti/Dockerfile ext/graphiti

echo "=== Building Memgraph proxy ==="
docker build -t memgraph-proxy -f ext/memgraph/Dockerfile ext/memgraph

echo "All container images built."
