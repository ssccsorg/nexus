# nexus-storage-ref-demo

The storage reference, drawn on the shared frame. One board, one run per volume, and every number on the screen came
from `nexus-storage-ref`.

The application is a Vite deployment of `@ssccsorg/skene-frame`. It draws nothing of the frame: it supplies a brand, a
board, and one pack, and the frame supplies the address, the record model, the panels, and the router.

It lives inside the crate it demonstrates, beside the volume it reads, because it is the reference's own demonstration
rather than one more application on the shelf.

## What it shows

`nexus-storage-ref` makes six claims, and each one has a place on the board.

| Claim | Where it is drawn |
|---|---|
| an address is a key, and it resolves by a rule | the `keyspace` reading lists one row per key, grouped by the prefix the key carries |
| a point read is one read of the medium | a fact for the record the `read` verb reached, with the byte count it printed |
| a record's address follows from what it holds | a hint for the address the `verify` verb recomputed, with the fields it used |
| a listing is the only scan, so a box is answered from it | a hint for the axes the box narrowed and the records it admitted |
| a volume sits beside an application file and leaves it alone | a hint for the size the `sidecar` verb measured |
| a record can be read whole | the `esg` reading lists one row per record the `show` verb printed |
| a chain is the links its records carry | the `chain` reading draws the facts, the intents and the hints as one network |
| a press is one pick | a record pressed in the chain or the list opens in the `evidence` panel, and a machine pressed on the map focuses its records |
| a declaration can be branched | the panel's field takes a branch, and the chain draws what rests on it |

The board's own clock is the tool's order, and each run's `count` steps are the parser's arithmetic over the file's own
facts. A run edited by hand has to keep its header and its counts in step or the tests fail.

## Running it

The prerequisites, the step by step first run, and the troubleshooting table live in `../README.md`, which is the
demonstration's entry point. `./run.sh` here, or `npm run demo` in `../README.md`, performs the whole first run in one
step. The scripts this application offers:

| Script | What it does |
|---|---|
| `./run.sh` | the whole first run in a shell: build, frame, install when absent, board, serve. `--fresh` reinstalls first |
| `npm run demo` | the whole first run: build, frame, install, import, serve |
| `npm run import` | runs the reference tool over a volume and writes the board and the run |
| `npm run dev` | serves the application on 5292, with the board injected |
| `npm run build` | typechecks, then writes a production bundle carrying the board |
| `npm run preview` | serves that bundle on 5293 |
| `npm test` | the board, the brand, and the importer |
| `npm run typecheck` | `tsc --noEmit` alone |

`npm run import` runs `keys`, `box`, `read`, `verify`, `show`, `sidecar`, and `self-check` against the volume and
writes `projects.json` and `scenario/<id>.txt`. Every verb prints the address in the identity's own base-36 text, so the
board and the run carry that form and nothing outside ASCII. It counts nothing and evaluates nothing: a second model of
the store would be a model free to disagree with the store. A path given to `--volume` is read relative to the repository
root, so the command works from this directory and the board stays machine independent. `--reading` names the reading a
project opens on, which is `keyspace` unless a volume is better read as a chain.

Without a built binary the importer reports what it could not run. `NEX_REF_BIN` names one elsewhere.

## Layout

| | |
|---|---|
| `projects.json` | the board, written by the importer |
| `scenario/<id>.txt` | one run per volume, written by the importer |
| `src/pack.tsx` | the `volume` pack: the `keyspace` and `esg` resource kinds and their readings |
| `src/chain.tsx` | the `chain` reading: the network the records' own links make |
| `src/evidence.tsx` | the `evidence` panel: the picked record, its links, and the field a branch edits |
| `src/workbench.ts` | the pick and the branch, which are the demonstration's own state and not the volume's |
| `src/brand.tsx` | the mark, the name, the version, the note beside them, the lines the card opens, and the layers |
| `tools/import-volume.mjs` | runs the reference tool and writes the board and the run |
| `tools/run-demo.mjs` | the first run in one command: build, frame, install, import, serve |
| `tests/` | the board, the mark, and the importer |

## Tests

```bash
npm test        # vitest, node environment
npm run build   # typecheck, then a production bundle carrying the board
```

The end-to-end case in `tests/import.test.ts` runs the importer against both committed volumes and compares the bytes
with the ones committed. It is skipped when no `nexus-storage-ref` binary is present, and it says so rather than passing
on nothing.

## The boundary

The key space is the storage layer's own shape, so it arrives as a pack. Nothing in this application names a commercial
product, and the frame learns no storage vocabulary: a key space, a prefix, an axis range, and an exit code are the
demo's, and each one is a resource, a reading, or a run line.

A record's kind is a term of the format, so its mark is the frame's: `KindTag` draws one colour and one letter, and this
application imports it rather than choosing either. The word beside the mark is the application's own reading of the
chain, because what a record does in a chain is a question about these records and not about the format: an intent
concluded a fact, so that fact is a result rather than an observation. The two sit side by side because they answer
different questions, and the word is derived from the links the records carry rather than from a list kept here.

A map is the frame's reading of two resources, and both are declared here because both are about this project rather
than about the volume: `kinds` says which participant kinds present a location, and `place` says where the ground is and
where each member stands on it. The records on the map are still the tool's; the importer writes the declaration beside
them and the frame draws it.

The deployment installs one view, `@ssccsorg/skene-view-map`. A view is installed by importing its package, so a package
that is not imported is a surface the board does not offer: this board has a map and no shell, no graph, and no footage,
and those packages are not its dependencies. The rule runs the other way for readings too. A project names the readings
it stands, and one whose host surface is absent, such as a map over a volume that declares no place, is not named at all.

The evidence panel is held by the same rule rather than by a check of its own. It reads the links a record carries, so it
is a surface of a project that holds a chain, and it says so with the frame's capability seam: the panel declares
`needs: 'evidence'` and the importer writes that capability for a volume that holds work as well as readings. The
`sample-volume` project holds one observation, declares nothing, and stands no panel, which is why a surface this demo
offers is not a surface every project in the board has.

The chain is this pack's reading and not the frame's graph. A graph of a run is a claim about a run, and the frame's own
draws the participants and the work passed between them, which is the shape a run of many machines has. What an audit
reads is the records and the links they carry, and those are this volume's terms, so the reading lives here beside the
records it draws. The layer is the same one the map's `kinds` and `place` sit on: the frame supplies the surface and the
mark, and the deployment supplies what is true of its own project.
