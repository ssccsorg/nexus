# nexus-storage-ref

The demonstration is live at [nexus-demo-b5z.pages.dev](https://nexus-demo-b5z.pages.dev): an ESG evidence campaign
read on the `skene` frame, with its records, the chain they compose, the plant on a map, and the campaign timeline. The
volume behind it is committed, so the board is read-only and needs no install.

neXus is a state fabric. Its unit is a transition: a fact, an intent, and a hint compose into a new fact,
`F x I x H -> F'`. A coordinate addresses the fact, a record keeps it, and the transition is itself recorded, so the
record is the state.

Storage is one surface of that structure, the place a fact is kept and found. This crate takes that surface and only its
read half: a key space, one point read, and one listing. It exists so a developer or a technical auditor can run that
half in a few minutes and see what the contract guarantees before choosing an implementation to build on.

It is not a product, not a conformance suite, and not a measurement. It reads a volume and reports what the bytes are.

## Run it in one line

```bash
cargo run -q -p nexus-storage-ref -- self-check
```

That runs the contract in a temporary directory and prints the coordinate it derived. To read a committed volume:

```bash
cargo run -q -p nexus-storage-ref -- keys --volume storage/ref/demo/volume
```

The demonstration is one line too:

```bash
npm --prefix storage/ref/demo/app run demo
```

It builds this crate, puts the `skene` frame beside the checkout when it is absent, imports the committed volume, and
serves the board. Each command is the same on macOS, Linux, and Windows, and none needs a POSIX shell.

## The structure, and the surface this crate takes

neXus keeps three record types over one coordinate space.

| Record | What it is |
|---|---|
| Fact | an immutable observation, addressed by its coordinate, derived from its content, its origin, and its creator |
| Intent | a stateful exploration with a lifecycle, from submit through claim to conclude |
| Hint | a read-only signal that bounds the work |

A transition composes them into a new fact, and that fact persists. The composition is what the model provides, and it is
not specific to storage: the same records carry a calculation, a process, or an agent's accumulation of knowledge.

The surfaces are separable. This crate takes storage, and inside it the read path. The write path, the intent lifecycle,
and the wire are other consumers of the same records, and this crate depends on none of them.

That separation is why the claims below are worth stating. A record is not a row in a log, because the coordinate that
addresses it is also the axis a later question is asked over, and because the work that produced it is a record at a
neighbouring coordinate.

## What this demonstrates

The record layer's claims are about the address, and this crate shows them on a volume it does not own.

- An address is a key. It resolves by a rule, not by a lookup.
- A point read is one read of the medium, and it reads nothing else.
- A record's address follows from what it holds, so holding the record is enough to check it. Verification is
  recomputation.
- A listing is the only scan, and a question over several axes is answered from it, so a box reads no record.
- A volume sits beside an existing application file and leaves that file alone.

Nothing in these claims depends on where the layer runs or who sells it. The ceiling near the end separates them from the
deployment work a product adds.

## Layout

| Path | What it is |
|---|---|
| `src/main.rs` | the tool: `keys`, `box`, `read`, `verify`, `sidecar`, `show`, and `self-check` over the read surface |
| `tests/cli.rs` | the contract, each case running the built binary |
| `demo/volume/` | one record as the format writes it, committed so the crate has something to read without a write path |
| `demo/esg-volume/` | the volume behind the live board: the evidence chain of one reporting campaign |
| `demo/app/` | the demonstration: a Vite application that reads these volumes through the `skene` frame |
| `demo/README.md` | what the demonstration holds, and how to run it |
| `docs/integration-seam.md` | where an external claim system attaches |

This file describes the tool. `demo/README.md` describes the demonstration, and `demo/app/README.md` describes the
application itself.

## The volume

`demo/volume/` is one record as the format writes it.

- `facts/f_<coordinate>.fact` is the record.
- `blob/<hash>.bin` is the payload, and `blob/<hash>.bin.meta` is its media type.

The record's origin is `did:example:asset-1`, its creator is `sensor-1`, and the payload is one JSON event. The coordinate
is the file name between `f_` and `.fact`. The bytes are the record encoding, and nothing here decodes them.

Copy an address out of `keys` rather than typing one. Coordinate characters are visually similar, and a mistyped character
is still a well formed address, so the tool reports it as absent rather than as a typo.

## Commands

Run these from the repository root, which is where `-p` finds the workspace and where the volume's path is written from.

```bash
# Run the contract in a temporary volume. Start here.
cargo run -p nexus-storage-ref -- self-check

# List the record keys, each address in its base-36 rendering.
cargo run -p nexus-storage-ref -- keys --volume storage/ref/demo/volume --prefix ""

# Keep the keys under a prefix. The prefix is matched against the key as this run prints it.
cargo run -p nexus-storage-ref -- keys --volume storage/ref/demo/volume --prefix "facts/"

# The records a box over one coordinate axis admits, answered from the keys.
cargo run -p nexus-storage-ref -- box --volume storage/ref/demo/volume --axis 0=000-000

# Read the bytes at the address a key implies.
cargo run -p nexus-storage-ref -- read --volume storage/ref/demo/volume \
  --address 00024o7m35346n71ge0ev7vj33q5cp6fg6ow3230ba3oo00g6mp0lm4rh0uq

# Recompute the address from what the record holds.
cargo run -p nexus-storage-ref -- verify --volume storage/ref/demo/volume \
  --address 00024o7m35346n71ge0ev7vj33q5cp6fg6ow3230ba3oo00g6mp0lm4rh0uq

# Print one record as the volume holds it: kind, address, origin, creator, time, and its content.
cargo run -p nexus-storage-ref -- show --volume storage/ref/demo/volume \
  --key facts/f_00024o7m35346n71ge0ev7vj33q5cp6fg6ow3230ba3oo00g6mp0lm4rh0uq.fact

# Place the volume beside an existing application file.
cargo run -p nexus-storage-ref -- sidecar --app ./app.db --volume ./records
```

Every example reads and prints addresses as they are, so the terminal, the file name, and the demonstration all show the
same text.

`cargo test -p nexus-storage-ref` holds the contract these commands promise: the exit codes below, a read that creates
nothing, and the prefix filter.

Every command prints each address as 60 base-36 digits, three an axis. That is the identity's own text: the coordinate
space is the syllable block's 11,172 values, and the spelling of a value is base-36. A box's bounds are read in the same
form, so a printed address can be handed back to the tool unchanged, and a record's key is the same text, so a volume
holds no character outside ASCII.

`keys` lists the volume and keeps the keys that start with the prefix, because this channel resolves a listing prefix as
a path. The filter is the tool's, and the cost it shows is the listing itself.

`box` reads the coordinate out of each key and answers the range from it. The store opens without record maps and no
record is read, so the answer costs the keys and nothing else. An axis the box does not name is free, `--axis` names
one, and both of its bounds are one axis window, so a second flag constrains a second axis.

`read` accepts the address and nothing else: a length that is not the depth's, a character outside the alphabet, or an
axis value above the space is refused before the volume is touched. An address that decodes but is not stored exits 2.

`sidecar` refuses a directory that already holds content, then compares the application file's size and modification time
before and after the copy and prints what it measured. Size and modification time keep the check bounded when the
application file is large, so the check is a guard against the copy touching the file rather than a content hash of it.

`self-check` takes the coordinate from the listing rather than from a constant, derives the key from it again, checks
that the record's own axis selects it from the keys, reads the record, and checks that the application file is unchanged.

`show` resolves one key and prints the record the medium holds: its kind, address, origin, creator, write time, the
record itself, and the content the record points at. A fact's content is its payload and an intent's is the
transformation it declares; a hint carries its content in the record. Content that is one line of text is printed as
text and anything else as hex, so a caller reads what a record says without decoding the format, and a multi-line
payload stays one line of output that a parser can read. A key outside `facts/`, `intents/`, and `hints/` is refused
before the volume is opened, because those are the three kinds the format defines and a listing key is not a record.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | The command finished. For `read`, the record is present. |
| 1 | The request or the volume is wrong, so no question was asked. A missing volume, an address that is not a coordinate, a directory that already holds content, a record whose address does not follow from what it holds. |
| 2 | The request was well formed and the volume holds no record at that address. |

A read never creates the directory it reads. Exit 2 is the answer for an address that parsed and found nothing, which
keeps it apart from a misplaced path or a mistyped address.

`keys` answers 0 or 1 only. An empty volume and a prefix that matches nothing are finished commands, not an absent record,
so a script that needs to know whether a prefix found anything reads the count it prints.

## The ceiling: what a product on this contract reaches

This crate stops at the read surface on a host. The role of this section is to keep the contract and the deployment
apart, so that neither is mistaken for the other. Nothing in the table is demonstrated by this crate, and none of it makes
the record layer truer. It is the shape the same contract takes when it is carried to a device or a server, which is what
a reader would see if this surface were developed into a product.

| Area | This crate | A product on the same contract |
|---|---|---|
| Write | None. The volume is a committed fixture. | A record addressed by the coordinate the writer assigns or by its content, with a conflict and idempotence rule for a repeated write |
| Point read | One key, and the bytes at it | One key, and a framing read that answers what a record says about itself without its payload |
| Multi-axis read | A listing and a prefix filter in the tool | A range over several coordinate positions, answered from the keys alone, so a box reads no record |
| Enumeration | The whole listing in memory | A walk that holds one page, and a store that keeps no record map, so memory does not follow the record count |
| Address input | The coordinate and its ASCII rendering | The coordinate alone, because the decode is a cost a device does not pay |
| Medium | A host directory | A log a part can take: segments, an index on the medium, a ring that retires its oldest segment, and recovery from a power cut |
| Format | None. The fixture states the layout. | A version declared on the volume, key validation, and a volume directory, so two implementations agree on a volume |
| Target | A host program | A host and a no_std target under a stated memory budget |
| Integration | A command line | A C ABI over a prebuilt archive, on a caller-supplied seam of bytes, a write, and an erase |
| Search | None | Text and semantic search over the records |
| Seal | None | A key held in a secure element, so a copied volume does not open, and the record states who was in control |

Every row is deployment work: where the same record layer runs, what carries it, and what a medium adds. The claims in
the section above do not change when those are added, and that is why the ceiling is worth stating at all. The record
layer is the same at a host and on a part; only the work around it changes.

Nothing here is measured, so no number in this crate is a performance claim. An external claim system attaches at the
seam in `docs/integration-seam.md`, which is a mapping rather than an integration.

## License

Apache-2.0, inherited from the workspace.
