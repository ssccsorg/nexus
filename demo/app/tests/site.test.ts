// The site the map draws, held to the declaration and to the records.
//
// A boundary is a claim, and the only honest version of it here is the box the place already declares: the map cannot
// draw a fence around ground the units do not stand on. The site's name and each unit's role are read from the records,
// so a rename in the volume moves the map rather than leaving two names to drift apart.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

import { bundleOf, type BoardDocument, type ProjectDocument } from '@ssccsorg/skene-frame';
import { overlaysOf, type Overlay } from '@ssccsorg/skene-view-map';

import '../src/pack';
import '../src/site';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as BoardDocument;
const document = board.projects.find((project) => project.id === 'esg-scenario') as ProjectDocument;
const bundle = bundleOf(document, board.projects);

interface Measured {
  creator: string;
  payload: { metric?: string; site?: string; reference?: string } | null;
}

const chain = (bundle as unknown as { esg?: { records: Measured[] } }).esg;
const measured = (chain?.records ?? []).filter((record) => record.payload?.metric);

/** The polygon the site layer draws, as the ring the map is handed. */
function siteRing(): { label: unknown; ring: number[][] } {
  const overlay = overlaysOf(bundle.place ?? { view: [0, 0, 1, 1] }).find(
    (layer: Overlay) => layer.id === 'site',
  );
  expect(overlay, 'the deployment declares the site layer').toBeDefined();
  const data = overlay?.data as {
    features: { properties: Record<string, unknown>; geometry: { coordinates: number[][][] } }[];
  };
  const feature = data.features[0];
  return { label: feature?.properties.label, ring: feature?.geometry.coordinates[0] ?? [] };
}

test('the boundary is the place’s own ground, corner for corner', () => {
  const bounds = bundle.place?.geo?.bounds;
  expect(bounds, 'the place declares where it lies on the earth').toBeDefined();
  const [west, south, east, north] = bounds as [number, number, number, number];
  expect(siteRing().ring).toEqual([
    [west, north],
    [east, north],
    [east, south],
    [west, south],
    [west, north],
  ]);
});

test('the site on the map is the site the measured records name', () => {
  const sites = new Set(measured.map((record) => record.payload?.site));
  expect(sites.size, 'the records name one site').toBe(1);
  expect(siteRing().label, 'the boundary is labelled with that site').toBe([...sites][0]);
});

test('a unit is the instrument whose record it stands for', () => {
  const units = Object.entries(bundle.place?.units ?? {});
  expect(units.length, 'the site stands its machines').toBeGreaterThan(0);
  for (const [id, unit] of units) {
    const record = measured.find((held) => held.creator === id);
    expect(record, `${id} wrote a record the volume holds`).toBeDefined();
    expect(unit.role, `${id} is named by its record`).toContain(String(record?.payload?.reference));
  }
});

test('a place with no ground gets no boundary', () => {
  const plain = overlaysOf({ view: [0, 0, 10, 10] } as never);
  expect(plain.find((layer: Overlay) => layer.id === 'site')).toBeUndefined();
});
