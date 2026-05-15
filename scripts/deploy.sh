#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../gateway/af-sync"
npx wrangler deploy
