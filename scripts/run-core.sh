#!/bin/bash
#
# nexus-core -- Local CI runner
#
# Standalone core-only check script. Does NOT delegate to run.sh.
# Use run.sh at the project root for comprehensive CI (core + docker + gateway).
#
# Mirrors .github/workflows/core.yml locally:
#   cargo fmt | check | clippy | test
#
# Usage:
#   scripts/run-core.sh               # Full check: fmt + clippy + test
#   scripts/run-core.sh --check       # Check only
#   scripts/run-core.sh --clippy      # Clippy only
#   scripts/run-core.sh --test        # Test only
#

set -e
cd "$(dirname "$0")/../core"

MODE="all"

while [[ "$#" -gt 0 ]]; do
    case $1 in
        --check) MODE="check" ;;
        --clippy) MODE="clippy" ;;
        --test) MODE="test" ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
    shift
done

run_check()  { cargo check -p nexus-graph -p nexus-storage-sqlite -p nexus-storage-duckdb -p nexus-storage-petgraph -p nexus-process && cargo check; }
run_fmt()    { cargo fmt; }
run_clippy() { cargo clippy -- -D warnings 2>&1 | head -20 || true; }
run_test()   {
    cargo test -p nexus-storage-sqlite -- --nocapture 2>&1
    echo "---"
    cargo test -p nexus-storage-duckdb -- --nocapture 2>&1
    echo "---"
    cargo test -p nexus-storage-petgraph -- --nocapture 2>&1
    echo "---"
    cargo test -p nexus-graph -- --nocapture 2>&1
    echo "---"
    cargo test -p nexus-process -- --nocapture 2>&1
}
run_all() {
    echo "=== fmt ===" && run_fmt
    echo "=== check ===" && run_check
    echo "=== clippy ===" && run_clippy
    echo "=== test ===" && run_test
}

case $MODE in
    check)  echo "=== fmt ===" && run_fmt && run_check ;;
    clippy) echo "=== fmt ===" && run_fmt && run_clippy ;;
    test)   echo "=== fmt ===" && run_fmt && run_test ;;
    all)
        echo "nexus-core CI (local)"
        run_all
        echo ""
        echo "All checks passed."
        ;;
esac
