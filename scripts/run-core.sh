#!/bin/bash
#
# nexus-core — Local CI runner
#
# Mirrors .github/workflows/core.yml locally.
# Pre-flight auto-fixes catch formatting, trivial clippy, and compiler
# suggestions before strict checks — eliminating most CI noise.
#
# Pipeline:
#   fmt --all → clippy --fix --workspace → fix --workspace → fmt → check → clippy → test
#
# Usage:
#   scripts/run-core.sh               # Full check
#   scripts/run-core.sh --check       # Check only
#   scripts/run-core.sh --clippy      # Clippy only
#   scripts/run-core.sh --test        # Test only
#

set -e
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

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

run_check()  { cargo check -p nex && cargo check -p nexus-storage-duckdb; }

# ── Pre-flight auto-fixes: catch trivial issues before strict checks ────

run_fmt()          { cargo fmt --all; }
run_clippy_fix()   { cargo clippy --fix --allow-dirty --workspace 2>&1 || true; }
run_compiler_fix() { cargo fix --allow-dirty --workspace 2>&1 || true; }
run_auto_fix()     { run_fmt && run_clippy_fix && run_compiler_fix && run_fmt; }

# ── Strict checks: must pass — no warnings tolerated ────────────────────

run_clippy() { cargo clippy --workspace -- -D warnings; }
run_test()   {
    cargo test -p nex -- --nocapture 2>&1
    echo "---"
    cargo test -p nexus-storage-duckdb -- --nocapture 2>&1
    echo "---"
    cargo test -p nexus-storage-sim -- --nocapture 2>&1
}
run_all() {
    echo "=== fmt --all ===" && run_fmt
    echo "=== clippy --fix --workspace ===" && run_clippy_fix
    echo "=== fix --workspace ===" && run_compiler_fix
    echo "=== fmt (after fixes) ===" && run_fmt
    echo "=== check ===" && run_check
    echo "=== clippy ===" && run_clippy
    echo "=== test ===" && run_test
}

case $MODE in
    check)  echo "=== fmt --all ===" && run_fmt && run_check ;;
    clippy) run_auto_fix && run_clippy ;;
    test)   echo "=== fmt --all ===" && run_fmt && run_test ;;
    all)
        echo "nexus-core CI (local)"
        run_all
        echo ""
        echo "All checks passed."
        ;;
esac
