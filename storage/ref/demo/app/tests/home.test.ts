// The reading a project opens on is the one its own bundle names.
//
// The frame comes up where a project declares, so a board whose projects each name a main reading is a board whose
// projects each open on their own picture. This is read from the committed board rather than from a fixture, because the
// declaration is the board's and a copy would be a second place to keep it.

import { bundleOf, declaredReadings, standing } from '@ssccsorg/skene-frame';
import { expect, test } from 'vitest';
// The same three packages the deployment imports, so the register this reads is the register the board offers.
import '@ssccsorg/skene-view-map';
import '@ssccsorg/skene-view-shell';
import '@ssccsorg/skene-view-media';
import '../src/pack';
import '../src/chain';
import board from '../projects.json';

const projects = board.projects as unknown as Parameters<typeof bundleOf>[0][];

function openIn(id: string): { main: string | undefined; keys: string[] } {
  const bundle = bundleOf(
    projects.find((each) => each.id === id)!,
    projects,
  ) as unknown as { views?: { main?: string }; layout?: never };
  const readings = standing(declaredReadings(), { layout: bundle.layout, views: bundle.views });
  return { main: bundle.views?.main, keys: readings.map((each) => each.key) };
}

test.each([
  ['esg-scenario', 'chain'],
  ['sample-volume', 'esg'],
])('%s opens on the reading it declares', (id, main) => {
  const opened = openIn(id);
  expect(opened.main).toBe(main);
  expect(opened.keys, `${id} stands the reading it opens on`).toContain(main);
});

test('a project that names no main opens on the frame\u2019s own first', () => {
  const opened = openIn('villa');
  expect(opened.main).toBeUndefined();
  // The seat reading is the register's own first, so a project that names none opens on the shell.
  expect(opened.keys[0]).toBe('shell');
  expect(opened.keys, 'and the map stands beside it').toEqual(expect.arrayContaining(['map']));
});
