# nexus-storage-ref

The smallest consumer of the neXus storage surface. It reads a volume through the storage contract and nothing else.

This crate is a consumer of `nex-fih` and `chton`'s `FsIo`. It depends on no product crate, writes no record, and decodes none.

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

## What it does not do

- No write path. The sample volume is the input.
- No query over several axes. A box over a coordinate is a layer above the storage contract.
- No index, resident or otherwise. The listing is the scan.
- No record decoding. The bytes are handed back as they are.
- No performance claim. Nothing in this crate is timed or measured.
- No target build. The commands are a host program.

Those are outside this crate. An external claim system attaches at the seam in `docs/integration-seam.md`.

## License

Apache-2.0, inherited from the workspace.
