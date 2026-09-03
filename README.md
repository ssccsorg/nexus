# neXus

neXus is the FIH (Fact / Intent / Hint) blackboard storage and coordination runtime of the SSCCS stack. It is a Rust workspace (edition 2024) that provides the storage core, the process layer, and the daemon surface for building blackboard-backed agents and products. The same storage core compiles for hosts, WASM targets, and no_std MCU-class targets.

Project status: pre-1.0 (version 0.1.0). The crates are not published to crates.io; consume them by git revision, as the Rem project does. License: Apache-2.0.

The project documentation site carries the design material and the development guide: <https://docs.ssccs.org/projects/nexus/development>

## Overview

All interaction with a neXus blackboard goes through three record types:

- Fact: an immutable, validated observation.
- Intent: a stateful exploration with the lifecycle `submit` to `claim` to `heartbeat` to `conclude`.
- Hint: a read-only governance signal.

Records are content-addressed. Fact ids are `CoordId` values derived from content, origin, and creator in one SHA-256 pass. Raw content lives in a blob store keyed by its content hash; record files are compact postcard binaries. The blackboard is a plain storage surface: any backend that can store facts, intents, hints, and blobs can host it. There is no privileged orchestrator; peers coordinate through the shared board.

## Layered stack and related repositories

The FIH stack is split across repositories by layer. Each layer depends strictly on the layers below it; no layer reaches upward.

