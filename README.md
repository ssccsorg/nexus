# neXus

Peer-to-peer state fabric for agents and storage, from microcontrollers to data centers. One coordinate space, one protocol, multiple directions, no orchestration.

neXus implements the FIH (Fact, Intent, Hint) model of the SSCCS stack, in a Rust workspace (edition 2024) that carries the storage core, the process layer, and the daemon surface. The same storage core compiles for hosts, WASM targets, and no_std MCU-class targets.

Status: pre-1.0 (0.1.0). The crates are not published; pin them by git revision. Apache-2.0.

## The model

Every interaction with a neXus board is one of three records.

| Record | What it is |
|---|---|
| Fact | an immutable, validated observation |
| Intent | a stateful exploration with a lifecycle, from `submit` through `claim` and `heartbeat` to `conclude` |
| Hint | a read-only governance signal |

A transition composes them into a new fact, `F x I x H -> F'`. Storage is one surface of that structure: any backend that can hold facts, intents, hints, and blobs can host a board, and there is no privileged orchestrator, because peers coordinate through the board itself.

A fact's id is a `CoordId` derived from its content, its origin, and its creator in one SHA-256 pass. Raw content lives in a blob store keyed by its content hash, and record files are compact postcard binaries.

## The stack

The FIH stack is split across repositories by layer. Each layer depends on the layers below it, and no layer reaches upward.

