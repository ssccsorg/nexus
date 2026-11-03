# Using skene as a library: handoff for a neXus storage demo

This report hands one task to the next agent: a visual demonstration of `nexus-storage-ref` built on the
`skene` frame, with the same relationship the verification space (`es`) already has to it. It records what
the frame now is, what a deployment supplies, how the existing consumer did it file by file, and what the
storage demo should show and test.

Two rules bind this work and the report itself.

- No commercial product name appears in the demo, its board, its run files, its brand, or its tests. The
  frame is open source and a deployment brings only its own domain. The frame's own source and tests carry
  none either, so there is no example to inherit by accident.
- The numbers a screen shows are the tool's own. A second model of the store inside the demo is a model
  free to disagree with the store.

## What skene is now

`skene` is one repository, one package, and one published surface.

| Item | Value |
|:---|:---|
| Repository | `ssccsorg/skene`, public, Apache-2.0 |
| Package | `@ssccsorg/skene-frame`, version `0.1.0`, `private: false` |
| Language | TypeScript source, never prebuilt; the consumer's bundler compiles it |
| Distribution | a sibling clone quoted in the consumer's manifest as a `file:` path |

It ships three contract entries and one build tool: the root module, `/ui`, `/tokens.css`, and
`/tools/vite-board.mjs`. The layers under them (`lib/*`, `stores/*`, `hooks/*`, `views/*`, `components/*`)
are exported on purpose for a deployment's tests and tools, and carry the same promise.

The root module is the whole stable API. It exports the frame, the brand, the host, the room, the
registers (`registerPack`, `registerReading`, `registerPanel`, `registerInstrument`, `registerResource`,
`registerRoom`), the board readers (`bundleOf`, `PROJECT_FORMAT`), the run reader (`parseScenario`, `titleOf`,
`actors`, `chainFor`, `entityOf`, `INTENT`), and two stores: `useScenario` and `useRecords`. Those two are
the only runtime reach past the contract, and they exist because adding a project to a live board is a
built-in feature rather than a pack's privilege.

## The contract in one page

A deployment supplies exactly three things.

| Thing | What it is |
|:---|:---|
| a brand | the name in the bar, the version, the layers it stands on, and the mark |
| a board | `projects.json` plus one run per project, read at run time |
| a pack | optionally, readings and resource kinds the frame has never heard of |

The frame supplies the address, the record model, the panels, the transport, the place, the native
readings, and the router. The frame knows facts, intents, and hints. A domain may rename the three in its
own bundle; it may not change what they mean.

A pack arrives as one declaration rather than as an edit to the frame:

```ts
import { registerPack, registerResource } from '@ssccsorg/skene-frame';

registerPack({ id: 'volume', readings: [{ key: 'keyspace', path: '/keyspace', View: KeySpace }] });
registerResource('keyspace', (at) => at as KeySpace);
```

The project then names the pack and the reading it opens on:

```json
{ "kind": "views", "at": { "pack": "volume", "main": "keyspace" } }
```

A pack module registers itself where it is imported, the same way the frame imports `./views` at the end
of `index.ts`. The deployment's `main.tsx` therefore imports its pack before it renders.

## The board document

```json
{
  "note": "one sentence a reader sees beside the projects",
  "projects": [
    {
      "format": "project/1",
      "id": "sample-volume",
      "resources": [
        { "kind": "channel", "at": "sim" },
        { "kind": "record", "at": "scenario/sample-volume.txt" },
        { "kind": "views", "at": { "pack": "volume", "main": "keyspace" } }
      ]
    }
  ]
}
```

`id` is lower case letters, digits, and dashes. `format` is optional; a document without one is read as
this build's own. A project's resources are read by kind, and the kinds this build reads are `channel`,
`kinds`, `sims`, `policy`, `sources`, `layout`, `views`, `record`, `status`, `clock`, `place`,
`campaigns`, and `project` (which nests another project). A kind with no reader is refused by name rather
than ignored, and every kind except `project` may be declared once.

`record` is required: `bundleOf` throws for a project that declares none, because a project with no run
has nothing to show. The path is fetched relative to the board.

`views.at` takes `pack` (a name or a list), `main` (the reading to open on), and `on` (an explicit list of
readings to stand). `layout` still works as the older spelling of the same question.

## The run document

The run is the engine's own text, parsed by `parseScenario`. The reader is deliberate about what it
models, and the corpus verbs it does not model are accepted rather than flagged.

