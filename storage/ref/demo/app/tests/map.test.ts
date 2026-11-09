// The place the ESG project declares, held to the frame's rule for drawing a map.
//
// A map is not a picture of any data: it is the place's declaration read against the run. So what is asserted here is
// that the declaration answers the frame's own question, that every participant a record names has a kind, and that the
// units the map draws are the machines and not the records' bookkeeping.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

import {
  actors,
  bundleOf,
  parseScenario,
  type BoardDocument,
  type ProjectDocument,
} from '@ssccsorg/skene-frame';
import { located, mappable, scenes, worldOf } from '@ssccsorg/skene-frame/lib/world';

import '../src/pack';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as BoardDocument;

const document = board.projects.find((project) => project.id === 'esg-scenario') as ProjectDocument;
const bundle = bundleOf(document, board.projects);
const text = readFileSync(`${ROOT}${bundle.record}`, 'utf8');
const parsed = parseScenario(text);

// The frame's own reading of a bundle, assembled the way the board assembles it.
const world = worldOf(
  {
    records: parsed.records,
    events: parsed.events,
    unknown: parsed.unknown,
    project: bundle,
    title: text,
    scenes: scenes(parsed.events, bundle),
    chain: [],
    actors: actors(parsed.records),
    intents: [],
    hints: [],
  },
  scenes(parsed.events, bundle).length,
);

test('the place answers the frame, so the map is offered rather than empty', () => {
  expect(mappable(bundle.place, world), 'the place declares a ground and a member on it').toBe(true);
});

test('every unit the map draws is a machine whose kind presents a location', () => {
  const kinds = bundle.kinds ?? {};
  const units = bundle.place?.units ?? {};
  expect(Object.keys(units).length, 'the site stands something').toBeGreaterThan(0);

  for (const [id, unit] of Object.entries(units)) {
    const icon = unit.icon ?? '';
    expect(kinds[icon], `${id} is a kind the project assembled`).toBeDefined();
    expect(kinds[icon], `${id} presents a location`).toContain('shows');
  }

  // `located` is the frame's answer, and a map with no located member is not offered at all.
  expect(world.standing.filter(located).length, 'the map has something to draw').toBe(
    Object.keys(units).length,
  );
});

test('every unit on the map is a participant the records name', () => {
  // A unit is where a machine stands, so the units and the records' creators are the same set of participants. A unit
  // named for nobody is a pin over nothing, and a creator with no unit is a participant the map does not draw.
  const chain = (bundle as unknown as { esg?: { records: { creator: string | null }[] } }).esg;
  const creators = new Set(
    (chain?.records ?? [])
      .map((record) => record.creator)
      .filter((creator): creator is string => Boolean(creator)),
  );
  expect(creators.size, 'the chain names its participants').toBeGreaterThan(1);

  const units = Object.keys(bundle.place?.units ?? {});
  expect(units.length, 'the map stands something').toBeGreaterThan(0);
  for (const unit of units) {
    expect(creators.has(unit), `${unit} wrote a record the volume holds`).toBe(true);
  }
});
