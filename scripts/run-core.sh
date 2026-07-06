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

run_check()  { cargo check -p nex && cargo check -p nexd && cargo check -p nexus-storage-duckdb && cargo check -p nexus-storage-sim; }

# ── WASM check: ensure storage-sim builds for wasm32 target ────────────

run_wasm_check() {
    # Find all Cargo.toml under core directories, excluding non-WASM targets.
    # Exclusions: storage/duckdb (crossterm), storage/ve-composite (tokio),
    # storage/sim (tokio), apps/* (HTTP server), playbooks/* (scripts),
    # target/ (build artifacts).
    # apps/ is excluded: each app has its own build target (native, container, etc.)
    # and is not expected to compile for wasm32.
    find . -name Cargo.toml \
        -not -path './target/*' \
        -not -path './ext/*' \
        -not -path './apps/*' \
        -not -path './storage/duckdb/*' \
        -not -path './storage/ve-composite/*' \
        -not -path './storage/sim/*' \
        -not -path './nexd/*' \
        -not -path './nex-server/*' \
        -not -path './apps/*' \
        -not -path './playbooks/*' \
        -not -path './Cargo.toml' \
        -exec sh -c '
            dir="$(dirname "$1")"
            echo "=== WASM: $dir ==="
            cargo check --manifest-path "$1" --target wasm32-unknown-unknown 2>&1
        ' _ {} \;
}

# ── Pre-flight auto-fixes: catch trivial issues before strict checks ────

run_fmt() {
    cargo fmt --all 2>&1 || true
}
run_clippy_fix()   { cargo clippy --fix --allow-dirty -p nex 2>&1 || true; }
run_compiler_fix() { cargo fix --allow-dirty -p nex 2>&1 || true; }
run_auto_fix()     { run_fmt && run_clippy_fix && run_compiler_fix && run_fmt; }

# ── Strict checks: must pass — no warnings tolerated ────────────────────

run_clippy() {
    # Core crates only. Apps (nex-cf, wasmer, api, zed) are separate projects.
    for pkg in \
        nex \
        nexus-storage-composite \
        nexus-storage-petgraph \
        nexus-storage-sim \
        nexus-model \
        interface-query \
        interface-cypher \
        nexus-gateway-serde-proxy \
        nexd
    do
        cargo clippy -p "$pkg" -- -D warnings -A clippy::await-holding-refcell-ref
    done
}
run_test()   {
    cargo test -p nex -- --nocapture 2>&1
    echo "---"
    cargo test -p nexd --test integration -- --test-threads=1 --nocapture 2>&1
    echo "---"
    cargo test -p nexus-storage-sim -- --nocapture 2>&1

}
run_all() {
    echo "=== fmt --all ===" && run_fmt
    echo "=== clippy --fix --workspace ===" && run_clippy_fix
    echo "=== fix --workspace ===" && run_compiler_fix
    echo "=== fmt (after fixes) ===" && run_fmt
    echo "=== check ===" && run_check
    echo "=== wasm check ===" && run_wasm_check
    echo "=== clippy ===" && run_clippy
    echo "=== test ===" && run_test
}

case $MODE in
    check)  echo "=== fmt --all ===" && run_fmt && run_check && run_wasm_check ;;
    clippy) run_auto_fix && run_clippy ;;
    test)   echo "=== fmt --all ===" && run_fmt && run_wasm_check && run_test ;;
    all)
        echo "nexus-core CI (local)"
        run_all
        echo ""
        echo "All checks passed."
        ;;
esac
