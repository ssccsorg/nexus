// A project travels between boards in both directions, and a second run says it is already there.
//
// The shared project is the demonstration that the board is not one domain: it is taken from another board rather than
// imported from a volume. That claim only holds if the copy is symmetric, so the same tool is run here with the two
// directories the other way round, and the bytes are compared at both ends.

import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, expect, test } from 'vitest';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const TOOL = join(ROOT, 'tools/sync-project.mjs');
const SHARED = join(ROOT, '..', 'shared');
const ID = 'ecu';

const made: string[] = [];
function scratch(): string {
  const dir = mkdtempSync(join(tmpdir(), 'nex-ref-demo-sync-'));
  made.push(dir);
  return dir;
}
afterEach(() => {
  for (const dir of made.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function sync(args: string[]) {
  return spawnSync(process.execPath, [TOOL, ...args], { encoding: 'utf8', cwd: ROOT });
}

/** The project an id names in a board, as text, so two boards can be compared without depending on order. */
function projectAt(board: string, id: string): string {
  const text = JSON.parse(readFileSync(join(board, 'projects.json'), 'utf8')) as {
    projects: { id: string }[];
  };
  return JSON.stringify(text.projects.find((each) => each.id === id));
}

test('the shared project copies into a board, and its run with it', () => {
  const board = scratch();
  const done = sync(['--from', SHARED, '--id', ID, '--into', board]);
  expect(done.status, done.stderr).toBe(0);
  expect(projectAt(board, ID)).toBe(projectAt(ROOT, ID));
  expect(readFileSync(join(board, 'scenario', `${ID}.txt`), 'utf8')).toBe(
    readFileSync(join(ROOT, 'scenario', `${ID}.txt`), 'utf8'),
  );
});

test('the same command carries it back, and the source is what comes back', () => {
  // The destination starts as the shared board, so the copy that returns has to reproduce it exactly.
  const board = scratch();
  cpSync(SHARED, board, { recursive: true });
  const done = sync(['--from', ROOT, '--id', ID, '--into', board, '--replace']);
  expect(done.status, done.stderr).toBe(0);
  expect(projectAt(board, ID)).toBe(projectAt(SHARED, ID));
  expect(readFileSync(join(board, 'scenario', `${ID}.txt`), 'utf8')).toBe(
    readFileSync(join(SHARED, 'scenario', `${ID}.txt`), 'utf8'),
  );
});

test('a second run is the second run saying it is already in step', () => {
  const board = scratch();
  expect(sync(['--from', SHARED, '--id', ID, '--into', board]).status).toBe(0);
  const once = readFileSync(join(board, 'projects.json'), 'utf8');
  const again = sync(['--from', SHARED, '--id', ID, '--into', board]);
  expect(again.status, again.stderr).toBe(0);
  expect(again.stdout).toContain('already in step');
  expect(readFileSync(join(board, 'projects.json'), 'utf8')).toBe(once);
});

test('a board holding another version keeps it unless the copy is asked for', () => {
  const board = scratch();
  writeFileSync(
    join(board, 'projects.json'),
    `${JSON.stringify({ note: 'a board', projects: [{ format: 'project/1', id: ID, resources: [] }] }, null, 2)}\n`,
  );
  cpSync(join(SHARED, 'scenario'), join(board, 'scenario'), { recursive: true });
  const refused = sync(['--from', SHARED, '--id', ID, '--into', board]);
  expect(refused.status, 'a copy over a different version is refused').toBe(1);
  expect(refused.stderr).toContain('--replace');
  expect(sync(['--from', SHARED, '--id', ID, '--into', board, '--replace']).status).toBe(0);
  expect(projectAt(board, ID)).toBe(projectAt(SHARED, ID));
});
