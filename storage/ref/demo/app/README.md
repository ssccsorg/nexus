# nexus-storage-ref-demo

The storage reference, drawn on the shared frame. One board, one run per volume, and every number on the screen came
from `nexus-storage-ref`.

The application is a Vite deployment of `@ssccsorg/skene-frame`. It draws nothing of the frame: it supplies a brand, a
board, and one pack, and the frame supplies the address, the record model, the panels, and the router.

It lives inside the crate it demonstrates, beside the volume it reads, because it is the reference's own demonstration
rather than one more application on the shelf.

## What it shows

`nexus-storage-ref` makes five claims, and each one has a place on the board.

| Claim | Where it is drawn |
|---|---|
| an address is a key, and it resolves by a rule | the `keyspace` reading lists one row per key, grouped by the prefix the key carries |
| a point read is one read of the medium | a fact for the record the `read` verb reached, with the byte count it printed |
| a record's address follows from what it holds | a hint for the address the `verify` verb recomputed, with the fields it used |
| a listing is the only scan, so a box is answered from it | a hint for the axes the box narrowed and the records it admitted |
| a volume sits beside an application file and leaves it alone | a hint for the size the `sidecar` verb measured |

The board's own clock is the tool's order, and each run's `count` steps are the parser's arithmetic over the file's own
facts. A run edited by hand has to keep its header and its counts in step or the tests fail.

## Running it

The prerequisites, the step by step first run, and the troubleshooting table live in `../README.md`, which is the
demonstration's entry point. `npm run demo` there performs the whole first run in one step. The scripts this application
offers:

| Script | What it does |
|---|---|
| `npm run demo` | the whole first run: build, frame, install, import, serve |
| `npm run import` | runs the reference tool over a volume and writes the board and the run |
| `npm run dev` | serves the application on 5292, with the board injected |
| `npm run build` | typechecks, then writes a production bundle carrying the board |
| `npm run preview` | serves that bundle on 5293 |
| `npm test` | the board, the brand, and the importer |
| `npm run typecheck` | `tsc --noEmit` alone |

`npm run import` runs `keys`, `box`, `read`, `verify`, `sidecar`, and `self-check` against the volume and writes
`projects.json` and `scenario/<id>.txt`. Every verb prints the address in the identity's own base-36 text, so the board
and the run carry that form and nothing outside ASCII. It counts nothing and evaluates nothing: a second model of the
store would be a model free to disagree with the store. A path given to `--volume` is read relative to the repository
root, so the command works from this directory and the board stays machine independent.

Without a built binary the importer reports what it could not run. `NEX_REF_BIN` names one elsewhere.

## Layout

| | |
|---|---|
| `projects.json` | the board, written by the importer |
| `scenario/<id>.txt` | one run per volume, written by the importer |
| `src/pack.tsx` | the `volume` pack: the `keyspace` resource kind and its reading |
| `src/brand.tsx` | the mark, the name, the version, and the layers |
| `tools/import-volume.mjs` | runs the reference tool and writes the board and the run |
| `tools/run-demo.mjs` | the first run in one command: build, frame, install, import, serve |
| `tests/` | the board, the mark, and the importer |

## Tests

```bash
npm test        # vitest, node environment
npm run build   # typecheck, then a production bundle carrying the board
```

The end-to-end case in `tests/import.test.ts` runs the importer against the committed volume and compares the bytes with
the ones committed. It is skipped when no `nexus-storage-ref` binary is present, and it says so rather than passing on
nothing.

## The boundary

The key space is the storage layer's own shape, so it arrives as a pack. Nothing in this application names a commercial
product, and the frame learns no storage vocabulary: a key space, a prefix, an axis range, and an exit code are the
demo's, and each one is a resource, a reading, or a run line.
