# nexus-storage-ref

The reference implementation of the neXus storage domain. One volume, one record, one coordinate. No resident index, no database replacement.

This crate is a consumer of the public storage surface: `fih-model`, `nex-fih`, `nex`, and `chton`'s `FsIo`. It does not depend on any product crate.

## What it shows

The record is the unit. A record has content, an origin, and a creator. Its coordinate is derived from those three values, so the same record produces the same coordinate on a host and on a smaller target.

The volume is the storage. The reference uses `FsIo`, a directory of keys. It adds the volume beside an existing application file and does not touch the file, its schema, or its queries. That is the no-migration claim.

The query is the key set. A multi-axis question reads the record keys; there is no resident index whose memory grows with the record count.

## Commands

```bash
# Write the sample record to a volume.
cargo run -p nexus-storage-ref -- write --volume ./records

# Read it back by coordinate.
cargo run -p nexus-storage-ref -- read --volume ./records --id <coordinate>

# List the records.
cargo run -p nexus-storage-ref -- query --volume ./records

# Recompute the coordinate and the content hash.
cargo run -p nexus-storage-ref -- verify --volume ./records --id <coordinate>

# Add a volume beside an existing application file.
cargo run -p nexus-storage-ref -- sidecar --app ./app.db --volume ./records

# Print the host-side coordinate for the sample record.
cargo run -p nexus-storage-ref -- device-id

# Run the full host path in a temporary volume.
cargo run -p nexus-storage-ref -- self-check
```

`--record` takes a JSON file with `origin`, `creator`, `mime_type`, and `payload`. The sample lives at `samples/asset-event.json`.

## Portability

The host prints a coordinate with `device-id`. The MCU verifier computes the same coordinate when it is given the same payload bytes, origin, and creator. The two strings matching is the portability evidence. The record does not need to travel through a network for the coordinates to agree.

The current MCU evidence is `verify/mcu` in this workspace: a riscv32imac firmware that boots under QEMU and runs the storage round trip inside the memory budget. The reference CLI is the host side of that comparison.

## Boundary

The reference is a downstream consumer. It uses the public API and the file-backed channel. It does not add a new storage backend to the core, and it does not expose an external claim protocol. An external claim system attaches at the seam described in `docs/integration-seam.md`.

## License

Apache-2.0, inherited from the workspace.