| Repository | Role |
|---|---|
| [syntagma](https://github.com/ssccsorg/syntagma) | Structural coordinate specification: `tagma-core` (Coord, CoordPath, CoordSpaceN) and `tagma-map`. no_std with alloc. |
| [chton](https://github.com/ssccsorg/chton) | IO and storage materialization: `chton::io` (FileIo, IoFuture, FsIo, CoordMapStoreIo) and `chton::store`. no_std layering included. |
| nexus (this repository) | Semantics, contracts, and runtime: `fih-model`, `nex-core`, `nex-fih`, `nex-io`, the `nex` process crate, `nexd`, `nex-server`, `nex-client`. |
| [nex-ext](https://github.com/ssccsorg/nex-ext) | Adapters that attach host database engines to the network (for example the DuckDB and Cypher-backed cold query path). Consumes the cold query contract that lives in `fih-model`. |
| [rem](https://github.com/ssccsorg/rem) | First product consumer: a USB stick for portable memory. It implements a FAT32-backed FileIo in its own workspace against the published contract. |

The ecosystem rule is that contracts live in the stable core, not in implementations. A consumer implements a published trait (for example `FileIo`) in its own workspace and plugs it in; the core never depends on a specific backend.

## Repository layout

The repository contains the root workspace and the nested `nex/` workspace, which root members depend on by path.

Root workspace members:

| Path | Package | Role |
|---|---|---|
| `fih/` | `fih-model` | Pure FIH model: `Fact`, `Intent`, `Hint`, `CoordId`, `FihHash`, `BoardState`, capability trait definitions (`AsyncFactCapable`, `AsyncFilterCapable`, `AsyncStorageRead` and friends), semantic traits, the `Now` clock trait, and the folded cold query contract (`ColdQuery`, `QueryCapable`). no_std with alloc. |
| `nexd/` | `nexd` | Native daemon. Spawns and supervises `nex-server`, serves agent management, and proxies FIH methods over JSON-RPC on a Unix socket. |
| `nex-server/server/` | `nex-server` | JSON-RPC 2.0 storage server over a Unix socket. The handler holds an `Arc<FihStorage<FsIo>>`. |
| `nex-server/client/` | `nex-client` | Typed JSON-RPC client for the server and daemon sockets. |
| `storage/sim/` | `nexus-storage-sim` | Scenario-driven verification runner with in-memory IO backends. |
| `libs/` | `nexus-gateway-serde-proxy`, `nexus-session-server`, `nexus-async-store` | Shared host libraries. |
| `playbooks/agents/` | `nexus-privileged-agent` | Consumer playbook used by `./playbooks/run.sh`. |
| `apps/nex-calc/` | `nex-calc` | Coordinate accumulation CLI, the current design basis for deterministic path accumulation. |
| `verify/support/` | `nexus-verify-support` | Support library for the target verification packages. |
| `verify/osless/` | `nexus-osless-verify` | Host-runnable verification of the OS-less storage path, including the cross-thread critical section stress. |
| `verify/mcu/` | `nexus-mcu-verify` | no_std riscv32imac firmware that boots under QEMU and runs the storage round trip inside the MCU memory budget. |
| `benches/` | workspace bench target | Multi-axis index benchmarks (`cargo bench -p nex`). |

The `nex/` workspace:

| Path | Package | Role |
|---|---|---|
| `nex/core/` | `nex-core` | Clock implementations (`SystemClock` under std, epoch-based clocks for MCU targets) and storage primitives (blob, meta, object store traits). |
| `nex/io/` | `nex-io` | no_std re-export shim over `chton::io`. |
| `nex/fih/` | `nex-fih` | The storage implementation layer: `FihStorage<I: FileIo>`, record maps, the structural filter index, semantic store registration, and re-exports of `fih-model`. no_std with a `std` default feature. |
| `nex/process/` | `nex` | The process layer: OODA scheduler, detection tasks, eviction, plus the backward-compatible alias surface (`nex::storage::core::FihStorage`, `nex::storage::semantic`, `nex::io`, top-level `FileIo`, `FsIo`). |

Standalone applications with their own workspaces and verifiers live under `apps/` (`nex-api`, `nex-calc-fihcontract`, `nex-spinwasi-ssccsdocs`, `nex-tagma`, `nex-wasmer-ssccsdocs`). Edge sync modules live under `gateway/`. The `docs/` directory holds the devlogs that record architectural decisions.

## Getting Started

Requirements: a stable Rust toolchain. Several checks additionally need the `wasm32-unknown-unknown`, `wasm32-wasip2`, and `riscv32imac-unknown-none-elf` targets, which the CI workflow installs.

Build the root workspace:

```bash
cargo build --workspace
```

Build and test the storage workspace:

```bash
cd nex && cargo test --workspace
```

Run the standard checks (fmt, clippy, tests, and the wasm gate):

```bash
./run.sh --core
```

The top-level runner mirrors the CI pipeline:

```bash
./run.sh                 # core + gateway + apps + playbooks
./run.sh --core          # core checks only
./run.sh --gateway       # gateway layer checks
./run.sh --apps          # standalone app verification
./run.sh --server        # nex-server verification over a Unix socket
./run.sh --bench         # tagma multi-axis index benchmarks
./run.sh --playbooks     # consumer playbooks
```

`scripts/run-core.sh` accepts `--check`, `--clippy`, and `--test` for focused local runs.

The MCU runtime verification runs in the Docker image `ghcr.io/ssccsorg/nexus-verify` (built from `verify/Dockerfile` with QEMU and the RISC-V binutils) and boots `target/riscv32imac-unknown-none-elf/release/nexus-mcu-verify` on `qemu-system-riscv32 -machine virt`.

## Integration Guide

### Add the dependency

Pin the repository by revision. The Rem workspace is the reference for a complete consumer setup:

```toml
[dependencies]
nex = { git = "https://github.com/ssccsorg/nexus", package = "nex", rev = "<commit>" }
nex-fih = { git = "https://github.com/ssccsorg/nexus", package = "nex-fih", rev = "<commit>" }
tokio = { version = "1", features = ["full"] }
```

The `nex` package re-exports the historical deep paths, so `nex::storage::core::FihStorage`, `nex::io::FileIo`, and `nex::io::FsIo` resolve without further wiring. `nex-fih` exposes the model types and the capability traits (`Fact`, `Content`, `CoordId`, `AsyncFactCapable`, `AsyncFilterCapable`, `AsyncStorageRead`).

### Host usage

Open a store over a directory, hydrate it, and submit a fact:

```rust
use nex::io::FsIo;
use nex::storage::core::FihStorage;
use nex_fih::{AsyncFactCapable, Content, Fact};

let io = FsIo::new("/tmp/nexus-store")?;
let store = FihStorage::new(io, "my-app");
store.rebuild_cache().await?;

let fact = Fact::new(
    "user-note".to_string(),
    Content::from("remember this"),
    "my-app".into(),
);
store.submit_fact(&fact).await?;
store.flush_pending().await?;
```

### The FileIo contract

`FihStorage<I>` is generic over `I: FileIo` (resolved through `nex::io`, a shim over `chton::io`). A custom backend implements `FileIo` in the consumer workspace. The canonical semantics that `rebuild_cache` and the write path rely on are fixed: listing a missing prefix is an empty enumeration, listed keys include their prefix so they round-trip through `read`, and deleting a missing key is a no-op. The Rem repository pins these semantics with integration tests in `rem-storage-fat` and is the reference implementation of a FAT32-style backend.

### no_std targets

The storage core builds without an OS. Consume `nex-fih` and `chton` with `default-features = false`, provide `alloc` and a `critical-section` implementation with the `restore-state-bool` feature, and construct storage with an injected clock through the `Now` trait (`FihStorage::with_clock`). On a host the std `SystemClock` is the default; on an MCU use an epoch-based clock. `nexus-mcu-verify` under `verify/mcu` is the complete reference: it builds as a riscv32imac no_std firmware and runs the FIH round trip under QEMU inside the 512 KB budget.

### Daemon deployment

For a supervised deployment, run `nexd`, which spawns `nex-server` as a child process and serves `/tmp/nexd.sock` by default. The typed client replaces hand-rolled JSON-RPC:

```rust
use nex_client::connect; // connect(path) then call the typed methods
```

The FIH methods served by `nexd` and `nex-server` are `write_fact`, `read_state`, `read_fact`, `read_intent`, `read_hint`, `write_intent`, `write_hint`, `claim_intent`, `heartbeat_intent`, `release_intent`, `conclude_intent`. `nexd` additionally serves `spawn_agent`, `list_agents`, and `kill_agent`. The wire protocol shape is documented in `docs/wire-protocol.md`.

## Verification

GitHub Actions runs on push to `main` and on pull requests:

- Core: fmt, clippy, tests, the wasm32 gate, and the MCU runtime verification under QEMU in Docker.
- Server: `nex-server` and `nex-client` verification over a Unix socket.
- Gateway: the HTTP API and edge sync modules.
- Apps: standalone app verifiers (spin-wasi reference, `nex-calc`, `nexd` lifecycle, tagma consumer).
- Playbooks: consumer playbook scenarios.

The same flows run locally through `./run.sh` sub-commands, so a change is validated locally before it reaches CI.

## Contributing

The repository follows an issue-first flow:

1. Open a GitHub issue that describes the change and add the relevant labels.
2. Create a branch named `{issue-number}-{subject-alphabets-with-one-or-two-dashes}`.
3. Open a pull request titled `PR: {category}: {message}`, with `#{issue}` after the category on PR branches and omitted on `main`.

Commit messages use the same shape: `{category}: {message}` with `#{issue}` on PR branches. Categories include `feat`, `fix`, `refactor`, `docs`, `chore`, `ci`, `test`, and `build`.

Code guidance:

- Keep changes scoped to the goal and do not generate speculative code.
- Put unit and integration tests under `tests/` directories rather than inline in host code files.
- Run `cargo fmt`, `cargo clippy`, and the relevant `./run.sh` target before opening a pull request.
- The infrastructure contract rule applies to every change: the core defines capability traits and stable contracts, and implementations (including consumer-side backends) implement those contracts without modifying the core surface. A change that alters a core contract requires the capability gap to be demonstrated first.

## Documentation

- Development guide: <https://docs.ssccs.org/projects/nexus/development>
- Project documentation: <https://docs.ssccs.org/projects/nexus/>
- Design and decision records: the `docs/` directory in this repository, including the layered restructure (`2026-08-20-176-content-hash-conflict-l2-restructure.md`), the cold query and DuckDB direction (`2026-08-29-181-cypher-removal-and-nex-duckdb-direction.md`), the multi-dimensional structural search benchmark (`2026-08-27-179-multidim-structural-search-bench.md`), and the wire protocol (`wire-protocol.md`).

---

## License

Apache-2.0. See the `LICENSE` file in this repository.
