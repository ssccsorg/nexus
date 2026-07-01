# Daemon Runtime Module

Built-in daemon runtime providing graceful shutdown, signal handling, and 
concurrent task management. Adapted from [proc-daemon](https://github.com/jamesgober/proc-daemon) 
v1.1.2 (Apache 2.0).

## Modifications from upstream

- Stripped: async-std support, Windows signal handling, config-watch, 
  memory pools, metrics, profiling, lock-free coordination, resource tracking
- Rewrote: daemon core (1,288→95 lines), shutdown coordinator, error types
- Kept: Unix signal handling via tokio, shutdown lifecycle, timeout validation

## Upstream update procedure

1. Compare new proc-daemon release against our `shutdown.rs` and `signal.rs`
2. Port relevant bugfixes manually
3. Do NOT `git subtree pull` — our code has diverged completely

## Files

| File | Source | Lines |
|------|--------|-------|
| `daemon_core.rs` | Adapted from `daemon.rs` | 95 |
| `daemon_config.rs` | Adapted from `config.rs` | 87 |
| `shutdown.rs` | Adapted from `shutdown.rs` | 122 |
| `signal.rs` | Adapted from `signal.rs` | 37 |
| `error.rs` | Adapted from `error.rs` | 78 |
| Others | Stubs — available from proc-daemon source | 1 each |
