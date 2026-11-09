// The line a record is drawn as, held to the figures the record exists to state.
//
// A payload arrives with its keys in the medium's order, and that order is not the order a reader needs: `tco2e` sat
// behind `basis` and `factor_kg_per_kwh`, so a line cut to a few fields reported the method and not the result. The
// rule is that a record's quantities lead, and this holds the two conclusions to it.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

import { payloadLine, type EsgRecord } from '../src/pack';

const ROOT = fileURLToPath(new URL('../', import.meta.url));

const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as {
  projects: { id: string; resources: { kind: string; at: unknown }[] }[];
};

const chain = board.projects.find((project) => project.id === 'esg-scenario')?.resources.find(
  (resource) => resource.kind === 'esg',
)?.at as { records: EsgRecord[] } | undefined;

const facts = (chain?.records ?? []).filter((record) => record.kind === 'fact');

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
    const scalars = Object.keys(record.payload ?? {}).filter((key) =>
      ['string', 'number', 'boolean'].includes(typeof (record.payload ?? {})[key]),
    );
    const line = payloadLine(record);
    for (const key of scalars) {
      expect(line, `${record.key} carries ${key}`).toContain(`${key}=`);
    }
  }
});