| Repository | Role |
|---|---|
| [syntagma](https://github.com/ssccsorg/syntagma) | structural coordinate specification, no_std with alloc |
| [chton](https://github.com/ssccsorg/chton) | IO and storage materialization, no_std layering included |
| neXus (this repository) | semantics, contracts, and runtime |
| [nex-ext](https://github.com/ssccsorg/nex-ext) | database engine adapters, external engine runners, and edge sync workers |

Contracts live in the stable core and implementations live outside it. A consumer implements a published trait, for example `FileIo`, in its own workspace, and the core never depends on a specific backend.

## Quick start

A stable Rust toolchain is enough for the default build. The wasm and MCU checks also need the `wasm32-unknown-unknown`, `wasm32-wasip2`, and `riscv32imac-unknown-none-elf` targets, which CI installs.

```bash
cargo build --workspace           # the root workspace
cd nex && cargo test --workspace  # the storage workspace
./run.sh --core                   # fmt, clippy, tests, and the wasm gate
```

Two entry points are one line each.

```bash
cargo run -q -p nexus-storage-ref -- self-check
npm --prefix storage/ref/demo/app run demo
```

The first reads a volume through the storage surface and prints what it found, and needs only the toolchain. The second also needs Node 20 or newer: it builds the reference, imports the committed volume, and serves a board that draws the same reads. Both are the same line on macOS, Linux, and Windows. What the reference is and what it deliberately leaves out is in [storage/ref](storage/ref), and the demonstration is in [storage/ref/demo](storage/ref/demo).

The MCU runtime verification runs in the image `ghcr.io/ssccsorg/nexus-verify`, built from `verify/Dockerfile`, and boots `nexus-mcu-verify` on `qemu-system-riscv32 -machine virt`.

## Repository layout

The repository holds a root workspace and a nested storage workspace under [nex/](nex), which root members depend on by path. [nex/README.md](nex/README.md) describes the engine's execution and scaling model.

| Path | What |
|---|---|
| [fih/](fih) | the pure model: `Fact`, `Intent`, `Hint`, `CoordId`, the capability traits, the `Now` clock, and the cold query contract, no_std with alloc |
| [nex/](nex) | the storage engine workspace: `nex-fih`, `nex-io`, `nex-core`, and the `nex` process crate |
| [nex-server/](nex-server), [nexd/](nexd) | the board as a process over a Unix socket, and the daemon that supervises it |
| [storage/ref/](storage/ref) | the smallest consumer of the storage surface, and its demonstration |
| [storage/sim/](storage/sim) | a scenario-driven runner with in-memory backends |
| [verify/](verify) | host and MCU verification of the OS-less storage path |
| [apps/](apps) | applications built on the model, each with its own workspace and verifier |
| [libs/](libs), [benches/](benches), [playbooks/](playbooks) | shared host libraries, the multi-axis benchmarks, and consumer playbooks |

[apps/nex-calc](apps/nex-calc) is the minimal complete skeleton of the model, and the design basis for deterministic path accumulation.

## Use it

Pin the repository by revision.

```toml
[dependencies]
nex = { git = "https://github.com/ssccsorg/nexus", package = "nex", rev = "<commit>" }
nex-fih = { git = "https://github.com/ssccsorg/nexus", package = "nex-fih", rev = "<commit>" }
tokio = { version = "1", features = ["full"] }
```

`nex` re-exports the historical deep paths, so `nex::storage::core::FihStorage`, `nex::io::FileIo`, and `nex::io::FsIo` resolve without further wiring.

```rust
use nex::io::FsIo;
use nex::storage::core::FihStorage;
use nex_fih::{AsyncFactCapable, Content, Fact};

let io = FsIo::new("/tmp/nexus-store")?;
let store = FihStorage::new(io, "my-app");
store.rebuild_cache().await?;

let fact = Fact::new("user-note".to_string(), Content::from("remember this"), "my-app".into());
store.submit_fact(&fact).await?;
store.flush_pending().await?;
```

`FihStorage<I>` is generic over `I: FileIo`, whose contract lives in [chton](https://github.com/ssccsorg/chton). Three semantics hold for any backend: listing a missing prefix is an empty enumeration, listed keys include their prefix so they round-trip through `read`, and deleting a missing key is a no-op.

For a supervised deployment, `nexd` spawns `nex-server` and serves `/tmp/nexd.sock`, and `nex_client` is the typed client. The wire shape and the method list are in [docs/wire-protocol.md](docs/wire-protocol.md).

For a target without an operating system, consume `nex-fih` and `chton` with `default-features = false`, provide `alloc` and a `critical-section` implementation, and inject a clock through `Now`. [verify/mcu](verify/mcu) is the working reference: a riscv32imac firmware that runs the round trip under QEMU inside a 512 KB budget.

## Verification

GitHub Actions runs the same flows [run.sh](run.sh) runs locally, one job each: core (fmt, clippy, tests, the wasm gate, and the MCU runtime under QEMU), server, gateway, apps, and playbooks.

```bash
./run.sh              # everything
./run.sh --core       # core only
./run.sh --gateway    # the gateway layer
./run.sh --apps       # the standalone app verifiers
./run.sh --server     # nex-server over a Unix socket
./run.sh --bench      # the multi-axis benchmarks
./run.sh --playbooks  # the consumer playbooks
```

`scripts/run-core.sh` takes `--check`, `--clippy`, and `--test` for a focused local run.

## Documentation

- Development guide: <https://docs.ssccs.org/projects/nexus/development>
- Project documentation: <https://docs.ssccs.org/projects/nexus/>
- Design records: [docs/](docs), including the cold query direction and the wire protocol. The storage implementation's own design, measurement, and rejected alternatives are kept with the product rather than published here.

## Contributing

Open an issue first, then a branch named `{issue-number}-{subject-words}`, then a pull request titled `PR: {category}: {message}`. Commits use `{category}: {message}`, with `#{issue}` on a task branch.

Two rules carry the rest: keep the change scoped to the goal, and put tests under `tests/` rather than inline. The infrastructure contract rule governs every change: the core defines capability traits and stable contracts, implementations implement them without modifying the core surface, and a change that alters a core contract has to demonstrate the capability gap first.

Run `cargo fmt`, `cargo clippy`, and the relevant `./run.sh` target before opening the pull request.

## License

Apache-2.0. See [LICENSE](LICENSE).
