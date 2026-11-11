// The board this application ships with, held to the shape the frame reads it in and to the tool's own report.
//
// Nothing here draws. What it pins is that each project resolves to a bundle the frame can read, that its run parses
// with no verb dropped, that every count the run declares holds, and that the numbers on the key-space panel are the
// ones the reference tool printed rather than a second count taken here.

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

import {
  bundleOf,
  parseScenario,
  titleOf,
  type BoardDocument,
  type ProjectDocument,
} from '@ssccsorg/skene-frame';
import { standing } from '@ssccsorg/skene-frame/lib/packs';
import { declaredReadings } from '@ssccsorg/skene-frame/lib/readings';

// The pack registers the `keyspace` reader where it is imported, the same way the application's `main.tsx` does. The map
// is imported because the board opens on it: a view stands where its package is imported, so a test that resolved the
// board against views the deployment does not install would be reading a register the application does not have.
import '@ssccsorg/skene-view-map';
import '../src/pack';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as BoardDocument;

/** What the pack puts on the bundle under the project's own resource kind. */
interface KeySpace {
  volume: string;
  keys: string[];
  listed: number;
  box: { axis: number; from: string; until: string; records: number } | null;
  records: number;
}

function read(id: string): { bundle: ReturnType<typeof bundleOf>; space: KeySpace; text: string } {
  const document = board.projects.find((entry) => entry.id === id) as ProjectDocument;
  expect(document, `the board names ${id}`).toBeDefined();
  const bundle = bundleOf(document, board.projects);
  const space = (bundle as unknown as { keyspace: KeySpace }).keyspace;
  return { bundle, space, text: readFileSync(`${ROOT}${bundle.record}`, 'utf8') };
}

const ids = board.projects.map((project) => project.id);

/** The projects generated from the volumes this board commits, and the projects taken from another board. */
const VOLUMES = ['esg-scenario', 'sample-volume'];
const SHARED = [
  'drive',
  'pump',
  'ecu',
  'drone',
  'field',
  'digital',
  'game',
  'villa',
  'spiderwing',
  'kwaitsing',
];

/** A header line the importer wrote, as text. */
function header(text: string, key: string): string {
  const match = new RegExp(`^#\\s*${key}:\\s*(\\S+)$`, 'm').exec(text);
  expect(match, `the run states its ${key} in the header`).not.toBeNull();
  return String(match?.[1]);
}

/** A hint the run declares, by name. */
function hint(text: string, label: string): string | undefined {
  return parseScenario(text).records.find(
    (record) => record.kind === 'hint' && record.label === label,
  )?.content;
}

test('the board holds one project for each committed volume, and the shared one', () => {
  expect(ids).toEqual([...VOLUMES, ...SHARED]);
  expect(new Set(ids).size, 'each volume has one project').toBe(ids.length);
});

test('the shared projects resolve through the frame, and their runs are runs', () => {
  // A project from another board is not a volume: it carries no key space and no record list, so what it is held to here
  // is that it resolves, that its run parses with no verb dropped, and that every count the run declares holds.
  for (const id of SHARED) {
    const document = board.projects.find((entry) => entry.id === id) as ProjectDocument;
    expect(document, `the board names ${id}`).toBeDefined();
    const bundle = bundleOf(document, board.projects);
    expect(bundle.record, `${id} declares a run`).toMatch(/^scenario\/.+\.txt$/);
    const text = readFileSync(`${ROOT}${bundle.record}`, 'utf8');
    expect(titleOf(text).length, `${id}'s run names itself`).toBeGreaterThan(0);
    const { checks, records, unknown } = parseScenario(text);
    expect(unknown, `${id} uses only modelled verbs`).toEqual([]);
    expect(records.length, `${id} carries a run`).toBeGreaterThan(10);
    for (const check of checks) {
      expect(check.derived, `${id} at ${check.at}s: the file says ${check.expected}`).toBe(
        check.expected,
      );
    }
  }
});

