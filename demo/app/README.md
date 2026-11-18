# nexus-refstorage-demo

The storage reference, drawn on the shared frame. One board, one run per volume, and every number on the screen came
from `nex-refstorage`.

The application is a Vite deployment of `@ssccsorg/skene-frame`. It supplies a brand, a host, a board, and one pack;
the frame supplies the address, the record model, the panels, and the router. The host says the board and its runs are
the files this bundle shipped with, so the frame asks no room and follows no feed.

## Running it

The prerequisites, the first run, and the troubleshooting table live in `../README.md`, which is the demonstration's
entry point. `./run.sh` here, or `npm run demo`, performs the whole first run in one step.

| Script | What it does |
|---|---|
| `./run.sh` | the whole first run in a shell: build, fetch the frame, install, board, serve. `--fresh` reinstalls first |
| `npm run demo` | the whole first run: build, fetch the frame, install, import, serve |
| `npm run import` | runs the reference tool over a volume and writes the board and the run |
| `npm run dev` | serves the application on 5292, with the board injected |
| `npm run build` | typechecks, then writes a production bundle carrying the board |
| `npm run preview` | serves that bundle on 5293 |
| `npm test` | the board, the brand, and the importer |
| `npm run typecheck` | `tsc --noEmit` alone |

`npm run import` runs `keys`, `box`, `read`, `verify`, `show`, `sidecar`, and `self-check` against the volume and writes
`projects.json` and `scenario/<id>.txt`. A path given to `--volume` is read relative to the repository root, so the
command works from this directory and the board stays machine independent. `--reading` names the reading a project
opens on, which is `esg` unless a volume is better read as a chain.

Without a built binary the importer reports what it could not run. `NEX_REF_BIN` names one elsewhere.

## Layout

| | |
|---|---|
| `projects.json` | the board, written by the importer |
| `scenario/<id>.txt` | one run per volume, written by the importer |
| `src/pack.tsx` | the `volume` pack: the `esg` resource kind and its reading |
| `src/chain.tsx` | the `chain` reading: the network the records' own links make |
| `src/evidence.tsx` | the `evidence` panel: the picked record, its links, and the field a branch edits |
| `src/workbench.ts` | the pick and the branch, which are the demonstration's own state and not the volume's |
| `src/brand.tsx` | the mark, the name, the version, the note beside them, the lines the card opens, and the layers |
| `src/host.ts` | the host: no shell, no room, no feed, because the board and its runs are files |
| `src/narrow.tsx` | the notice a window too narrow for the three columns reads |
| `public/_headers` | what the deployment allows the browser, which is no device at all |
| `tools/import-volume.mjs` | runs the reference tool and writes the board and the run |
| `tools/sync-project.mjs` | copies one project and its run between two boards, in either direction |
| `tools/run-demo.mjs` | the first run in one command: build, fetch the frame, install, import, serve |
| `tools/frame.mjs` | leaves the sibling skene checkout on skene's main and prints the commit |
| `tests/` | the board, the mark, the importer, and the reading each project opens on |

## Tests

```bash
npm test        # vitest, node environment
npm run build   # typecheck, then a production bundle carrying the board
```

The end-to-end case in `tests/import.test.ts` runs the importer against both committed volumes and compares the bytes
with the ones committed. It is skipped when no `nex-refstorage` binary is present, and it says so rather than passing
on nothing.
