// The arrangement the board comes up in, and what it does with the one the pilot leaves.
//
// The frame's panels answer for themselves (`starts`), and a project may declare an arrangement of its own, which is
// applied when the project opens over whatever the pilot left. A bundle that says nothing therefore opens at the panels'
// own answers and keeps the pilot's arrangement, which is what a board whose panels are the frame's wants: the panels the
// project is read in are out, the console is away, and a panel a pilot closes stays closed on the next visit.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

// Importing the demo's modules is what registers its panel beside the frame's.
import '../src/evidence';
import { declaredPanels } from '@ssccsorg/skene-frame/lib/panels';

const ROOT = fileURLToPath(new URL('../', import.meta.url));

const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as {
  projects: { id: string; resources: { kind: string; at: unknown }[] }[];
};

/** What each panel answers for itself, which is where a project that declares nothing leaves the board. */
function starts(): Record<string, boolean | undefined> {
  return Object.fromEntries(declaredPanels().map((panel) => [panel.key, panel.starts]));
}

test('the project declares no arrangement, so what is kept is the pilot’s', () => {
  const declared = board.projects
    .find((project) => project.id === 'esg-scenario')
    ?.resources.find((resource) => resource.kind === 'layout')?.at;
  expect(declared, 'a declaration would come back over the pilot’s choice on every open').toEqual({});
});

test('the board opens with the declarations, the space column and the transport', () => {
  const own = starts();
  expect(own.rail, 'the declarations').toBe(true);
  expect(own.plan, 'the space column').toBe(true);
  expect(own.timeline, 'the transport').toBe(true);
  expect(own.evidence, 'and not the picked record').toBe(false);
  expect(own.console, 'and not the console').toBe(false);
});
