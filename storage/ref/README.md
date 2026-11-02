# nexus-storage-ref

The smallest consumer of the neXus storage surface. It reads a volume through the storage contract and nothing else.

## Scope and goal

The goal is that a developer or a technical auditor can see what the storage contract guarantees, and can run that part in a few minutes, before choosing an implementation to build on.

The scope is the read side of one volume on a host: the key space, one point read, and one listing.

It is not a product, a conformance suite, or a measurement. It reads a volume and reports what the bytes are. A product built on this surface covers the layers in [What a full implementation adds](#what-a-full-implementation-adds), and this crate deliberately stops below them.

## What it shows

The volume is a key space. A record's address is the key its bytes live at, so a reader that holds an address reads one key and no other. A listing hands over the keys, and that is the only scan the surface offers.

Three things follow from that, and they are what the commands exercise.

- An address resolves to a key by a rule, not by a lookup.
- A point read is one read of the medium.
- A prefix read is a listing followed by a filter.

A volume also sits beside an existing application file and leaves that file alone.

## The sample volume

`samples/volume/` is one record as the format writes it, and it is committed so the crate has something to read without a write path.

- `facts/f_<coordinate>.fact` is the record.
- `blob/<hash>.bin` is the payload, and `blob/<hash>.bin.meta` is its media type.

The record's origin is `did:example:asset-1`, its creator is `sensor-1`, and the payload is one JSON event. The coordinate is the file name between `f_` and `.fact`. The bytes are the record encoding, and nothing here decodes them.

Copy an address out of `keys` rather than typing one. Coordinate characters are visually similar, and a mistyped character is still a well formed address, so the tool reports it as absent rather than as a typo.

## Commands

```bash
# Run the contract in a temporary volume. Start here.
cargo run -p nexus-storage-ref -- self-check

# List the record keys.
cargo run -p nexus-storage-ref -- keys --volume ./samples/volume

# Keep the keys under a prefix, a coordinate prefix included.
cargo run -p nexus-storage-ref -- keys --volume ./samples/volume --prefix "facts/f_<one coordinate character>"

# List the whole key space, payload keys included.
cargo run -p nexus-storage-ref -- keys --volume ./samples/volume --prefix ""

# Read the bytes at the key an address implies.
cargo run -p nexus-storage-ref -- read --volume ./samples/volume --address <coordinate>

# Place the sample volume beside an existing application file.
cargo run -p nexus-storage-ref -- sidecar --app ./app.db --volume ./records
```

`cargo test -p nexus-storage-ref` holds the contract these commands promise: the exit codes below, a read that creates nothing, and the prefix filter.

`keys` lists the volume and keeps the keys that start with the prefix, because this channel resolves a listing prefix as a path. The filter is the tool's, and the cost it shows is the listing itself.

`read` accepts the canonical coordinate, and also the 60-character ASCII rendering of one, which `fih_model::ascii` produces. A rendering is decoded to the coordinate before the key is derived, so both forms address the same record and a rendering that decodes to nothing the volume holds still exits 2. Anything else is refused before the volume is touched.

`sidecar` refuses a directory that already holds content, then compares the application file's size and modification time before and after the copy and prints what it measured. Size and modification time keep the check bounded when the application file is large, so the check is a guard against the copy touching the file rather than a content hash of it.

`self-check` takes the coordinate from the listing rather than from a constant, derives the key from it again, reads the record, and checks that the application file is unchanged.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | The command finished. For `read`, the record is present. |
| 1 | The request or the volume is wrong, so no question was asked. A missing volume, an address that is not a coordinate, a directory that already holds content. |
| 2 | The request was well formed and the volume holds no record at that address. |

A read never creates the directory it reads. Exit 2 is the answer for an address that parsed and found nothing, which keeps it apart from a misplaced path or a mistyped address.

`keys` answers 0 or 1 only. An empty volume and a prefix that matches nothing are finished commands, not an absent record, so a script that needs to know whether a prefix found anything reads the count it prints.

## What a production implementation adds

The crate stops at the storage contract by design. This table compares it with the production implementation that exists today, layer by layer. It states capabilities and no numbers.

| Area | This crate | A production implementation |
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

The comparison covers the record layer. The seal that proves who was in control is in development and is in neither column. Nothing here is measured, so no number in this crate is a performance claim.

An external claim system attaches at the seam in `docs/integration-seam.md`, which is a mapping rather than an integration.

## License

Apache-2.0, inherited from the workspace.
