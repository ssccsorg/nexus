// What the screen says about the records, held to the records and to the tool's own answers.
//
// Two vocabularies meet on a row and they are not the same list. The frame marks a record's kind, which is a term of
// the format. This application names the record's place in the chain, which is read from the links the records carry.
// The figures are the third thing a reader came for, and a line that dropped them would be a row about nothing.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

import { concludedOf, payloadLine, roleOf, type EsgRecord } from '../src/pack';

const ROOT = fileURLToPath(new URL('../', import.meta.url));

const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as {
  projects: { id: string; resources: { kind: string; at: unknown }[] }[];
};

const chain = board.projects.find((project) => project.id === 'esg-scenario')?.resources.find(
  (resource) => resource.kind === 'esg',
)?.at as { records: EsgRecord[] } | undefined;

const ordered = [...(chain?.records ?? [])].sort((left, right) => left.written - right.written);
const concluded = new Set(
  ordered.map((record) => concludedOf(record)).filter((at): at is string => Boolean(at)),
);
const facts = ordered.filter((record) => record.kind === 'fact');

test('the chain reads as a measurement, a transformation, a result, and a requirement', () => {
  expect(ordered.map((record) => roleOf(record, concluded))).toEqual([
    'measurement',
    'measurement',
    'measurement',
    'transformation',
    'result',
    'transformation',
    'result',
    'requirement',
  ]);
});

test('only the facts whose address is their own are recomputed, and every one holds', () => {
  // A result is filed at its intent's address, and an intent and a hint are named, so `verify` has nothing to
  // recompute for them. Recording a state there would be an answer the tool never gave.
  for (const record of ordered) {
    if (record.kind === 'fact' && !concluded.has(record.address)) {
      expect(record.addressState, `${record.key} is recomputed`).toBe('valid');
    } else {
      expect(record.addressState, `${record.key} has nothing to recompute`).toBeNull();
    }
  }
});

test('a payload line leads with the quantities the record measured', () => {
  expect(facts.length, 'the scenario carries measured facts').toBeGreaterThan(0);
  for (const record of facts) {
    const numeric = Object.values(record.payload ?? {}).filter(
      (value) => typeof value === 'number',
    );
    if (numeric.length === 0) continue;
    const first = payloadLine(record).split(' · ')[0] ?? '';
    expect(first, `${record.key} leads with a quantity`).toMatch(/^[a-z_]+=-?[0-9]/);
  }
});

test('the reported figure of each conclusion is on its row', () => {
  // The two conclusions are the facts the controller filed: the scenario states 84.58 and 45.53 tCO2e.
  const lines = facts.map((record) => payloadLine(record)).join('\n');
  expect(lines, 'Scope 2 states its result').toContain('tco2e=84.58');
  expect(lines, 'Scope 1 states its result').toContain('tco2e=45.53');
});

test('a payload line drops no scalar field', () => {
  for (const record of facts) {
    const payload = record.payload ?? {};
    const scalars = Object.keys(payload).filter((key) =>
      ['string', 'number', 'boolean'].includes(typeof payload[key]),
    );
    const line = payloadLine(record);
    for (const key of scalars) {
      expect(line, `${record.key} carries ${key}`).toContain(`${key}=`);
    }
  }
});
