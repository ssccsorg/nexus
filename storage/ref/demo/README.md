# The demonstration

Three directories, one purpose: to show what the reference crate claims rather than state it.

`nexus-storage-ref` reads a volume and prints. This directory puts a screen on the same reads, so a developer or a
technical auditor can see the key space, a point read, and a verification without reading the crate first.

## Run it in one line

In a shell, from the repository root:

```bash
storage/ref/demo/app/run.sh
```

That builds the tool, puts the frame beside the checkout when it is absent, regenerates the board from the committed
volumes, and serves it on the address Vite prints. `--fresh` reinstalls from the lock file first, which is what heals a
frame that changed under the copy npm made of it earlier.

The same work without a POSIX shell:

```bash
npm --prefix storage/ref/demo/app run demo
```

The steps below are the same work one at a time, for a first run you want to watch.

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
done to them, and hints that name the disclosure requirements. It is committed, and it is read the same way, so the
chain it holds is the medium's answer and not a second model of it.

`app/` holds the board, the runs, and the importer that writes both. It is a deployment of the `skene` frame, which
supplies the address, the record model, the panels, and the router.

## How they relate

`app/tools/import-volume.mjs` is the only path between them. It runs `keys`, `box`, `read`, `verify`, `show`,
`sidecar`, and `self-check` against a volume, keeps each exit code, and writes `app/projects.json` and one run under
`app/scenario/` from what the tool printed.

The board holds one project per committed volume, and `app/tools/run-demo.mjs` regenerates it whole on every launch,
with `--reset` on the first volume, so a board written by an earlier run is replaced rather than appended to. An append
cannot restore a project that run never had.

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
  holds the keys, the record count, and the kinds the volume carries. Each fact whose address is its own is put to the
  `verify` verb, and the reading states how many held. The content a record points at is resolved by the same verb: a
  fact's payload, and the transformation an intent declares, so a caller reads an intent without decoding the medium.
- A chain is the links its records carry. The `chain` reading draws the facts, the intents and the hints as one network
  and names the place each record takes in it.
- A place is a declaration and a record is a reading. The `esg` project declares its kinds and its ground, so the map
  reading draws the machines the volume holds facts from, at the positions the declaration gives them.

Every address is drawn in its base-36 rendering, three characters an axis, so the board and the runs carry no native
character and a printed address can be handed back to the tool unchanged.

The run the board draws is written in the same vocabulary the model defines: facts for the reads it took, hints for
what the tool reported, and one intent that is claimed and then concluded. The screen therefore shows two things at once,
the address of a record and the transition that asked the question about it. Storage is the surface being read. The
structure on show is the record of the work over it.

## The ESG scenario

The `esg-scenario` project reads `esg-volume/` as a chain. The volume holds twenty-six records for one site and one
quarter, shaped the way a quarterly inventory is. Seventeen are measured quantities: electricity, natural gas, a
refrigerant top-up, the quarter's output, solar generation, water withdrawal and discharge, metal scrap, cooling,
compressed air, process heat, forklift charging, press cycles, crane lift hours, air handling, waste to landfill, and
inbound freight. Three are the intents that transform four of them, each claimed and then concluded by the reporting
controller: Scope 2 on both a location-based and a market-based basis, Scope 1 split into combustion and fugitive
refrigerant, and the intensity per assembly over the two results. Three are the facts each intent filed as its result,
so a conclusion is a record of its own rather than a column on the intent. Three are the disclosure requirements the
figures answer: E1-6 for the emissions, E3-4 for the water, E5-5 for the waste.

The thirteen readings no transformation reads are the point of having them. A quarter's inventory holds quantities that
no calculation picked up, and a screen that drew only the part of the plant that was used would be an inventory that
looked settled.

What the project calls those records is its own. `fact`, `intent` and `hint` are the engine's names for the three things
a run is made of, and an auditor reads the same three as evidence, a calculation, and the requirement the work answers.
The project says so in its bundle and the frame draws it, so no screen on this project's board spells the engine's model
at a reader: the mark takes a letter of the project's word and the word travels as the title. The colour stays the
kind's, because which ink a kind is drawn in is the frame's and two boards that coloured one kind two ways would be two
boards.

The records carry what a reported figure has to be able to answer for: a reference for the meter, work order, or
production system; a quality flag; the consolidation approach, the base year, and whether the figure was restated; the
factor and its source; and, for Scope 2, both bases. The `esg` reading draws them in the order the medium recorded them,
one row per record, so a reader follows measured value, transformation, result, and requirement down one screen.

### The chain the records make

The `chain` reading draws the same records as the network their links make, in four columns: the requirements, the
observed records, the transformations, and the results they concluded.

Every edge is a field of a record, so the drawing cannot claim more than the volume does.

| Edge | Read from | Says |
|---|---|---|
| evidence | an intent's `from_facts` | this source was read by this transformation |
| conclusion | an intent's `Concluded.to_fact` | this transformation filed this result |
| bounds | the `requirement` a result states, matched to the code a hint is titled with | the work answered this requirement |