test('a project resolves through the frame, and its run is where it says', () => {
  for (const id of VOLUMES) {
    const { bundle, space, text } = read(id);
    expect(bundle.record, `${id} declares a run`).toMatch(/^scenario\/.+\.txt$/);
    expect(text.length, `${id}'s run is not empty`).toBeGreaterThan(0);
    expect(titleOf(text).length, `${id}'s run names itself`).toBeGreaterThan(0);
    // The board and the run describe one volume, and the run is the tool's own report of it.
    expect(header(text, 'volume'), `${id}'s run names the volume it measured`).toBe(space.volume);
  }
});

test('a project opens on the reading it names, and brings the pack that holds it', () => {
  for (const id of VOLUMES) {
    const document = board.projects.find((entry) => entry.id === id) as ProjectDocument;
    const views = document.resources.find((resource) => resource.kind === 'views')?.at as
      | { pack?: string | string[]; main?: string }
      | undefined;
    expect(views?.pack, `${id} brings the volume pack`).toBe('volume');
    const main = String(views?.main ?? '');
    expect(main.length, `${id} names the reading it opens on`).toBeGreaterThan(0);
    // The reading has to stand for this project: a name no register holds is a board that opens on nothing.
    const stands = standing(declaredReadings(), bundleOf(document, board.projects)).map(
      (reading) => reading.key,
    );
    expect(stands, `${id} opens on ${main}, which stands for it`).toContain(main);
  }
});

test('every run is read whole: no verb is dropped and every count holds', () => {
  for (const id of VOLUMES) {
    const { text } = read(id);
    const { checks, records, unknown } = parseScenario(text);
    expect(unknown, `${id} uses only modelled verbs`).toEqual([]);
    expect(checks.length, `${id} pins its counts`).toBeGreaterThanOrEqual(6);
    for (const check of checks) {
      expect(check.derived, `${id} at ${check.at}s: the file says ${check.expected}`).toBe(
        check.expected,
      );
    }
    expect(records.length, `${id} carries a run`).toBeGreaterThan(10);
  }
});

test('the header agrees with the hints the run carries', () => {
  for (const id of VOLUMES) {
    const { text } = read(id);
    const bound = hint(text, 'key-count') ?? '';
    const stated = Object.fromEntries(
      bound.split(',').map((pair) => pair.split('=') as [string, string]),
    );
    // The breakdown covers every prefix the volume holds, so its total is the header's key count. `exit` is the
    // listing's own status and not part of the arithmetic.
    const total = Object.entries(stated)
      .filter(([name]) => name !== 'exit')
      .reduce((sum, [, value]) => sum + Number(value), 0);
    expect(total, `${id} states one key count`).toBe(Number(header(text, 'keys')));
    expect(Number(hint(text, 'box-count')), `${id} states one record count`).toBe(
      Number(header(text, 'records')),
    );
    // The exit codes are the read's, the verification's, and the contract's, in that order.
    expect(header(text, 'exit').split('/')).toEqual([
      hint(text, 'read-exit'),
      '0',
      hint(text, 'self-check-exit'),
    ]);
  }
});

