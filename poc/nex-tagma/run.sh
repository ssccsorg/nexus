#!/usr/bin/env bash
set -euo pipefail
# nex-tagma: build, lint, test, and benchmark
# Usage: ./run.sh

cd "$(dirname "$0")"

echo "=== nex-tagma ==="

echo "--- build ---"
cargo build 2>&1 || { echo "FAILED"; exit 1; }

echo "--- clippy ---"
cargo clippy -- -D warnings 2>&1 || { echo "FAILED"; exit 1; }

echo "--- format ---"
cargo fmt --check 2>&1 || { echo "FAILED"; exit 1; }

echo "--- tests ---"
cargo test --tests 2>&1 || { echo "FAILED"; exit 1; }

echo "--- bench ---"
cargo run -- bench 2>&1 || { echo "FAILED"; exit 1; }

echo "nex-tagma: passed"