```text
# The sample volume
seed 1
tick 0
position sample-volume 70,88
count 1

fact keyspace-listed 0 keys/sample
hint key-count facts=1,blobs=2,exit=0
count 2

intent read-sample keyspace-listed read-one-address
claim read-sample read/sample

tick 1
fact sample-record 128 read/sample
intent verify-sample sample-record recompute-the-address
claim verify-sample verify/sample
conclude verify-sample verified
count 4
```

| Verb | What it writes |
|:---|:---|
| `seed`, `tick`, `time` | the clock, and nothing else |
| `fact <name> <bytes> <origin>` | one fact, named so a later step can reach it |
| `position <unit> <x,y>`, `path <unit> <from> <to> <steps> <interval>` | facts whose origin is `<unit>/position`, drawn on the place |
| `hint <name> <content>` | one hint, which is the bound a judgement was made under |
| `intent <name> <from> <description>` | one intent, declared over the fact it starts from |
| `claim <name> <worker>` | a step, not a record: it puts a name on the timeline |
| `conclude <name> <label>` | the fact an intent produced |
| `write` / `try-write <count> <bytes> <origin> <interval>` | a block of facts, with the clock left on the last |
| `count <n \| ->` | the engine's own assertion about how many facts the run holds at that point |

`writerOf` takes a record's writer from the first segment of its origin, so `keys/sample` and
`read/sample` land on two rows while `nexus-storage-ref/keys` and `nexus-storage-ref/read` land on one.
Choose origins with that in mind: one row per tool verb is what makes a storage run readable.

The verbs the reader accepts and draws nothing for are `flush`, `query`, `list`, `read`, `read-fact`,
`heap`, `buffered`, `fail`, `axis`, `order`, `medium`, and `exit`. A run written in the corpus's own
language therefore reads without complaint, and the parts the board does not model stay visible in the
file. Anything else is collected as `unknown` by the parser and shown rather than dropped.

## The two ways a run reaches the surface

The static board is the precedent and the recommended first step.

- The board and `scenario/<id>.txt` are read at run time with `fetch`.
- `boardIntoBuild()` copies both into the build, so a production bundle carries the file the tool wrote.
- No server, no socket, no key. Deterministic, and gateable by a test.

The live volume is the second step and is already plumbed.

- A project declares `{ "kind": "channel", "at": "sim" }` or another room name.
- With `VITE_RECORDS=1`, the frame reads `/records` and expects `{ "records": WireRecord[], "total": n }`.
- `WireRecord` is `{ seq, kind, at, origin?, bytes?, cycle?, label?, content?, description?, worker?,
  fromLabel?, producedLabel?, producedByLabel? }`; links are resolved by the frame against the records
  already received, so a record may name a label that has not arrived.
- A stream is also available at `ws://<host>/stream?project=<id>&pace=<n>&at=<n>`, overridable by
  `VITE_STREAM_URL` or `host.endpoints.stream`. `at` freezes a frame at a record count, which is what
  makes a screenshot reproducible.

## The worked precedent

The verification space (`es`) is one application directory over the frame, and its shape is the template.

```text
es/app/
  package.json          "@ssccsorg/skene-frame": "file:../../skene/packages/frame"
  .npmrc                install-links=true
  index.html
  vite.config.ts        boardIntoBuild(), dedupe, maplibre excluded, port 5280
  projects.json         one project per verification target
  scenario/<id>.txt     one run per target, written by the importer
  src/main.tsx          imports the brand and renders <Frame brand={brand} />
  src/brand.tsx         title, name, version, layers, Mark, mark
  src/styles.css        tailwind, tokens.css, and a @source of the frame's own src
  tools/import-spec.mjs runs the engine and writes the run plus the board entry
  tests/                board.test.ts, mark.test.ts, scenario.test.ts
```

Four details carry weight and are easy to miss.

- `.npmrc` holds `install-links=true`, which installs the `file:` dependency packed rather than linked.
  That is what lets the deployment declare only what it imports: the frame's dependencies arrive with it,
  there is one React in the tree, and no bundler flag is needed.
- `resolve.dedupe: ['react', 'react-dom']` states the same intent at the bundler, because the frame
  declares React as a peer.
- `optimizeDeps: { exclude: ['maplibre-gl'] }` keeps MapLibre's worker resolvable; pre-bundling rewrites it
  into `.vite/deps`, where its relative worker path does not exist, and a map then draws tiles with no
  GeoJSON.
