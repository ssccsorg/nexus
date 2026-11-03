# neXus

Index-free, P2P state fabric for multi-axis records across heterogeneous systems. One coordinate space, one protocol, multiple directions, no orchestration.

neXus implements the FIH (Fact, Intent, Hint) model of the SSCCS stack: a board of records that any peer can read and extend, with no privileged orchestrator. The same core compiles for hosts, WASM targets, and no_std MCU-class parts.

Status: pre-1.0 (0.1.0), Apache-2.0, not published to crates.io. Pin by git revision.

## What it is

Every interaction with a neXus board is one of three records.

| Record | What it is |
|---|---|
| Fact | an observation that never changes |
| Intent | a piece of work with a lifecycle, from `submit` through `claim` and `heartbeat` to `conclude` |
| Hint | a signal that bounds or governs the work |

A transition composes them into a new fact, `F x I x H -> F'`. The new fact is a record like any other, so the work that produced data sits on the same board as the data, and a peer reads both the same way.

A fact's address is derived from what the fact holds. A reader who holds a record can therefore check it by recomputation rather than by trusting the peer that wrote it, and no counter or lookup table has to agree across peers.

## What it gives

| If you need | neXus provides | Shown in |
|---|---|---|
| State that outlives the process | one coordinate space for the data and for the work that produced it | [apps/nex-calc](apps/nex-calc) |
| Verification without a trusted writer | an address derived from content, origin, and creator, so a record is checked by recomputation | [fih/](fih) |
| A target with no operating system | the same core on a host, on WASM, and on a no_std MCU-class part inside a 512 KB budget | [verify/mcu](verify/mcu) |
| Operation without an orchestrator | single-owner execution units that coordinate through the board and scale by replication | [nex/README.md](nex/README.md) |
| An existing stack left in place | the medium is a trait, so a filesystem, an object store, or memory plugs in below, and host database engines attach through the cold query contract | [chton](https://github.com/ssccsorg/chton), [nex-ext](https://github.com/ssccsorg/nex-ext) |
| A board that is a service | JSON-RPC over a Unix socket, with a daemon and a typed client | [docs/wire-protocol.md](docs/wire-protocol.md) |

## The smallest examples

Each of these is one part of the project in its smallest complete form. Read the one that matches the question, then follow it to the full document.

| Path | What it is the smallest example of |
|---|---|
| [apps/nex-calc](apps/nex-calc) | the FIH model itself: a calculation is a recorded traversal, and the traversal is the computation |
| [storage/ref](storage/ref) | the storage surface: read a volume by key, answer a multi-axis question from the keys, and check a record by recomputing its address. It carries a [demonstration board](storage/ref/demo) that draws the same reads |
| [apps/nex-derive](apps/nex-derive) | a directed goal space: a goal is work, intent, and contract, and resolving it derives a fact |
| [storage/sim](storage/sim) | a run driven by a scenario, over in-memory backends |
| [verify/osless](verify/osless) | the OS-less storage path on a host, including the cross-thread critical section stress |
| [verify/mcu](verify/mcu) | the same round trip as bare-metal firmware, under QEMU |

## Run it

```bash
cargo build --workspace           # the root workspace
cd nex && cargo test --workspace  # the storage workspace
./run.sh --core                   # fmt, clippy, tests, and the wasm gate
```

Two entry points are one line each.

| Entry point | Needs | What it does |
|---|---|---|
| `cargo run -q -p nexus-storage-ref -- self-check` | a Rust toolchain | runs the storage contract in a temporary volume and prints the coordinate it derived |
| `npm --prefix storage/ref/demo/app run demo` | a Rust toolchain and Node 20 or newer | builds the reference, imports the committed volume, and serves the board |

Both are the same line on macOS, Linux, and Windows.

The wasm and MCU checks need targets and a runtime beyond the default build.

| Check | Needs | Runs |
|---|---|---|
| wasm gate | `wasm32-unknown-unknown`, `wasm32-wasip2` | cargo, on the host |
| MCU runtime | `riscv32imac-unknown-none-elf` | `nexus-mcu-verify` on `qemu-system-riscv32 -machine virt`, inside `ghcr.io/ssccsorg/nexus-verify`, built from `verify/Dockerfile` |

CI installs all three targets. A local run of those checks needs them installed first.

## Use it

Pin the repository by revision.

```toml
[dependencies]
nex = { git = "https://github.com/ssccsorg/nexus", package = "nex", rev = "<commit>" }
nex-fih = { git = "https://github.com/ssccsorg/nexus", package = "nex-fih", rev = "<commit>" }
tokio = { version = "1", features = ["full"] }
```

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

The storage surface is generic over a trait, so a backend is the consumer's. Three semantics hold for any of them.

- Listing a missing prefix is an empty enumeration.
- Listed keys include their prefix, so they round-trip through `read`.
- Deleting a missing key is a no-op.

The contract is in [chton](https://github.com/ssccsorg/chton).

A supervised deployment runs the board as a process.

- `nexd` spawns `nex-server` and serves `/tmp/nexd.sock`.
- `nex_client` is the typed client.
- The method list is in [docs/wire-protocol.md](docs/wire-protocol.md).

For a target without an operating system, three things are needed.

- Consume `nex-fih` and `chton` with `default-features = false`.
- Provide `alloc` and a `critical-section` implementation.
- Inject a clock through `Now`.

[verify/mcu](verify/mcu) is the working reference.

## Where everything lives

The repository holds a root workspace and a nested storage workspace under [nex/](nex), which root members depend on by path.

| Path | What |
|---|---|
| [fih/](fih) | the pure model: `Fact`, `Intent`, `Hint`, `CoordId`, the capability traits, the `Now` clock, and the cold query contract, no_std with alloc |
| [nex/](nex) | the storage engine workspace: `nex-fih`, `nex-io`, `nex-core`, and the `nex` process crate |
| [nex-server/](nex-server), [nexd/](nexd) | the board as a process over a Unix socket, and the daemon that supervises it |
| [storage/](storage) | the reference consumer with its demonstration, and the scenario runner |
| [verify/](verify) | host and MCU verification of the OS-less path |
| [apps/](apps) | applications built on the model, each with its own workspace and verifier |
| [libs/](libs), [benches/](benches), [playbooks/](playbooks) | shared host libraries, the multi-axis benchmarks, and consumer playbooks |

## Verification

GitHub Actions runs the same flows [run.sh](run.sh) runs locally, one job each.

| Job | What it verifies |
|---|---|
| core | fmt, clippy, tests, the wasm gate, and the MCU runtime under QEMU |
| server | `nex-server` and `nex-client` over a Unix socket |
| gateway | the HTTP API in `apps/nex-api` and the serialization proxy in `libs/serde-proxy` |
| apps | the standalone app verifiers |
| playbooks | the consumer playbook scenarios |

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

Two rules carry the rest.

- Keep the change scoped to the goal, and put tests under `tests/` rather than inline.
- The infrastructure contract rule: the core defines capability traits and stable contracts, implementations implement them without modifying the core surface, and a change that alters a core contract has to demonstrate the capability gap first.

Run `cargo fmt`, `cargo clippy`, and the relevant `./run.sh` target before opening the pull request.

## License

Apache-2.0. See [LICENSE](LICENSE).
