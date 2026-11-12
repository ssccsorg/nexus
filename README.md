# neXus

Content-Addressed State Fabric for Heterogeneous Systems. One coordinate space, one protocol, multiple directions, no orchestration.

Status: pre-1.0 (0.1.0), Apache-2.0, not published to crates.io. Pin by git revision.

## Overview

- **Peer-to-peer.** Servers, agents, edge devices, data centers, and terminals attach to the same board through one binary, `nex`, and speak the same coordinate-addressed I/O interface. No hierarchy, no central orchestrator.
- **FIH records.** Every interaction is a `Fact` (immutable observation), an `Intent` (work with a lifecycle `submit → claim → heartbeat → conclude`), or a `Hint` (a signal that bounds or governs the work).
- **Consumer-defined axes.** The core fixes three axes — observation, work, constraint — and the traits that compose them. What a Fact records, what an Intent produces, and what a Hint bounds are left to the consumer. The same structure carries a calculation graph, a storage transaction, or a goal space without changes to the core surface.
- **Scale-invariant.** The same record structure and composition rules reappear at every scale, from a single agent to an ecosystem.
- **Shared coordinate space.** A transition composes records into a new fact, `F x I x H -> F'`, on the same board as the data, so work and data resolve through one coordinate space.
- **Recorded traversal.** A computation is a recorded path: the record is the state, the traversal is the computation, and replay equals recomputation.
- **Spatial identity, immutable records.** Identity is a coordinate, not a hash. Resolving a record by identity is independent of record count; coordination cost scales with attachments, not data volume.
- **One address, two forms.** The value is twenty axis values below 11,172 and is the authority for every comparison. Its text is a fixed-width lowercase base-36 spelling, ASCII, order preserving, sixty bytes at depth twenty. A process holds the value; the text exists at a boundary.
- **Verification without trust.** A reader checks a record by recomputation rather than by trusting the writer.
- **One core, many targets.** The same core compiles for hosts, WASM targets, and no_std MCU-class parts. `Now` is injected, so the core never reads a system clock.

| Form | Size | Where it is used |
|---|---|---|
| value, `CoordId<20>` | 40 bytes | every in-process structure: indexes, maps, comparisons |
| canonical text | 60 bytes | a storage key, a file name, a wire field, a log line |

Both spellings are sixty UTF-8 bytes, so a volume's keys, segments, and indexes are the same size either way. The rule and its measurement are in [the address is a value and the text is a boundary](docs/2026-11-06-the-address-is-a-value-and-the-text-is-a-boundary.md).

