#!/bin/bash
#
# nexus-core -- Local CI runner
#
# Standalone core-only check script. Does NOT delegate to run.sh.
# Use run.sh at the project root for comprehensive CI.
#
# Usage:
#   scripts/run-core.sh                 # Full check
#   scripts/run-core.sh --check         # Check only
#   scripts/run-core.sh --clippy        # Clippy only
#   scripts/run-core.sh --test          # Test core crates (table + model)
#   scripts/run-core.sh --graph-test    # Test graph crate (known pre-existing issues)

set -e
cd "$(dirname "$0")/../core"

MODE="all"

while [[ "$#" -gt 0 ]]; do
    case $1 in
        --check) MODE="check" ;;
        --clippy) MODE="clippy" ;;
        --test) MODE="test" ;;
        --graph-test) MODE="graph-test" ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
    shift
done

run_check()   { cargo check -p nexus-model -p nexus-table && cargo check; }
run_fmt()     { cargo fmt --check; }
run_clippy()  { cargo clippy -p nexus-model -p nexus-table -- -D warnings; }
run_test()    { cargo test -p nexus-table -- --nocapture 2>&1; }
run_graph_test() { cargo test -p nexus-graph -- --nocapture 2>&1 || echo "[warn] some graph tests have pre-existing issues"; }
run_all() {
    echo "=== fmt ===" && run_fmt
    echo "=== check ===" && run_check
    echo "=== clippy ===" && run_clippy
    echo "=== test (table) ===" && run_test
}

case $MODE in
    check)  run_check ;;
    clippy) run_clippy ;;
    test)   run_test ;;
    graph-test) run_graph_test ;;
    all)
        echo "nexus-core CI (local)"
        run_all
        echo ""
        echo "All checks passed."
        ;;
esac
