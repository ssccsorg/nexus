#!/usr/bin/env bash
set -euo pipefail
# base11172: build, test, benchmark
# Usage: ./run.sh

cd "$(dirname "$0")"

echo "=== base11172 ==="

echo "--- build ---"
cargo build 2>&1 || { echo "FAILED"; exit 1; }

echo "--- clippy ---"
cargo clippy -- -D warnings 2>&1 || { echo "FAILED"; exit 1; }

echo "--- tests ---"
cargo test 2>&1 || { echo "FAILED"; exit 1; }

echo "--- bench ---"
cargo run -- bench 2>&1 || { echo "FAILED"; exit 1; }

echo "base11172: passed"
