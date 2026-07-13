#!/usr/bin/env bash
set -euo pipefail
#
# tagma — Single entry point
#
# Usage:
#   ./run.sh                 # Full pipeline: check → build → test
#   ./run.sh --check         # fmt → clippy → build → test (strict)
#   ./run.sh --fix           # auto-fix → build → test
#   ./run.sh --doc           # build documentation
#   ./run.sh --help
#

cd "$(dirname "$0")"
export RUSTFLAGS="-D warnings"

# ── Helpers ───────────────────────────────────────────────────────────

check_checks() {
    (cd sw/rust && cargo fmt --check)
    (cd sw/rust && cargo clippy --all-targets)
    (cd sw/rust && cargo build --release)
    (cd sw/rust && cargo test --release)
}

build_and_test() {
    (cd sw/rust && cargo build --release)
    (cd sw/rust && cargo test --release)
}

auto_fix() {
    (cd sw/rust && cargo fmt --all)
    (cd sw/rust && cargo clippy --fix --allow-dirty 2>&1 || true)
    (cd sw/rust && cargo fix --allow-dirty 2>&1 || true)
    (cd sw/rust && cargo fmt --all)
}

build_docs() {
    (cd sw/rust && cargo doc --no-deps)
}

# ── Dispatch ──────────────────────────────────────────────────────────

case "${1:-}" in
    --check|check)
        check_checks
        ;;
    --fix|fix)
        auto_fix
        build_and_test
        ;;
    --doc|doc)
        build_docs
        ;;
    --help|-h)
        echo "Usage: ./run.sh [--check|--fix|--doc|--help]"
        exit 0
        ;;
    *)
        auto_fix
        check_checks
        ;;
esac
