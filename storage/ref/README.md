# nexus-storage-ref

The smallest consumer of the neXus storage surface. It reads a volume through the storage contract and nothing else.

## Scope and goal

The goal is that a reader can see what the storage contract guarantees, and can run that part in a few minutes, before choosing an implementation to build on.

The scope is the read side of one volume on a host: the key space, one point read, and one listing.

It is not a product, a conformance suite, or a measurement. It reads a committed volume and reports what the bytes are. A product built on this surface covers the layers in [What a full implementation adds](#what-a-full-implementation-adds), and this crate deliberately stops below them.

## What it shows

The volume is a key space. A record's address is the key its bytes live at, so a reader that holds an address reads one key and no other. A listing hands over the keys, and that is the only scan the surface offers.

Three things follow from that, and they are what the commands exercise.

- An address resolves to a key by a rule, not by a lookup.
- A point read is one read of the medium.
- A listing is the only scan, so a prefix read is a listing.

A volume also sits beside an existing application file and leaves that file alone.

## The sample volume

`samples/volume/` is one record as the format writes it, and it is committed so the crate has something to read without a write path.

- `facts/f_<coordinate>.fact` is the record.
- `blob/<hash>.bin` is the payload, and `blob/<hash>.bin.meta` is its media type.

The record's origin is `did:example:asset-1`, its creator is `sensor-1`, and the payload is one JSON event. The coordinate is the file name between `f_` and `.fact`. The bytes are the record encoding, and nothing here decodes them.

## Commands

```bash
# List the record keys.
cargo run -p nexus-storage-ref -- keys --volume ./samples/volume

# List the whole key space, payload keys included.
cargo run -p nexus-storage-ref -- keys --volume ./samples/volume --prefix ""

# Read the bytes at the key an address implies.
cargo run -p nexus-storage-ref -- read --volume ./samples/volume --coordinate <coordinate>

# Place the sample volume beside an existing application file.
cargo run -p nexus-storage-ref -- sidecar --app ./app.db --volume ./records

# Run the read path in a temporary volume.
cargo run -p nexus-storage-ref -- self-check
```

`read` prints the key and the number of bytes at it. An address the volume does not hold exits with a failure, so a script can tell the two outcomes apart.

## What a full implementation adds

The crate stops at the storage contract by design. Each row is a layer above it, and the difference between the two columns is what a product built on this surface provides. Nothing below is measured here.

| Layer | Here | In a full implementation |
|---|---|---|
| Query | A listing hands over the keys, and the caller reads the records it wants | A range over several coordinate axes, answered from the keys, so a box reads no record |
| Record | Opaque bytes | A framing read and typed views, so a caller never decodes the encoding, and a read that answers what a record says about itself without its payload |
| Medium | A host directory | A log a part can take: segments, an index on the medium, a ring that retires its oldest segment, and recovery from a power cut |
| Format | A committed fixture | A version declared on the volume, key validation, and the volume directory, so two implementations agree on a volume |
| Device | A host program | A target build under a stated memory budget, a caller-supplied storage seam of bytes, a write and an erase, and a C ABI so firmware links a prebuilt archive |

No number in this crate is a performance claim. An external claim system attaches at the seam in `docs/integration-seam.md`, which is a mapping rather than an integration.

## License

Apache-2.0, inherited from the workspace.
