// A surface stands only for a project that declares the thing it needs.
//
// The evidence panel reads the links a record carries, so it is not a panel of the frame: it belongs to a project that
// holds a chain. The board states that as a capability and the panel names it, which is the same seam the frame's own
// console uses, so a volume of plain readings gets no panel rather than a panel that opens onto nothing.
//
// The rule is checked on the register and on the committed board together, because either half alone passes: a panel
// with no `needs` stands everywhere, and a capability no panel asks for gates nothing.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

// Importing the module is what registers the panel, the way the entry imports it.
import '../src/evidence';
import { declaredPanels, standingPanels } from '@ssccsorg/skene-frame/lib/panels';

const ROOT = fileURLToPath(new URL('../', import.meta.url));

const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as {
  projects: { id: string; resources: { kind: string; at: unknown }[] }[];
};

/** The capabilities the committed board declares for one project. */
function abilitiesOf(id: string): string[] {
  const project = board.projects.find((each) => each.id === id);
  const held = project?.resources.find((resource) => resource.kind === 'abilities')?.at;
  return Array.isArray(held) ? (held as string[]) : [];
}

/** The panels that stand for a project holding these capabilities. */
function standingFor(abilities: string[]): string[] {
  return standingPanels(declaredPanels(), abilities).map((panel) => panel.key);
}

test('the evidence surface stands for a project that declares it and for no other', () => {
  expect(standingFor(abilitiesOf('esg-scenario'))).toContain('evidence');
  expect(standingFor(abilitiesOf('sample-volume'))).not.toContain('evidence');
  expect(standingFor([]), 'a project that declares nothing gets no surface of this demo').not.toContain(
    'evidence',
  );
});

test('the volume with a chain declares the capability, and the volume of readings does not', () => {
  // The capability is written from the volume's own shape: it holds intents, which is what a record's links are made
  // of. A volume that holds one observation has a chain of nothing.
  expect(abilitiesOf('esg-scenario')).toEqual(['evidence']);
  expect(abilitiesOf('sample-volume')).toEqual([]);
});

test('the panel names the capability, so the declaration reaches it', () => {
  const ours = declaredPanels().find((panel) => panel.key === 'evidence');
  expect(ours?.needs).toBe('evidence');
  expect(ours?.starts, 'and it is open where it stands').toBe(true);
});