test('the key space the panel draws is the one the run lists', () => {
  for (const id of VOLUMES) {
    const { space, text } = read(id);
    // A key is a prefix and a name, so its label carries a slash. The synthetic labels the run uses elsewhere do not.
    const listed = parseScenario(text)
      .records.filter((record) => record.kind === 'fact' && /^[a-z]+\//.test(record.label ?? ''))
      .map((record) => record.label);
    expect(listed, `${id} lists the keys it draws`).toEqual(space.keys);
    expect(space.listed, `${id} draws the count the tool printed`).toBe(space.keys.length);
    expect(space.records, `${id} draws the records the box admitted`).toBe(
      Number(hint(text, 'box-count')),
    );
  }
});

test('the records the panel draws are the ones the run lists', () => {
  for (const id of VOLUMES) {
    const { bundle, text } = read(id);
    const chain = (bundle as unknown as { esg?: { records: { key: string }[] } }).esg;
    expect(chain, `${id} carries the records its run lists`).toBeDefined();
    const { records } = parseScenario(text);
    const drawn = records.filter(
      (record) => record.kind === 'fact' && /^record-\d+$/.test(record.label ?? ''),
    );
    const keys = records.filter(
      (record) => record.kind === 'hint' && /^record-key-\d+$/.test(record.label ?? ''),
    );
    const stated = records.find(
      (record) => record.kind === 'fact' && record.label === 'records-held',
    );

    // The panel, the count the run states, the rows it writes, and the kinds it names are one answer.
    expect(chain?.records.length, `${id} draws every record the run lists`).toBe(drawn.length);
    expect(Number(stated?.bytes), `${id} states the count it draws`).toBe(drawn.length);
    expect(
      keys.map((record) => record.content),
      `${id} names the key of each record it draws`,
    ).toEqual(chain?.records.map((record) => record.key));
    const kinds = (hint(text, 'record-kinds') ?? '')
      .split(',')
      .map((pair) => Number(pair.split('=')[1] ?? 0));
    expect(
      kinds.reduce((sum, at) => sum + at, 0),
      `${id} states the kinds it draws`,
    ).toBe(drawn.length);
  }
});

test('every link an intent carries lands on a record the volume holds', () => {
  // The chain is the claim: a conclusion is a fact of its own, and an intent names the facts it read and the fact it
  // concluded. A link that names nothing would leave a reader a chain that cannot be followed, which is the one thing a
  // record of evidence may not do.
  let intents = 0;
  for (const id of VOLUMES) {
    const { bundle } = read(id);
    const chain = (bundle as unknown as {
      esg?: { records: { key: string; kind: string; address: string; record: Record<string, unknown> }[] };
    }).esg;
    expect(chain, `${id} carries the records its run lists`).toBeDefined();
    const held = new Set(chain?.records.map((record) => record.address));

    for (const record of chain?.records ?? []) {
      if (record.kind !== 'intent') continue;
      intents += 1;
      const fields = record.record as {
        from_facts?: string[];
        status?: Record<string, { to_fact?: string }>;
      };
      const concluded = fields.status?.Concluded?.to_fact;
      expect(concluded, `${id}: ${record.key} is concluded`).toBeTruthy();
      const links = [...(fields.from_facts ?? []), String(concluded)];
      expect(links.length, `${id}: ${record.key} reads something`).toBeGreaterThan(1);
      for (const link of links) {
        expect(held.has(link), `${id}: ${record.key} names ${link}, which the volume holds`).toBe(
          true,
        );
      }
    }
  }
  // Without this the test would pass over a volume that holds no chain at all.
  expect(intents, 'a committed volume carries a concluded chain').toBeGreaterThan(0);
});

test('the board and its runs carry no native character', () => {
  // Every address on the board is the tool's rendering, so the demo reads the same in a terminal that has no font for
  // the native alphabet. The run and the key space are the two places an address is written down.
  for (const id of VOLUMES) {
    const { space, text } = read(id);
    expect(text, `${id}'s run is ASCII`).toMatch(/^[\x20-\x7E\n]*$/);
    expect(space.keys.join('\n'), `${id}'s keys are ASCII`).toMatch(/^[\x20-\x7E\n]*$/);
  }
});

/** Every file the application is made of, so a name added anywhere is caught. */
function files(under = ROOT): string[] {
  const found: string[] = [];
  for (const name of readdirSync(under)) {
    if (['node_modules', 'dist'].includes(name)) continue;
    const path = join(under, name);
    if (statSync(path).isDirectory()) found.push(...files(path));
    else if (/\.(ts|tsx|mjs|sh|json|css|html|txt|md|npmrc)$/.test(path) || name === '.npmrc') {
      found.push(path);
    }
  }
  return found.sort();
}

test('no file in the application names a commercial product', () => {
  // Assembled from fragments so the guard's own source does not match the pattern it looks for.
  const product = new RegExp(
    ['kt' + 'ema', 'kt' + 'ma', 'kl' + 'eis', 'kl' + 'etos', 'kr' + 'isis', 'te' + 'los'].join('|'),
    'i',
  );
  for (const path of files()) {
    expect(readFileSync(path, 'utf8'), `${path} names a product`).not.toMatch(product);
  }
});
