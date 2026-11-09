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
    'measurement',
    'transformation',
    'result',
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

test('a payload line follows the order the record was written in', () => {
  // The author of the record decides what it leads with. A screen that re-sorted the fields would be a second opinion
  // about which field matters, and the record would have to be read past to find its figure.
  const leading: Record<string, string> = {
    electricity_consumption: 'value=412000',
    natural_gas_consumption: 'value=96400',
    refrigerant_top_up: 'value=12.5',
    production_output: 'value=18400',
  };
  let measured = 0;
  for (const record of facts) {
    // A conclusion is a fact too, and it carries no metric: the measured records are the ones a meter reported.
    const metric = (record.payload as { metric?: string } | null)?.metric;
    if (!metric) continue;
    measured += 1;
    expect(leading[metric], `${metric} is a metric the scenario writes`).toBeDefined();
    expect(payloadLine(record).startsWith(leading[metric]), `${record.key} leads with its figure`).toBe(
      true,
    );
  }
  expect(measured, 'the scenario carries the measured records').toBe(4);
});

test('the reported figures of every conclusion are on its row', () => {
  // Scope 2 on both bases, Scope 1 split by source, and the intensity over the two.
  const lines = facts.map((record) => payloadLine(record)).join('\n');
  expect(lines, 'Scope 2 states the market-based result').toContain('market_based_tco2e=84.58');
  expect(lines, 'Scope 2 states the location-based result').toContain(
    'location_based_tco2e=52.74',
  );
  expect(lines, 'Scope 1 states its result').toContain('tco2e=45.53');
  expect(lines, 'the intensity states its metric').toContain('kgco2e_per_unit=7.07');
  expect(lines, 'the intensity states its numerator').toContain('total_tco2e=130.11');
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
