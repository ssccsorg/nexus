# The demonstration

Two directories, one purpose: to show what the reference crate claims rather than state it.

`nexus-storage-ref` reads a volume and prints. This directory puts a screen on the same reads, so a developer or a
technical auditor can see the key space, a point read, and a verification without reading the crate first.

## The two directories

| Path | What it holds |
|---|---|
| `volume/` | one record as the format writes it, committed so both the tool and the application have something to read |
| `app/` | a Vite application that runs the tool over that volume and draws what came back |

`volume/` holds no write path and no generator. It is the bytes of one record: `facts/f_<coordinate>.fact`, the payload
under `blob/`, and the payload's media type beside it. The record's coordinate is its file name. Nothing in this
directory decodes it.

`app/` holds the board, the run, and the importer that writes both. It is a deployment of the `skene` frame, which
supplies the address, the record model, the panels, and the router.

## How they relate

`app/tools/import-volume.mjs` is the only path between them. It runs `keys`, `box`, `read`, `verify`, `sidecar`, and
`self-check` against `volume/`, keeps each exit code, and writes `app/projects.json` and `app/scenario/sample-volume.txt`
from what the tool printed.

The importer counts nothing. A second model of the store would be a model free to disagree with the store, so every
number on the screen is a number the tool printed, and each `count` line in a run is the parser's arithmetic over the
facts that run writes. An edit that moves a line has to move its count, or the tests fail.

## What the demo shows

Each claim of the reference crate has a place on the screen.

- An address is a key, and it resolves by a rule. The `keyspace` reading lists one row per key, grouped by the prefix
  the key carries.
- A point read is one read of the medium. The run carries the byte count the `read` verb printed.
- A record's address follows from what it holds. The run carries the fields the `verify` verb recomputed it from.
- A listing is the only scan, so a box is answered from it. The run carries the axes the box narrowed and the records
  it admitted.
- A volume sits beside an application file and leaves it alone. The run carries the size the `sidecar` verb measured.

Every address is drawn in its base-36 rendering, three characters an axis, so the board and the runs carry no native
character and a printed address can be handed back to the tool unchanged.

## What the demo does not show

- A write path. The volume is committed, so the demonstration reads and never writes.
- Performance. The demo carries no timing and no comparison, so no number on the screen is a performance claim.
- The layers a product adds above the read surface. The crate README's table separates those from what is shown here.

## Running it

From the repository root, with a `skene` checkout beside this one:

```bash
cargo build -p nexus-storage-ref
cd storage/ref/demo/app
npm install
npm run import -- --id sample-volume --title "Sample volume"
npm run dev
```

`npm test` in `app/` compares the committed run with a fresh import and fails when the two disagree, so a stale run is a
test failure rather than a screen that lies. `app/README.md` covers the application's layout, its tests, and the frame
it deploys.
