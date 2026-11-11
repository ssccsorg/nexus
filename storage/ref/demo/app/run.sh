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

usage() {
    cat <<'TEXT'
usage: storage/ref/demo/app/run.sh [--fresh]

  --fresh   reinstall from the lock file before the run, which heals a stale frame
TEXT
}

# A failure ends with the next thing to run, because the step above it is the reason and not the answer.
fail() {
    printf '\n%s\n' "$1" >&2
    printf '  %s\n' "$2" >&2
    exit 1
}

fresh=0
for argument in "$@"; do
    case "$argument" in
        --fresh) fresh=1 ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            usage >&2
            fail "unknown option: $argument" "This takes --fresh and nothing else."
            ;;
    esac
done

if ! command -v node >/dev/null 2>&1; then
    fail "node is not on the path, and the board is built and served by Node." \
        "Install Node 20 or newer from https://nodejs.org, then run this again."
fi

if ! command -v npm >/dev/null 2>&1; then
    fail "npm is not on the path, and the demonstration is a Vite deployment." \
        "npm ships with Node, so install Node 20 or newer from https://nodejs.org."
fi

if [ "$fresh" -eq 1 ]; then
    if ! npm ci; then
        fail "the install failed, so nothing was rebuilt." \
            "Delete node_modules and run this again; the frame is read from ../skene."
    fi
fi

# The rest is the same work `npm run demo` does: the frame, the tool, the board, and the server.
if ! node tools/run-demo.mjs; then
    fail "the demonstration stopped at the step printed above." \
        "That step names what to install or fix. Nothing before it needs repeating."
fi
