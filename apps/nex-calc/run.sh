#!/usr/bin/env bash
set -euo pipefail
#
# nex-calc — FIH-based calculator convenience runner
#
# Usage:
#   ./run.sh              Interactive mode
#   ./run.sh --test       Run tests
#   ./run.sh <expression> Evaluate one expression, e.g. `./run.sh "3 + 5"`
#
# Examples:
#   ./run.sh
#   ./run.sh --test
#   echo -e "put 3\nput 5\nadd f1 f2\nresolve i1\nlist" | ./run.sh
#

cd "$(dirname "$0")"

case "${1:-}" in
    --test|-t)
        shift
        exec cargo test -p nex-calc "$@"
        ;;
    --help|-h)
        sed -n '3,15p' "$0"
        exit 0
        ;;
    *)
        exec cargo run -p nex-calc "$@"
        ;;
esac
