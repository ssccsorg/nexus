#!/bin/bash
#
# nexus-core — Local CI runner
#
# Mirrors .github/workflows/core.yml locally:
#   cargo check | clippy | test
#
# Usage:
#   ./run.sh              # Full check: fmt + clippy + test
#   ./run.sh --check      # Check only
#   ./run.sh --clippy     # Clippy only
#   ./run.sh --test       # Test only
#   ./run.sh --act        # Run via act (Docker, same as CI)
#

set -e
cd "$(dirname "$0")"

MODE="all"

while [[ "$#" -gt 0 ]]; do
    case $1 in
        --check) MODE="check" ;;
        --clippy) MODE="clippy" ;;
        --test) MODE="test" ;;
        --act) MODE="act"; shift; continue ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
    shift
done

run_check()  { cargo check -p nexus-cypher && cargo check; }
run_fmt()    { cargo fmt --check; }
run_clippy() { cargo clippy -- -D warnings; }
run_test()   { cargo test -p nexus-cypher && cargo test; }
run_all() {
    echo "=== fmt ===" && run_fmt
    echo "=== check ===" && run_check
    echo "=== clippy ===" && run_clippy
    echo "=== test ===" && run_test
}

case $MODE in
    check)  run_check ;;
    clippy) run_clippy ;;
    test)   run_test ;;
    act)
        echo "Running CI via act..."
        act -j check -W ../.github/workflows/core.yml --pull=false
        ;;
    all)
        echo "nexus-core CI (local)"
        run_all
        echo ""
        echo "All checks passed."
        ;;
esac
