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

// The pack registers the `keyspace` reader where it is imported, the same way the application's `main.tsx` does. A test
// that resolved the board without it would be reading a bundle the app cannot read either.
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

test('the board holds one project for the committed sample volume', () => {
  expect(ids).toEqual(['sample-volume']);
});

test('a project resolves through the frame, and its run is where it says', () => {
  for (const id of ids) {
    const { bundle, text } = read(id);
    expect(bundle.record, `${id} declares a run`).toMatch(/^scenario\/.+\.txt$/);
    expect(text.length, `${id}'s run is not empty`).toBeGreaterThan(0);
    expect(titleOf(text), `${id} is named by its own run`).toBe('Sample volume');
  }
});

test('a project opens on the reading it names, and brings the pack that holds it', () => {
  for (const id of ids) {
    const document = board.projects.find((entry) => entry.id === id) as ProjectDocument;
    const views = document.resources.find((resource) => resource.kind === 'views')?.at as
      | { pack?: string | string[]; main?: string }
      | undefined;
    expect(views?.pack, `${id} brings the volume pack`).toBe('volume');
    expect(views?.main, `${id} opens on the key space`).toBe('keyspace');
  }
});

test('every run is read whole: no verb is dropped and every count holds', () => {
  for (const id of ids) {
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
  for (const id of ids) {
    const { text } = read(id);
    const bound = hint(text, 'key-count') ?? '';
    const stated = Object.fromEntries(
      bound.split(',').map((pair) => pair.split('=') as [string, string]),
    );
    expect(Number(stated.facts) + Number(stated.blobs), `${id} states one key count`).toBe(
      Number(header(text, 'keys')),
    );
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
  for (const id of ids) {
    const { space, text } = read(id);
    const listed = parseScenario(text)
      .records.filter(
        (record) =>
          record.kind === 'fact' &&
          (record.label?.startsWith('facts/') || record.label?.startsWith('blob/')),
      )
      .map((record) => record.label);
    expect(listed, `${id} lists the keys it draws`).toEqual(space.keys);
    expect(space.listed, `${id} draws the count the tool printed`).toBe(space.keys.length);
    expect(space.records, `${id} draws the records the box admitted`).toBe(
      Number(hint(text, 'box-count')),
    );
  }
});

/** Every file the application is made of, so a name added anywhere is caught. */
function files(under = ROOT): string[] {
  const found: string[] = [];
  for (const name of readdirSync(under)) {
    if (['node_modules', 'dist'].includes(name)) continue;
    const path = join(under, name);
    if (statSync(path).isDirectory()) found.push(...files(path));
    else if (/\.(ts|tsx|mjs|json|css|html|txt|md|npmrc)$/.test(path) || name === '.npmrc') {
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
