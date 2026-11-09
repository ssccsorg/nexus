# The demonstration

Three directories, one purpose: to show what the reference crate claims rather than state it.

`nexus-storage-ref` reads a volume and prints. This directory puts a screen on the same reads, so a developer or a
technical auditor can see the key space, a point read, and a verification without reading the crate first.

## Run it in one line

```bash
npm --prefix storage/ref/demo/app run demo
```

That builds the tool, puts the frame beside the checkout when it is absent, imports the committed volumes, and serves
the board on the address Vite prints. It is the same line on macOS, Linux, and Windows, and it needs no POSIX shell. The
steps below are the same work one at a time, for a first run you want to watch.

## The three directories

| Path | What it holds |
|---|---|
| `volume/` | one record as the format writes it, committed so both the tool and the application have something to read |
| `esg-volume/` | records of one reporting period, read as a chain rather than as keys |
| `app/` | a Vite application that runs the tool over those volumes and draws what came back |

`volume/` holds no write path and no generator. It is the bytes of one record: `facts/f_<coordinate>.fact`, the payload
under `blob/`, and the payload's media type beside it. The record's coordinate is its file name. Nothing in this
directory decodes it.

`esg-volume/` holds the same format with more records in it: facts for measured quantities, intents that state what was
done to them, and a hint that names a disclosure requirement. It is committed, and it is read the same way, so the
chain it holds is the medium's answer and not a second model of it.

`app/` holds the board, the runs, and the importer that writes both. It is a deployment of the `skene` frame, which
supplies the address, the record model, the panels, and the router.

## How they relate

`app/tools/import-volume.mjs` is the only path between them. It runs `keys`, `box`, `read`, `verify`, `show`,
`sidecar`, and `self-check` against a volume, keeps each exit code, and writes `app/projects.json` and one run under
`app/scenario/` from what the tool printed.

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
- A record can be read whole. The `esg` reading lists one entry per record the `show` verb printed, and a run's tick
  holds the keys, the record count, and the kinds the volume carries.

Every address is drawn in its base-36 rendering, three characters an axis, so the board and the runs carry no native
character and a printed address can be handed back to the tool unchanged.

The run the board draws is written in the same vocabulary the model defines: facts for the reads it took, hints for
what the tool reported, and one intent that is claimed and then concluded. The screen therefore shows two things at once,
the address of a record and the transition that asked the question about it. Storage is the surface being read. The
structure on show is the record of the work over it.

## The ESG scenario

The `esg-scenario` project reads `esg-volume/` as a chain. The volume holds eight records for one site and one
quarter. Three are measured quantities; two are the intents that transform them, claimed and then concluded by the
reporting controller; two are the facts each intent filed as its result, so a conclusion is a record of its own rather
than a column on the intent; and one is the disclosure requirement the figures answer. The `esg` reading draws them in
the order the medium recorded them, one row per record, so a reader follows measured value, transformation, result, and
requirement down one screen.

Two limits are part of the demonstration rather than a footnote to it.

- The figures are synthetic. One site, one quarter, and one requirement were written to show the shape of a chain, and
  the emission factors in the intents are illustrative. Nothing here is a statement about any organisation's emissions.
- The claim is bounded. The demonstration shows that a fact, the transformation applied to it, and the requirement it
  answers are one addressable chain whose records can be read back and recomputed. Whether an organisation's reporting
  satisfies a regulation is a question about that organisation's governance and process, and this directory answers a
  narrower one about the evidence a chain carries.

Because the deployment is served over the web, it is read-only: the volume is committed, the board is generated ahead
of time, and no screen writes. The header carries `read-only` beside the version so a reader knows the medium they are
looking at before they look for a control that is not there.

### Regenerating the ESG volume

The volume is authored by a test that is ignored by default, because it writes bytes rather than checking them. It
builds no record by hand: it calls the same construction path the crate's own tests use, then reads the result back with
the importer.

```bash
cargo test -p nexus-storage-ref --test esg_scenario -- --ignored --nocapture
```

After it writes `esg-volume/`, regenerate the board entry from the application directory with the same command the
committed board was written with.

```bash
cd storage/ref/demo/app
npm run import -- --reset --id esg-scenario \
  --title "ESG compliance - Alpine Precision AG, 2026-Q1" \
  --volume storage/ref/demo/esg-volume --reading esg
npm run import -- --id sample-volume --title "Sample volume"
```

The first command starts a fresh board and the second restores the sample project beside it, because a reset drops
every project the board held. The end-to-end test performs both and compares the bytes, so the committed board and the
runs cannot drift from the volumes.

## What the demo does not show