- `styles.css` scans the frame's source (`@source '../node_modules/@ssccsorg/skene-frame/src'`) so Tailwind
  keeps the classes the frame writes.

`main.tsx` renders without `StrictMode`. Its double mount opens the record stream twice, and the first
socket has delivered records by the time it closes, which the second one cannot subtract.

The importer is the part worth copying closely: it runs the real tool and writes what came back. It
counts nothing and evaluates nothing. A generated run and a hand written one are the same kind of file,
and the test asserts both the header and the `count` steps against what the parser derives.

## What the neXus demo should show

`nexus-storage-ref` makes five claims, and each one has a place on the board.

| Claim of the ref tool | What the board draws |
|:---|:---|
| an address is a key, and it resolves by a rule | one fact per key, with the key as its name; the record reading lists them |
| a point read is one read of the medium, and reads nothing else | one fact written by the `read` verb, and one intent that took it |
| a record's address follows from what it holds, so verification is recomputation | a fact for the recomputed address, beside the key it was recomputed from |
| a listing is the only scan, and a question over several axes is answered from it | a selection over the key space, with the bounding axes as the hints that made it |
| a volume sits beside an application file and leaves it alone | a hint carrying the size and modification time the sidecar measured |

The mapping to the three kinds follows the substrate rather than inventing a fourth vocabulary. A record
read is a fact. The bound a reader was held to (the prefix, the axis range, the exit code, the counts) is
a hint. A read the tool performed is an intent, claimed and concluded into the fact it produced.

The native readings carry most of this already: `records`, `graph`, `hex`, `intents`, `act`, `map`,
`outside`, and `shell`. The pack is where the key space belongs, because a key space is neXus's own
resource and the frame must not learn it. A `keyspace` reading over one `{ kind: 'keyspace', at: ... }`
resource, with the pilot narrowing axes, is the one new picture; everything else is arrangement.

Two practical notes. One project per volume keeps the board honest; a box that has to be shown without a
pilot is a second project over the same volume, because a run is a file and the box's answer is part of one.
The run's clock should be the tool's own order, because `count` checks the parser against the tool's count
rather than a count the demo made up.

## Building the demo

The application belongs to neXus, beside the other standalone applications.

```text
apps/nex-view/
  package.json  .npmrc  index.html  vite.config.ts  tsconfig.json
  projects.json
  scenario/<volume>.txt
  src/main.tsx  src/brand.tsx  src/styles.css  src/pack.tsx
  tools/import-volume.mjs
  tests/board.test.ts  tests/mark.test.ts  tests/import.test.ts
```

The clone and the dependency, from this repository's root:

```bash
git clone --depth 1 https://github.com/ssccsorg/skene ../skene
```

```json
"@ssccsorg/skene-frame": "file:../../../skene/packages/frame"
```

The path is relative to the manifest, so it gains one `../` for every directory the manifest sits below
the repository root. `apps/nex-view/package.json` sits two directories down and writes `../../../skene`.

The importer, in the shape the verification space already uses:

```bash
node tools/import-volume.mjs --volume storage/ref/samples/volume --id sample-volume --title "Sample volume"
```

It runs `keys`, `box`, `read`, and `verify` against the volume, and `self-check` against a temporary one,
from a `nexus-storage-ref` binary (a sibling `target/debug/nexus-storage-ref` first, then `NEX_REF_BIN`).
It keeps the exit codes, and writes one run and one board entry. A `--reset` regenerates the whole board,
as the other importer's does.

The header of a written run states the volume, the tool, the subcommand, the exit code, and the counts, and
the run's `count` steps repeat the counts the tool printed. That is the gate: a run edited by hand has to
keep its header and its `count` steps in step, or the test fails.

The first board is one project per volume the repository holds, which today is the committed sample volume
under `storage/ref/samples/volume`. A box that must be shown without a pilot is a second project over the
same volume, and it is a defensible second step once the first reads correctly.

## The small files, verbatim

`vite.config.ts`:

```ts
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { boardIntoBuild } from '@ssccsorg/skene-frame/tools/vite-board.mjs';

export default defineConfig({
  plugins: [react(), tailwindcss(), boardIntoBuild()],
  resolve: { dedupe: ['react', 'react-dom'] },
  optimizeDeps: { exclude: ['maplibre-gl'] },
  server: { port: 5292, strictPort: true },
  preview: { port: 5293, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true },
  test: { include: ['tests/**/*.test.ts'], environment: 'node' },
});
```

