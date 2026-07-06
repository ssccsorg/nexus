#!/usr/bin/env bash
set -euo pipefail
#
# nex-calc-fihcontract — FIH-based calculator with FihContract layer
#
# Usage:
#   ./run.sh              Interactive mode
#   ./run.sh --test       Run tests
#
# This is a standalone project (not in workspace). Uses local Cargo.toml.
#

cd "$(dirname "$0")"

case "${1:-}" in
    --test|-t)
        shift
        exec cargo test "$@"
        ;;
    --help|-h)
        sed -n '3,11p' "$0"
        exit 0
        ;;
    *)
        exec cargo run "$@"
        ;;
esac
