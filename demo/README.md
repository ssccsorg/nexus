# The demonstration

A board over the reference crate's own reads: one point read, one verification, one ESG reporting quarter read as a
chain, and two projects taken from another board. Drawn by the `skene` frame, from the volumes committed here, and
served read-only.

Everything on it is synthetic. No real company, meter, or filing, and no line on it is a compliance determination.

## Run it

From the repository root, with a Rust toolchain and Node 20 or newer:

```bash
./run.sh --demo          # build the tool, check the demonstration, then serve it on 5292
./run.sh --demo-serve    # serve the committed board, no check
./run.sh --demo-check    # the check alone, which is what CI runs
./run.sh --demo-deploy   # publish the built board with a local wrangler login
```

`demo/app/run.sh` performs the first one on a POSIX shell, and `--fresh` makes it reinstall from the lock
file. `npm --prefix demo/app run demo` is the same run without a shell, which is the form Windows uses.

With nothing installed on the host:

```bash
docker build -t nexus-demo demo
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

The last two are copies of `shared/`, and `app/tools/sync-project.mjs` moves a project in either direction between two
boards. `app/tools/import-volume.mjs` is the only path between the volumes and the screen: it runs `keys`, `box`,
`read`, `verify`, `show`, `sidecar`, and `self-check`, and writes `app/projects.json` and one run from what the tool
printed. Every number on the board is the tool's output.

The board reads as a run down the left, a chain across the middle, the picked record's panel beside it, and a map of
the site. A press picks a record; `take the branch` reads the chain against edited text and leaves the volume alone, so
a reload removes it. The words for the three kinds are the project's, not a standard's.

## Regenerating it

The board and the runs are committed, so a first look needs none of this.

```bash
# the volume, from the scenario data
cargo test -p nex-refstorage --test esg_scenario -- --ignored --nocapture

# the board and the runs, from the volumes
demo/app/run.sh
```

`app/README.md` covers the importer on its own, which writes the board without starting a server.

A change to the site, a reading, or a requirement text goes in `esg.scenario.json`. The words the board uses for the
three kinds are the `words` a project declares in `app/tools/import-volume.mjs`. `app/projects.json` and `app/scenario/`
are generated, so an edit there is undone by the next import.

## What it does not show

A write path, a timing, or a regulatory determination. Both volumes are committed, nothing on the screen is a
measurement of speed, and it shows the evidence a chain carries rather than whether a filing satisfies a regulation.
