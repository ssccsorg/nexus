#!/usr/bin/env bash
set -euo pipefail
#
# nex-derive: directed FIH goal space convenience runner
#
# Usage:
#   ./run.sh              Interactive mode
#   ./run.sh --test       Run tests
#   ./run.sh --demo       Run the scripted demo and exit
#
# Examples:
#   ./run.sh
#   ./run.sh --test
#   printf 'demo\n' | ./run.sh
#

cd "$(dirname "$0")"

case "${1:-}" in
    --test|-t)
        shift
        exec cargo test -p nex-derive "$@"
        ;;
    --demo)
        shift
        exec sh -c 'printf "demo\nquit\n" | cargo run -q -p nex-derive'
        ;;
    --help|-h)
        sed -n '3,14p' "$0"
        exit 0
        ;;
    *)
        exec cargo run -p nex-derive "$@"
        ;;
esac
