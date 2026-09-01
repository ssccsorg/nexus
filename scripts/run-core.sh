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

run_check()  { cargo check -p nex && cargo check -p nexd && cargo check -p nexus-storage-sim; }

# ── WASM check: ensure storage-sim builds for wasm32 target ────────────

run_wasm_check() {
    # Find all Cargo.toml under core directories, excluding non-WASM targets.
    # Exclusions: storage/ve-composite (tokio),
    # storage/sim (tokio), apps/* (HTTP server), playbooks/* (scripts),
    # target/ (build artifacts).
    # apps/ is excluded: each app has its own build target (native, container, etc.)
    # and is not expected to compile for wasm32.
    find . -name Cargo.toml \
        -not -path './target/*' \
        -not -path './ext/*' \
        -not -path './apps/*' \
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

# ── WASIp2 smoke check (issue #181): the MCU-relevant WASI target ──────
#
# The storage core must compile for wasm32-wasip2 so the future launcher
# (Wasmi/WAMR + WASI-to-FAT32 bridge) can run FihStorage on-device. This
# is a smoke target, not a full gate: it checks the storage-path crates
# only (fih-model, nex-core, nex-fih, nex-io), which are the ones the
# launcher links. serde_json and futures-executor remain available on
# wasip2 (std is supported), so this currently passes; the check fixes the
# property. fih-model lives in the root workspace; the nex-* crates live
# in the `nex` sub-workspace and must be checked from there so the root
# workspace's std-feature serde does not unify into their feature sets and
# so the same chton revision (pinned in the sub-workspace lockfile) is
# validated.

run_wasip2_check() {
    echo "=== WASIp2: fih-model ==="
    cargo check -p fih-model --target wasm32-wasip2 2>&1
    for pkg in nex-core nex-fih nex-io; do
        echo "=== WASIp2: $pkg ==="
        (cd nex && cargo check -p "$pkg" --target wasm32-wasip2) 2>&1
    done
}

# ── no_std anchors (issue #181): the OS-less storage path must stay std-free ──
#
# The storage core is layered: fih-model (pure types), nex-core (clock
# contracts), nex-fih (semantics). Each layer must compile and pass its
# anchor tests with `--no-default-features`; the integration test files
# (tests/no_std_anchors.rs) reference no std APIs, so a std type leaking
# past its feature gate breaks the build. The real no_std target check is
# wasm32-unknown-unknown (wasip2 supports std, so it is only a smoke
# target). The host-only std surface (FsIo, FileOrigin, SystemClock) is
# exercised by the regular std test suite.

run_nostd_check() {
    # fih-model lives in the root workspace; the nex-* crates live in the
    # `nex` sub-workspace. The no_std checks must run from the workspace
    # that owns each crate: the root workspace's serde (default std)
    # would otherwise unify into the nex crates' feature set and break
    # the std-less MCU target.
    echo "=== no_std check: fih-model (no-default-features) ==="
    cargo check -p fih-model --no-default-features 2>&1
    for pkg in nex-core nex-fih nex-io; do
        echo "=== no_std check: $pkg (no-default-features) ==="
        (cd nex && cargo check -p "$pkg" --no-default-features) 2>&1
    done
    echo "=== no_std anchor tests ==="
    (cd nex && cargo test -p nex-core --no-default-features --test no_std_anchors) 2>&1
    (cd nex && cargo test -p nex-fih --no-default-features --test no_std_anchors) 2>&1
    echo "=== true no_std target: wasm32-unknown-unknown ==="
    (cd nex && cargo check -p nex-core --no-default-features --target wasm32-unknown-unknown) 2>&1
    (cd nex && cargo check -p nex-fih --no-default-features --target wasm32-unknown-unknown) 2>&1
    echo "=== MCU target: riscv32imac-unknown-none-elf ==="
    (cd nex && cargo check -p nex-core --no-default-features --target riscv32imac-unknown-none-elf) 2>&1
    (cd nex && cargo check -p nex-fih --no-default-features --target riscv32imac-unknown-none-elf) 2>&1
    (cd nex && cargo check -p nex-io --no-default-features --target riscv32imac-unknown-none-elf) 2>&1
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
        nex-core \
        nex-fih \
        nex-io \
        nexus-storage-sim \
        interface-query \
        nexus-gateway-serde-proxy \
        nexd
    do
        cargo clippy -p "$pkg" -- -D warnings -A clippy::await-holding-refcell-ref
    done
}
run_test()   {
    cargo test -p nex -- --nocapture 2>&1
    echo "---"
    # build nex-server before integration tests; cargo test -p nexd does not
    # pull in the nex-server binary as a dependency
    cargo build -p nex-server 2>&1
    # all nexd test targets (lib + integration + proc-daemon)
    cargo test -p nexd -- --test-threads=1 --nocapture 2>&1
    echo "---"
    cargo test -p nexus-storage-sim -- --nocapture 2>&1
    echo "---"
    # smoke verification runner: exercises every storage capability end to end
    cargo run -p nexus-storage-sim 2>&1
    echo "---"
}
run_all() {
    echo "=== fmt --all ===" && run_fmt
    echo "=== clippy --fix --workspace ===" && run_clippy_fix
    echo "=== fix --workspace ===" && run_compiler_fix
    echo "=== fmt (after fixes) ===" && run_fmt
    echo "=== check ===" && run_check
    echo "=== wasm check ===" && run_wasm_check
    echo "=== wasip2 smoke ===" && run_wasip2_check
    echo "=== no_std anchors ===" && run_nostd_check
    echo "=== clippy ===" && run_clippy
    echo "=== test ===" && run_test
}

case $MODE in
    check)  echo "=== fmt --all ===" && run_fmt && run_check && run_wasm_check && run_wasip2_check && run_nostd_check ;;
    clippy) run_auto_fix && run_clippy ;;
    test)   echo "=== fmt --all ===" && run_fmt && run_wasm_check && run_test ;;
    all)
        echo "nexus-core CI (local)"
        run_all
        echo ""
        echo "All checks passed."
        ;;
esac