The third is a derivation over two records and worth stating: a conclusion names the requirement it answers, and the
work that reached it is the work that requirement bounded. A requirement no conclusion cites draws no edge and stands
alone, which is what E3-4 and E5-5 are in the committed volume: on file, and answered by nothing. The drawing says so
rather than leaving the gap to be noticed.

A node is titled by a field its record holds: a measured record by the instrument that reported it, a transformation
by the work it declares, a result by the quantity it states, and a requirement by the code it is cited by. The address
travels as the tooltip, because a column of addresses is the panel this one replaces.

A record no link touches is drawn and dimmed rather than left out, and the header counts them: thirteen of the
seventeen readings feed no calculation. An audit's first finding is often the reading nobody picked up, so the graph
says it instead of drawing a plant that looks fully accounted for.

### The pick and the branch

A press is one pick, read from both sides. Pressing a record in the chain or in the list opens it in the frame's
`evidence` panel; pressing a machine on the map focuses the records that machine wrote, in both readings, and scrolls
the first into view. The two sides are the same choice, so the map and the list cannot point at different things.

The panel is a column of the frame and not a card over the space. It draws the record's fields, the links it carries,
and, for a transformation or a requirement, the one field a reader may change.

A change is a branch rather than a rewrite. `take the branch` holds the edited text beside the volume, and the chain is
read against it from that record on.

- A transformation's new text replaces the work it declares, and the result it concluded is drawn as `of the branch`.
- A requirement's new text is read the way the edges are read, so a code no conclusion cites reaches no transformation
  and the bounds edges disappear. Editing the text after the colon keeps the code and keeps the edges.

What a branch does not do is recompute. The figures are the reporting controller's and the volume holds its answer, so a
branch changes what the chain is read against and says which records rest on it. `revert` gives the volume back, and a
reload forgets the branch because it was never written.

The project opens on the map, with the chain and the `esg` reading beside it in the bar. Those are the three readings
this project stands: the frame's readings of the raw run are not named by it, because a record's byte count and a
member's board coordinate are the tool's own fields and not what the pack is about. The bar opens any of them for a
reader who wants them.

The frame's own panels are the frame's arrangement. The board opens with the declarations, the space column, the
transport and the evidence panel out, and the console away, which is what each panel answers for itself. This project
declares no arrangement of its own. A declaration is applied when the project opens, over whatever the pilot left, so a
panel a bundle closed would come back on every visit: the project says nothing, and the arrangement that stands is the
one the pilot arranged.

The same project declares a place, so the map draws the site the readings come from. Sixteen instruments stand on it,
of eight kinds: meters, a weighbridge, an inverter, a charger, a crane, a press, the boiler house, the machine hall's
air handling, the loading bay, and the service point. The controller and the requirements do not stand on it, because a
record and a bound are not things on the ground. That distinction is the frame's, not this demonstration's: a member is
drawn on a map when its kind presents a location, and the project is what says which kinds do.

The boundary is drawn as a dashed box over the ground, from the box the place itself declares, so it cannot be a fence
around ground the units do not stand on. The site's name and each unit's name are read from the records: the label is
the `site` the measured facts carry, and a machine is called by the metric and reference of the record its creator
wrote.

What the map does not carry is as deliberate as what it does. A pin is a declared position and not a reading from a
device: the records hold no coordinate, so no pin is evidence that an instrument is where the sketch puts it. Nothing
here is signed by hardware. What the tool does assert about a record is narrower and checkable: its address is
recomputed from the origin, the creator, and the payload, and the record reading states how many held.

It also declares a campaign, which the frame draws under the board: the reporting close, read over the run. Its
objectives are the artefacts a close is made of (measurements, Scope 1, Scope 2 on both bases, the intensity, the
requirement), and each one is a mark the importer writes from the records' own fields. A volume that states Scope 2 on
one basis writes no mark for both, so the close shows the gap instead of a green line. Scrubbing the timeline back
leaves the later artefacts open, because the campaign is a reading of the record at a cursor.

Two limits are part of the demonstration rather than a footnote to it.

- The figures are synthetic. One site, one quarter, and one requirement were written to show the shape of a chain, and
  the emission factors in the intents are illustrative. The site is schematic and its ground is an illustrative box of
  coordinates, so the map is a sketch of where a plant could stand and not a claim about where one does.
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
  --volume storage/ref/demo/esg-volume --reading map
npm run import -- --id sample-volume --title "Sample volume"
```

The first command starts a fresh board and the second restores the sample project beside it, because a reset drops
every project the board held. The end-to-end test performs both and compares the bytes, so the committed board and the
runs cannot drift from the volumes.

## What the demo does not show

- A write path. Both volumes are committed, so the demonstration reads and never writes. A branch is held in the page
  and is gone when it is reloaded.
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
  --volume storage/ref/demo/esg-volume --reading map
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

From the repository root, one command:

```bash
./run.sh --demo
```

It builds the tool, puts the frame beside the checkout when it is absent, installs the application with `npm ci`, runs
the application's tests, builds the bundle, and checks that the bundle carries the board. Then it serves the board on
`http://localhost:5292`, so the command a reader runs to check the thing is the command that shows it.

To run the check and stop, which is what CI does:

```bash
./run.sh --demo-check
```

In the application directory the steps are the same, one at a time:

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