- A write path. Both volumes are committed, so the demonstration reads and never writes.
- Performance. The demo carries no timing and no comparison, so no number on the screen is a performance claim.
- A regulatory determination. The ESG scenario shows a chain of evidence, and it does not audit an organisation.
- The layers a product adds above the read surface. The crate README's table separates those from what is shown here.

## Running it

### What you need

| Requirement | Why |
|---|---|
| A Rust toolchain | the importer runs the built `nexus-storage-ref` binary |
| Node 20 or newer | the frame declares it, and Vite 6 wants it |
| A network connection on the first run | the frame is cloned beside this checkout when it is absent. The application's manifest names the frame by a relative path, so it is installed from the checkout rather than from a registry |

Nothing else is installed globally. The volume is committed, so there is no data to prepare.

### The steps

All commands start at the repository root. `npm run demo` performs them in this order.

```bash
# 1. Build the reference tool. The importer runs this binary.
cargo build -p nexus-storage-ref

# 2. Put the frame beside this repository. Skip this when it is already there.
git clone --depth 1 https://github.com/ssccsorg/skene ../skene

# 3. Install the application. This reads the frame out of the sibling checkout.
cd storage/ref/demo/app
npm install

# 4. Regenerate the board and the runs from the committed volumes.
npm run import -- --reset --id esg-scenario \
  --title "ESG compliance - Alpine Precision AG, 2026-Q1" \
  --volume storage/ref/demo/esg-volume --reading esg
npm run import -- --id sample-volume --title "Sample volume"

# 5. Serve it.
npm run dev
```

The last command prints the address it serves on, which is `http://localhost:5292`.

A committed board and run are already in the tree, so a first look can skip step 4 and run step 3 followed by step 5.
Run step 4 whenever you want the screen to reflect a volume you changed, or to see the tool run for yourself.

### What each step does

- The build produces `target/debug/nexus-storage-ref`. The importer looks for the debug build first and the release build
  second, and never builds anything itself.
- The clone has to be a sibling of this repository, because `app/package.json` names the frame as
  `file:../../../../../skene/packages/frame`. Five levels up from `app` is this repository's parent.
- `npm install` installs the frame from that path into `node_modules`, which is why the clone comes before it.
- `npm run import` runs `keys`, `box`, `read`, `verify`, `show`, `sidecar`, and `self-check` against a volume and
  overwrites `app/projects.json` and that volume's run with what the tool printed. The first import starts a fresh
  board, and each later one adds its project to it.
- `npm run dev` serves the application with the board injected. The page opens on the reading each project names.

The one-line command does all five on any of the three platforms. It clones only when `../skene/packages/frame` is
missing, and installs only when `node_modules` is missing, so a second run goes straight to the import and the server.

### Checking it without a browser

In the application directory:

```bash
npm test
```

From the repository root:

```bash
cargo test -p nexus-storage-ref
```

`npm test` regenerates both runs in a temporary directory and compares them byte for byte with the committed ones, so
a stale run is a test failure rather than a screen that tells a story the tool no longer tells. The end-to-end case
skips when no binary is present and says so, so build the tool first if you want it to run.

To look at the production bundle instead of the dev server, still in the application directory:

```bash
npm run build
npm run preview   # http://localhost:5293
```

### If it does not start

| Symptom | Cause and fix |
|---|---|
| `npm install` fails while unpacking a tarball | the lock file names a frame path from before a move. Delete `node_modules` and `package-lock.json`, then install again |
| `npm install` cannot resolve the frame | the `skene` checkout is not a sibling of this repository, or is missing `packages/frame` |
| the importer reports it could not run the tool | no binary at `target/{debug,release}/nexus-storage-ref`. Build it, or name one with `--bin` or `NEX_REF_BIN` |
| the board is empty | `projects.json` is missing or holds no project. Run `npm run import` |
| the dev server refuses to start on 5292 | the port is strict, so another process holds it. Stop that process rather than changing the port, which the tests do not expect |

### Importer options

The importer has no `--help`, so this is the list.

| Option | Default | What it does |
|---|---|---|
| `--id` | the volume's directory name | the project's id, and the run's file name |
| `--title` | the id | the heading the run carries |
| `--volume` | `storage/ref/demo/volume` | the volume to read, relative to the repository root |
| `--reading` | `keyspace` | the reading the project opens on |
| `--out` | the application directory | where the board and the run are written |
| `--bin` | the debug build, then the release build | the tool to run |
| `--reset` | off | start a fresh board instead of adding this project to the existing one |

`app/README.md` covers the application's layout, its tests, and the frame it deploys.
