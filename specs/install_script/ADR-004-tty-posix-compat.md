# ADR-004 — POSIX sh and curl-pipe TTY Compatibility

**Status:** Accepted  
**Date:** 2026-04-09  
**Version:** EdgeQuake v0.9.12

---

## Context

EdgeQuake's primary install experience is:

```sh
curl -fsSL https://raw.githubusercontent.com/raphaelmansuy/edgequake/edgequake-main/quickstart.sh | sh
```

When a shell reads its script from a pipe, **stdin (fd 0) becomes the pipe**. Any
`read` command that reads from stdin (the default) immediately hits EOF or reads garbage
from the pipe buffer, returning an empty string. This breaks interactive menus.

Two standard patterns exist:
1. **Detect pipe and error out** — simple but hostile to the primary use case
2. **Read from `/dev/tty` explicitly** — correct solution

## Decision

**All interactive reads use `/dev/tty` directly**, never stdin.

### Implementation

```sh
# Read one line from /dev/tty. Result in $_TTY_INPUT.
# Fails gracefully (returns empty string) if /dev/tty is unavailable.
_tty_read() {
  if [ -e /dev/tty ]; then
    read -r _TTY_INPUT < /dev/tty
  else
    _TTY_INPUT=""
  fi
}

# Read secret (no echo). Result in $_TTY_INPUT.
_tty_read_secret() {
  if [ -e /dev/tty ]; then
    stty -echo < /dev/tty 2>/dev/null || true
    read -r _TTY_INPUT < /dev/tty
    stty echo < /dev/tty 2>/dev/null || true
    printf '\n'
  else
    _TTY_INPUT=""
  fi
}
```

### When `/dev/tty` is Unavailable

`/dev/tty` is unavailable in:
- GitHub Actions (and most CI environments without a PTY allocator)
- Docker build contexts
- `ssh host sh -c 'curl | sh'` (no PTY)

Detection: `[ ! -e /dev/tty ]`

Response: Print a clear error describing the situation, show the env-var based
alternative, and exit 1.

```
  ✗  No interactive terminal available (/dev/tty is not accessible).
  →  The EdgeQuake setup wizard requires an interactive terminal.
  →  For automated/CI installs, run directly with env vars:
       EDGEQUAKE_LLM_PROVIDER=openai \
       OPENAI_API_KEY=sk-... sh quickstart.sh --non-interactive
```

> Note: `--non-interactive` flag is a future enhancement (not in v0.9.12). For now,
> power users set env vars and the wizard will still run (the menus will work if called
> from an interactive session, or fail gracefully if not).

## Terminal Capability Guards

### Colours

Only emit ANSI escape codes when **stdout is a TTY**:

```sh
if [ -t 1 ]; then
  C_BOLD="\033[1m"; C_GREEN="\033[32m"; ...
else
  C_BOLD=""; C_GREEN=""; ...
fi
```

This prevents garbage in logs, CI output, or when stdout is captured to a file.

### Unicode Box-Drawing

The banner uses Unicode box-drawing characters (`╔`, `═`, `╗`, etc.) directly in the
source file (UTF-8 encoded). These will render correctly on any modern terminal with
UTF-8 locale support (macOS, Ubuntu, Fedora all default to UTF-8 since ~2010).

If the terminal cannot render them, they appear as `?` characters — harmless degradation.
An ASCII-only fallback is not implemented (adds code complexity for a rare edge case).

## POSIX Compatibility Constraints

The shebang is `#!/usr/bin/env sh`. This must work with:
- **bash** (default on macOS)
- **dash** (default `sh` on Ubuntu/Debian)
- **ash/busybox sh** (Alpine Linux, minimal containers)

Banned constructs:
- `local` keyword (not POSIX; use `_`-prefixed globals)
- `[[ ]]` (bash extension; use `[ ]`)
- Arrays `arr=()` (not POSIX; use positional params or newline-delimited strings)
- `echo -e` (not guaranteed; always use `printf`)
- Process substitution `<(cmd)` (not POSIX)
- `read -p` (not POSIX; use `printf "prompt: "` then `read -r`)

Allowed POSIX constructs used:
- `$(())` arithmetic expansion
- `$()` command substitution
- `case / esac`
- `read -r _var < /dev/tty`
- Functions (POSIX)
- `printf "%s" ...`
- `[ -e /dev/tty ]`

## Consequences

- Wizard works correctly for `curl | sh` (primary install path) ✓
- Wizard works for direct `sh quickstart.sh` ✓
- Wizard fails cleanly in true non-interactive environments (CI without PTY) ✓
- Secret input (API key) is masked via `stty -echo` ✓
- Script is portable across bash, dash, zsh in sh-mode ✓
