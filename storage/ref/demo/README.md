# The demonstration

A board over the reference crate's own reads: one point read, one verification, one ESG reporting quarter read as a
chain, and two projects taken from another board. Drawn by the `skene` frame, from the volumes committed here, and
served read-only at [rf-storage-demo.nexus.ssccs.org](https://rf-storage-demo.nexus.ssccs.org).

Everything on it is synthetic. No real company, meter, or filing, and no line on it is a compliance determination.

## Run it

From the repository root, with a Rust toolchain and Node 20 or newer:

```bash
./run.sh --demo          # build the tool, check the demonstration, then serve it on 5292
./run.sh --demo-serve    # serve the committed board, no check
./run.sh --demo-check    # the check alone, which is what CI runs
./run.sh --demo-deploy   # publish the built board with a local wrangler login
```

`storage/ref/demo/app/run.sh` performs the first one on a POSIX shell, and `--fresh` makes it reinstall from the lock
file, which is how a frame that changed under the copy npm made of it is healed. `npm --prefix storage/ref/demo/app run
demo` is the same run without a shell, which is the form Windows uses.

With nothing installed on the host:

```bash
docker build -t nexus-demo storage/ref/demo
docker run --rm -p 8080:8080 nexus-demo
```

The image serves the committed board. It carries no Rust toolchain, no volumes, and no write path.

## What is in it

| Path | What it holds |
|---|---|
| `volume/` | one record as the format writes it |
| `esg-volume/` | one reporting quarter, read as a chain |
| `esg.scenario.json` | the site, the readings, the figures, and the requirement texts the volume is written from |
| `app/` | the Vite application: the board, the runs, and the importer |
| `shared/` | two projects kept as a board of their own |

| Project | What it is |
|---|---|
| `esg-scenario` | one reporting quarter of a fictional manufacturer; opens on the chain |
| `sample-volume` | the committed record, read by the tool; opens on the records table |
| `villa` | a rescue, from another board |
| `spiderwing` | a patrol and the forty-seven seconds it cannot account for |

The last two are not volumes. They are copied from `shared/`, which keeps them as a board of their own so the copy is a
copy between equals, and `app/tools/sync-project.mjs` moves a project in either direction, leaving what the destination
already holds alone unless `--replace` names the side that wins. They share no subject with the ESG quarter, and that is
the point: the same records and the same frame carry another domain without a change on either side.

`app/tools/import-volume.mjs` is the only path between the volumes and the screen. It runs `keys`, `box`, `read`,
`verify`, `show`, `sidecar`, and `self-check`, and writes `app/projects.json` and one run from what the tool printed. The
board is therefore the tool's output, and no number on it is the screen's own.

## What the screen shows

- The left rail is the run that produced the board: the tool's checks and what each answered.
- The chain reads left to right as a measurement, the calculation it fed, the result that calculation concluded, and the
  requirement that bounds it. A record nothing read is drawn and dimmed, because an inventory that no calculation picked
  up is a finding rather than an omission.
- The lists use the project's words for the three kinds: evidence, calculation, and requirement. Those are this board's
  words, not a standard's.
- A press picks a record. The panel says what it holds, what it cites, what cites it, and the one field a branch may edit.
- `take the branch` reads the chain against edited text and leaves the volume alone. A reload removes it, because it was
  never written.
- The map draws the site the readings come from, from the place the project declares. A pin is a declared position and
  not a device reading, so nothing here is signed by hardware.

## Regenerating it

The board and the runs are committed, so a first look needs none of this.

```bash
# the volume, from the scenario data
cargo test -p nexus-storage-ref --test esg_scenario -- --ignored --nocapture

# the board and the runs, from the volumes
storage/ref/demo/app/run.sh
```

`app/README.md` covers the importer on its own, which writes the board without starting a server.

The gate runs the same writer against a temporary volume and fails when the committed bytes differ:

```bash
cargo test -p nexus-storage-ref --test esg_scenario
```

A change to the site, a reading, or a requirement text goes in `esg.scenario.json`. The words the board uses for the
three kinds are the `words` a project declares in `app/tools/import-volume.mjs`. Those two files are where a review
starts. `app/projects.json` and `app/scenario/` are generated, so an edit there is undone by the next import.

## Deploying it

A push to `main` publishes the board once every job is green. The `demo` job builds it and uploads the artifact, and the
`deploy` job publishes that artifact, so the address serves the bytes the suite passed rather than a second build. A pull
request stops at the artifact.

`CF_PAGES_DEPLOY` (Cloudflare Pages, Edit) and `CF_ACCOUNT_ID` come from the secrets the organisation keeps, and a
missing one fails the job rather than publishing to a guessed account. The job also runs `--demo-install`, because the
upload bundles the Pages Functions, and their import of the frame resolves through the application's `node_modules`.

`public/_redirects` sends a request that names no file to the page, because `/chain`, `/map`, and `/esg` are readings and
not files. `public/_headers` denies camera, microphone, location, and the other device APIs, so the page asks a browser
for nothing.

## What it does not show

- A write path. Both volumes are committed, and a branch lives in the page.
- Performance. Nothing on the screen is a timing.
- A regulatory determination. It shows the evidence a chain carries, not whether a filing satisfies a regulation.

If the dev server refuses port 5292, another process holds it; the port is strict and the tests do not expect it to
move. If the board is empty, run the import `app/README.md` describes.