The ports are the demo's own. `5280` is the verification space's and `5290` the frame's example, and
`8790` is the frame's default stream.

`src/styles.css`:

```css
@import 'tailwindcss';
@import 'tw-animate-css';
@import '@ssccsorg/skene-frame/tokens.css';

@source '../node_modules/@ssccsorg/skene-frame/src';
@source './';
```

`src/main.tsx`:

```tsx
import { createRoot } from 'react-dom/client';
import { Frame } from '@ssccsorg/skene-frame';
import { brand } from './brand';
import './pack';
import './styles.css';

const root = document.getElementById('root');
if (!root) throw new Error('the page has no root to render into');

createRoot(root).render(<Frame brand={brand} />);
```

## Tests the demo owes

The frame's own suite covers the record model, the address, the parser, the pack registry, and the host.
What a deployment owes is what only it can answer, in `tests/`, run with `vitest` in a node environment.

- the board parses, every project resolves through `bundleOf`, and every `record` path exists
- each run's `parseScenario` yields no unknown verbs, and every `count` step agrees with the facts parsed
  before it
- each run's header agrees with the run's own `count` steps and with the record count
- the importer writes a board and a run that the reader accepts, and a second run over the same volume
  produces the same bytes
- the brand's mark is a path in a view box, and the name, version, and layers are the deployment's own
- no file in the application names a commercial product, which is a one line regex over the tree

## Invariants and pitfalls

- One React. A second copy comes from linking the frame rather than installing it packed, and the symptom
  is a hook error rather than a type error.
- The frame publishes TypeScript source, so a consumer needs a bundler that compiles `node_modules`
  sources; Vite does, and `tsc` alone does not.
- A board read at run time means the build must carry `projects.json` and `scenario/`; that is what
  `boardIntoBuild()` is for, and the example still inlines its own copy of that plugin, which is the one
  place to not copy from.
- `bundleOf` throws for a project with no `record`, and a resource of an unknown kind is refused by name,
  so a typo in `projects.json` is a loud failure rather than an empty panel.
- An edit inside `skene` reaches the deployment on the next `npm install` there, not on the next render.
- The frame remembers what it should under the `skene.*` storage namespace. A deployment that installs a
  store of its own adds its name to that namespace.

## What not to do

- Do not put neXus vocabulary into `skene`. A key space, a blob, a prefix, an axis range, and an exit code
  are the demo's, and each arrives as a pack, a resource kind, or a run line.
- Do not fork or copy the frame. Two copies of one surface is the cost this arrangement exists to remove.
- Do not recompute what the tool reports. The importer takes the numbers; the panel draws them.
- Do not name a commercial product in the application, the board, the runs, the tests, or the comments.
- Do not edit the frame to make one demo easier. If a seam is missing, the seam is the change, and it is
  proposed to `skene` on its own issue.

## Product vocabulary inside skene

The frame names no product in its source or in its tests. The last of it was removed in `skene` commit
`f93fb81`: three prose mentions, four fixtures, and the one test that had been written against two product
binaries and skipped without them. That test now runs against wire records it owns, so its cases execute
instead of skipping.

The names survive in exactly one place, on purpose. `packages/frame/tests/persist.test.ts` holds the list so
it can refuse it, because the frame is one package every deployment links and a key named after one of them
is a key another deployment reads that state from. The guard reads the frame's source rather than its
tests, so a product name in `src/` fails the suite and a fixture is a reviewer's call.

The consequence for this work is simple: there is no vocabulary to copy. A name that belongs to a product
in the demo's code, board, runs, or tests comes from the demo, and it is a change to the demo.

## Open questions for the owner

- Whether the demo ships as a static board or as a live volume first. This report recommends static,
  because it is gateable and matches the existing precedent, with the live bridge as the step after.
- Whether the importer shells out to `nexus-storage-ref` only, or also reads the JSON-RPC surface
  (`read_state` on the server socket, through `nex-client`) for a volume with more records than the ref tool
  prints. The ref tool is the narrower dependency and the honest one for a storage demo; the socket belongs
  to the live step.
- Whether the key space reading is a pack in the demo or a candidate for the frame. This report keeps it in
  the demo, because a key space is a storage layer's own shape and the frame's own readings are named for
  what a record is rather than for where it is kept.
