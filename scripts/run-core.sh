#!/bin/bash
#
# nexus-core -- Local CI runner
#
# Standalone core-only check script. Does NOT delegate to run.sh.
# Use run.sh at the project root for comprehensive CI (core + docker + gateway).
#
# Mirrors .github/workflows/core.yml locally:
#   cargo fmt | clippy --fix | check | clippy | test
#
# Usage:
#   scripts/run-core.sh               # Full check
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
# Auto-fix trivial clippy warnings (unused imports, redundant clones, etc.)
# before running the strict check. --allow-dirty lets it modify working tree.
run_clippy_fix() { cargo clippy --fix --allow-dirty --allow-staged 2>&1 || true; }
# Strict clippy: deny all warnings. Must pass for CI to succeed.
run_clippy() { cargo clippy -- -D warnings; }
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
    echo "=== clippy --fix ===" && run_clippy_fix
    echo "=== fmt (after fix) ===" && run_fmt
    echo "=== check ===" && run_check
    echo "=== clippy ===" && run_clippy
    echo "=== test ===" && run_test
}

case $MODE in
    check)  echo "=== fmt ===" && run_fmt && run_check ;;
    clippy) echo "=== fmt ===" && run_fmt && run_clippy_fix && run_fmt && run_clippy ;;
    test)   echo "=== fmt ===" && run_fmt && run_test ;;
    all)
        echo "nexus-core CI (local)"
        run_all
        echo ""
        echo "All checks passed."
        ;;
esac
