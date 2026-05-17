#!/usr/bin/env bash
set -euo pipefail
#
# Run by .github/workflows/sync-docs.yml.
# Syncs all artifacts from R2 and runs fetch_ssccs.py transforms.
#
# Usage:
#   scripts/sync-docs.sh [sync_root]
#
# Environment:
#   AWS_ACCESS_KEY_ID       — R2 access key
#   AWS_SECRET_ACCESS_KEY   — R2 secret key
#   CFR2_USR_API_HOST       — R2 endpoint URL

cd "$(dirname "$0")/.."
SYNC_ROOT="${1:-/tmp/ssccs}"

echo "=== Syncing artifacts from R2 ==="
aws s3 sync "s3://ssccs-nexus-af/ssccs" "$SYNC_ROOT" \
    --endpoint-url "${CFR2_USR_API_HOST:-https://fda2afbec0a19b01c76d98e81f45be41.r2.cloudflarestorage.com}" \
    --delete

echo "=== Running transforms ==="
python3 scripts/fetch_ssccs.py --sync-root "$SYNC_ROOT" --all

echo "=== Committing changes ==="
if ! git diff --quiet HEAD; then
    git config user.name "ssccs-bot"
    git config user.email "git-bot@ssccs.org"
    git add -A
    git commit -m "sync: update from ssccs ($(date -u +%Y-%m-%d))"
    git push
else
    echo "Nothing to commit."
fi

echo "Sync complete."
