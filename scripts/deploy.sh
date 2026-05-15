#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../workers/af-sync"
npx wrangler deploy