The full model, coordinate identity, storage tiers, runtime architecture, and measured behavior are specified in the [whitepaper](https://docs.ssccs.org/projects/nexus/whitepaper).

## Repository Structure

Root workspace, and a nested workspace under [`nex/`](nex) for the state model, the IO seam,
the wire, the router, the hub, and the process layer.

| Path | Contents |
|---|---|
| [`fih/`](fih) | `Fact`, `Intent`, `Hint`, `CoordId`, capability traits, `Now`, cold query contract. `no_std + alloc` |
| [`nex/`](nex) | the state model (`nex-fih`, `fih-model`), the IO seam (`nex-io`), the wire (`nex-protocol`), the routing table (`nex-router`), the hub (`nex-hub`), and the process layer (`nex`) |
| [`nex-server/`](nex-server), [`nexd/`](nexd) | the board served over a Unix socket, its typed client (`nex-client`), and the daemon that supervises it |
| [`storage/ref/`](storage/ref) | reference node: read a volume by key, answer a box from the keys, check a record by recomputing its address |
| [`apps/`](apps) | applications built on the model, each with its own workspace and verifier |
| [`libs/`](libs) | shared host libraries |
| [`playbooks/`](playbooks) | consumer playbooks |
| [`docs/`](docs) | design records, cold query direction, wire protocol |

[`chton`](https://github.com/ssccsorg/chton) and [`nex-ext`](https://github.com/ssccsorg/nex-ext) are separate repositories: the storage contract, and the adapter that attaches a host database engine to it.

### The reference node

`storage/ref` demonstrates the storage contract at its simplest. It reads a key, lists a prefix, answers a box, and checks a record by recomputing its address. A box is answered by listing the keys and comparing their coordinates, so its cost is one pass over the key space.

The board at [rf-storage-demo.nexus.ssccs.org](https://rf-storage-demo.nexus.ssccs.org) draws those reads over committed volumes, read-only.

The contract states what a reader receives. What a node holds, how it finds it, and what it costs to find are the node's own decisions. The table is the whole of the distinction this repository draws.

| Decision | This node | A node is free to decide otherwise |
|:---|:---|:---|
| Answering a box | lists the keys and compares their coordinates, one pass over the key space | prune by leading axis prefix and answer from what it keeps |
| Filtering | walks the record maps and applies the predicates field by field | read `axis_hints` and narrow before touching a record |
| Read cost | holds the record it returns | hold a read within a budget it declares |
| Memory | the record maps grow with the records the volume holds | bound what it holds and re-read what it does not |
| Caching | none, beyond the eviction calls the contract names | cache and evict under a policy it owns |
| Writing | buffers in `pending` and reaches the medium on `flush_pending` | batch, journal, and recover on its own terms |
| Target | the record path builds for `riscv32imac`; the reader is a host transport behind its own feature | run the same contract on a device with its own budget |

None of the right-hand column is implemented here. A node is free to make those choices because the contract does not name them, and a reader receives the same record either way.

## Getting Started

```bash
cargo build --workspace           # root workspace
cd nex && cargo test --workspace  # model, IO seam, reference store, process layer
./run.sh --core                   # fmt, clippy, tests, wasm gate
```

Two entry points, one line each.

| Entry point | Requirements | Behavior |
|---|---|---|
| `cargo run -q -p nexus-storage-ref -- self-check` | a Rust toolchain | runs the storage contract in a temporary volume and prints the coordinate it derived |
| `./run.sh --demo` | a Rust toolchain and Node 20+ | builds the reference, checks the demonstration, and serves the board on `http://localhost:5292` |

Both run on macOS, Linux, and Windows. `npm --prefix storage/ref/demo/app run demo` is the same demonstration run
without a POSIX shell.

The wasm check and the no_std anchor check require targets beyond the default build.

| Check | Requirements | Execution |
|---|---|---|
| wasm gate | `wasm32-unknown-unknown`, `wasm32-wasip2` | cargo, on the host |
| no_std anchors | `wasm32-unknown-unknown`, `riscv32imac-unknown-none-elf` | `cargo check --no-default-features` over the core crates, and the anchor tests |

CI installs all three targets. Local runs of those checks require installing them first.

## Usage

Pin the repository by revision.

```toml
[dependencies]
nex = { git = "https://github.com/ssccsorg/nexus", package = "nex", rev = "<commit>" }
nex-fih = { git = "https://github.com/ssccsorg/nexus", package = "nex-fih", rev = "<commit>" }
nexus-storage-ref = { git = "https://github.com/ssccsorg/nexus", package = "nexus-storage-ref", rev = "<commit>" }
tokio = { version = "1", features = ["full"] }
```

```rust
use nex::io::FsIo;
use nex_fih::{AsyncFactCapable, Content, Fact};
use nexus_storage_ref::RefFihStorage;

let io = FsIo::new("/tmp/nexus-store")?;
let store = RefFihStorage::new(io, "my-app");
store.rebuild_cache().await?;

let fact = Fact::new("user-note".to_string(), Content::from("remember this"), "my-app".into());
store.submit_fact(&fact).await?;
store.flush_pending().await?;
```

`rebuild_cache` reconstructs the in-memory index from the medium. It is safe on startup and after a crash; the medium is the source of truth.

The storage surface is a trait, so a backend is the consumer's. Three semantics hold for any of them.

- Listing a missing prefix is an empty enumeration.
- Listed keys include their prefix, so they round-trip through `read`.
- Deleting a missing key is a no-op.

The contract is defined in [`chton`](https://github.com/ssccsorg/chton).

A supervised deployment runs the board as a process.

- `nexd` spawns `nex-server` and serves `/tmp/nexd.sock`.
- `nex_client` is the typed client.
- Method list: [`docs/wire-protocol.md`](docs/wire-protocol.md).

Targets without an operating system require three things.

- Consume `nex-fih` and `chton` with `default-features = false`.
- Provide `alloc` and a `critical-section` implementation.
- Inject a clock through `Now`.

The reference node is `RefFihStorage` in [`storage/ref`](storage/ref), the crate `nexus-storage-ref`. Its record path builds for `riscv32imac-unknown-none-elf` with `--no-default-features`; the command-line reader is a host transport behind its own `reader` feature.

## Minimal Examples

Each is one part of the project in its smallest complete form.

| Path | Scope |
|---|---|
| [`apps/nex-calc`](apps/nex-calc) | the FIH model itself: a calculation is a recorded traversal, and the traversal is the computation |
| [`storage/ref`](storage/ref) | the storage surface: read a volume by key, answer a multi-axis question from the keys, check a record by recomputing its address. Carries a [demonstration board](storage/ref/demo) that draws the same reads |
| [`apps/nex-derive`](apps/nex-derive) | a directed goal space: a goal is work, intent, and contract, and resolving it derives a fact |

## Testing

GitHub Actions runs the same flows [`run.sh`](run.sh) runs locally, one job each.

| Job | Scope |
|---|---|
| core | fmt, clippy, tests, the wasm gates, and the no_std anchors |
| server | `nex-server` and `nex-client` over a Unix socket |
| gateway | the HTTP API in `apps/nex-api` and the serialization proxy in `libs/serde-proxy` |
| apps | the standalone app verifiers |
| demo | the reference tool and the Vite demonstration: install, tests, bundle, and the board the bundle carries |
| playbooks | the consumer playbook scenarios |
| deploy | publishes the built board to Cloudflare Pages, on `main` and after every job |

```bash
./run.sh                # everything
./run.sh --core         # core only
./run.sh --gateway      # the gateway layer
./run.sh --apps         # the standalone app verifiers
./run.sh --server       # nex-server over a Unix socket
./run.sh --demo         # the reference tool and its board, checked and then served
./run.sh --demo-serve   # serve the committed board, no check
./run.sh --demo-check   # the same check without the server, which is what CI runs
./run.sh --demo-install # the frame and the install alone, which is what the deploy needs
./run.sh --demo-deploy  # publish the built board to Cloudflare Pages
./run.sh --playbooks    # the consumer playbooks
```

[`scripts/run-core.sh`](scripts/run-core.sh) takes `--check`, `--clippy`, and `--test` for a focused local run.

## Roadmap

Two questions follow from storage in a real deployment: where the bytes move, and what becomes of the data already held. Neither is answered here alone, and both have a place already.

- Transport, signal and command traffic both, is a consumer of the io seam, and the seam belongs to [`chton`](https://github.com/ssccsorg/chton). This repository carries a reference wire over a Unix socket, not a transport for a field bus or a network.
- A host database engine is attached rather than replaced: [`nex-ext`](https://github.com/ssccsorg/nex-ext) is the adapter for that.
- A foreign schema is carried in by a converter on top of the same contract. The mapping from a schema to the axis space is declared rather than inferred, because the decisions that matter (which column is the identity, how an update becomes a record, what "current" means) are the consumer's.

The reference node is read-only, so a write path, storage tiers, and a seal are layers a product adds rather than work outstanding here.

## Documentation

- **Whitepaper**: <https://docs.ssccs.org/projects/nexus/whitepaper> — the model, coordinate identity, storage tiers, runtime architecture, and verified behavior.
- Development guide: <https://docs.ssccs.org/projects/nexus/development>
- Project documentation: <https://docs.ssccs.org/projects/nexus/>
- Design records: [`docs/`](docs), including the cold query direction, the wire protocol, and [the boundary between an address and its text](docs/2026-11-06-the-address-is-a-value-and-the-text-is-a-boundary.md). The storage implementation's own design, measurement, and rejected alternatives are kept with the product rather than published here.

## Contributing

Open an issue first, then a branch named `{issue-number}-{subject-words}`, then a pull request titled `PR: {category}: {message}`. Commits use `{category}: {message}`, with `#{issue}` on a task branch.

Two rules carry the rest.

- Keep the change scoped to the goal, and put tests under `tests/` rather than inline.
- The infrastructure contract rule: the core defines capability traits and stable contracts, implementations implement them without modifying the core surface, and a change that alters a core contract has to demonstrate the capability gap first.

Run `cargo fmt`, `cargo clippy`, and the relevant `./run.sh` target before opening the pull request.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
