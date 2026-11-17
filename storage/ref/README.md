# nexus-storage-ref

The reference implementation of the neXus storage contract's read half. It opens a committed volume and answers what
the volume holds. There is no write path. The board over the volumes is under `demo/`.

This node answers by listing keys and comparing coordinates. It is the reference: a node that prunes, caches, or
bounds a read is a different implementation and does not belong here.

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

`demo/README.md` covers the board, the container, and the deployment.

## Records

| Record | What it is |
|---|---|
| Fact | an immutable observation, addressed by its coordinate |
| Intent | a stateful exploration with a lifecycle |
| Hint | a read-only signal that bounds the work |

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

## Layout

| Path | What it is |
|---|---|
| `src/` | the read path, and the `nexus-storage-ref` tool |
| `tests/` | the contract, each case running the built binary |
| `demo/` | the volumes, the scenario they are written from, and the board over them |
| `docs/integration-seam.md` | where an external claim system attaches |

No number in this crate is a performance claim.

## License

Apache-2.0, inherited from the workspace.
