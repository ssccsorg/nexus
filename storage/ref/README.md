# nexus-storage-ref

The reference implementation of the neXus storage contract's read half.

A running instance is at [rf-storage-demo.nexus.ssccs.org](https://rf-storage-demo.nexus.ssccs.org): a read-only board
over the volumes committed here, drawn by the `skene` frame. The board is one rendering of the records, because neXus
defines the record, the coordinate, and the read surface, and no screen.

neXus is a state fabric whose unit is a transition: a fact, an intent, and a hint compose into a new fact,
`F x I x H -> F'`. A coordinate addresses the fact, a record keeps it, and the transition is itself recorded, so the
record is the state. This crate answers what a committed volume holds. It has no write path.

## What it provides

- An address is a key, and it resolves by a rule rather than by a lookup.
- A point read is one read of the medium, and it reads nothing else.
- A record's address follows from what it holds, so verification is recomputation.
- A listing is the only scan. A question over several axes is answered from it and reads no record.
- A volume sits beside an existing application file and leaves that file alone.

## Records

| Record | What it is |
|---|---|
| Fact | an immutable observation, addressed by its coordinate |
| Intent | a stateful exploration with a lifecycle |
| Hint | a read-only signal that bounds the work |

The same three records carry a calculation, a process, or an agent's accumulation of knowledge. The write path, the
intent lifecycle, and the wire are other consumers of them, and this crate depends on none of them.

## Quick start

From the repository root:

```bash
cargo run -q -p nexus-storage-ref -- self-check
cargo run -q -p nexus-storage-ref -- keys --volume storage/ref/demo/volume
```

The board, from a checkout with Rust and Node:

```bash
./run.sh --demo          # check the demonstration, then serve it on 5292
./run.sh --demo-serve    # serve the committed board, no check
```

`demo/README.md` covers what the board shows, and a container that needs nothing installed on the host.

## Commands

Every command reads a volume, or the hub at `--hub <socket>` when one is named.

| Command | What it answers |
|---|---|
| `keys` | the record keys of a volume, under `facts/` by default and under any prefix asked for |
| `box` | the keys a range over one coordinate axis admits, from the keys alone |
| `read` | the bytes at the address a key implies |
| `verify` | whether an address follows from what the record holds |
| `show` | one record as the volume holds it: kind, address, origin, creator, time, and content |
| `sidecar` | a volume placed beside an existing application file |
| `self-check` | the contract, in a temporary volume |

An address is 60 base-36 digits, three to an axis. The terminal, the file name, and the board show the same text, so a
printed address can be handed back to the tool unchanged, and a volume holds nothing outside ASCII.

## The volume

- `facts/f_<coordinate>.fact` is the record, and the coordinate is the file name between `f_` and `.fact`.
- `blob/<hash>.bin` is the payload and `blob/<hash>.bin.meta` its media type.

`demo/volume/` is one record as the format writes it. `demo/esg-volume/` is the chain behind the live board, written
from `demo/esg.scenario.json`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | The command finished. |
| 1 | The request or the volume is wrong, so no question was asked. |
| 2 | The request was well formed and the volume holds no record at that address. |

A read never creates the directory it reads. `keys` answers 0 or 1 only, so a script that needs to know whether a prefix
found anything reads the count it prints.

## What a product adds

The records and the read path do not change with any of this.

| Area | Here | A product on the same contract |
|---|---|---|
| Write | none; the volumes are fixtures | records written at an assigned or a content address, with a conflict and idempotence rule |
| Multi-axis read | a listing and a prefix filter | a range over several coordinate positions, answered from the keys alone |
| Medium | a host directory | a log a part can take, with segments, an index on the medium, and recovery from a power cut |
| Target | a host program | a host and a no_std target under a stated memory budget |
| Search and seal | none | text and semantic search, and a key held in a secure element |

The seam an external claim system attaches at is `docs/integration-seam.md`, a mapping rather than an integration. No
number in this crate is a performance claim.

## Layout

| Path | What it is |
|---|---|
| `src/` | the read path, and the `nexus-storage-ref` tool |
| `tests/` | the contract, each case running the built binary |
| `demo/` | the volumes, the scenario they are written from, and the board over them |
| `docs/integration-seam.md` | where an external claim system attaches |

## License

Apache-2.0, inherited from the workspace.
