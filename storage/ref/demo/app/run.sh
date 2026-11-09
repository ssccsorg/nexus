#!/usr/bin/env bash
set -euo pipefail
#
# The demonstration board, served from this checkout.
#
#   storage/ref/demo/app/run.sh           # build what is missing, import, serve
#   storage/ref/demo/app/run.sh --fresh   # reinstall from the lock file first
#
# The frame is a live sibling checkout, and npm copies a `file:` dependency into
# node_modules, so an install that compares versions leaves the frame stale while
# the screen shows the old one. `--fresh` is the way out of that.
#
# The gate is `./run.sh --demo` at the repository root: it runs the tests and the
# bundle rather than serving.

cd "$(dirname "$0")"

fresh=0
for argument in "$@"; do
    case "$argument" in
        --fresh) fresh=1 ;;
        *)
            echo "unknown option: $argument" >&2
            exit 1
            ;;
    esac
done

if ! command -v npm >/dev/null 2>&1; then
    echo "npm is not on the path, and the demonstration is a Vite deployment" >&2
    exit 1
fi

if [ "$fresh" -eq 1 ]; then
    npm ci
fi

# The rest is the same work `npm run demo` does: the frame, the tool, the board, and the server.
exec node tools/run-demo.mjs
