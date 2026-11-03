// The importer is the only thing that produces the numbers, so what it writes is held to the reader and to itself.
//
// The fixture is the reference tool's own answer for the committed sample volume, kept here so the run's shape is tested
// without a built binary. The end-to-end case runs the importer for real and compares the bytes with the ones committed,
// and it says so when the binary is not there rather than pretending the file is right.

import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, test } from 'vitest';

import { parseScenario } from '@ssccsorg/skene-frame';
import { defaultBin, projectOf, scenarioOf } from '../tools/import-volume.mjs';

const HERE = fileURLToPath(new URL('../', import.meta.url));

const COORDINATE = '가뛈튋엀춣덞긗폟뮶윙첌췠뭻궖뺨감춑꼊쐝끒';
const BLOB = 'blob/578e724ee7e9c587be42c260a6d7e89abf7b75d502d3946ddc473b25aa22e3b8';

/** What the tool answered for `storage/ref/samples/volume`. */
const measured = {
  id: 'sample-volume',
  title: 'Sample volume',
  volume: 'storage/ref/samples/volume',
  coordinate: COORDINATE,
  keys: [`${BLOB}.bin`, `${BLOB}.bin.meta`, `facts/f_${COORDINATE}.fact`],
  keyCount: 3,
  keyExit: 0,
  box: { axis: 0, from: '가', until: '가', records: 1 },
  boxExit: 0,
  readBytes: 164,
  readKey: `facts/f_${COORDINATE}.fact`,
  readExit: 0,
  origin: 'did:example:asset-1',
  creator: 'sensor-1',
  written: '1793630807643521000 ns',
  payload: 125,
  address: 'valid, recomputed from the origin, the creator, and the payload',
  verifyExit: 0,
  appBytes: 9,
  sidecarExit: 0,
  selfCheck: true,
  selfCheckExit: 0,
};

test('a run built from the tool’s answer is one the reader accepts', () => {
  const text = scenarioOf(measured);
  const { unknown, checks, records } = parseScenario(text);
  expect(unknown, 'the run uses only modelled verbs').toEqual([]);
  expect(checks.length, 'the run pins its counts').toBeGreaterThanOrEqual(6);
  for (const check of checks) {
    expect(check.derived, `at ${check.at}s the file says ${check.expected}`).toBe(check.expected);
  }
  expect(records.length, 'the run carries the work').toBeGreaterThan(10);
});

test('the same answer writes the same run, twice', () => {
  // A generated run is a file a reviewer diffs, so a second import over one volume has to be a no-op.
  expect(scenarioOf(measured)).toBe(scenarioOf(measured));
});

test('the project carries the key space the panel draws', () => {
  const project = projectOf(measured);
  expect(project.id).toBe('sample-volume');
  expect(project.resources.map((resource) => resource.kind)).toEqual([
    'channel',
    'record',
    'keyspace',
    'views',
  ]);
  const space = project.resources[2].at as { keys: string[]; listed: number; records: number };
  expect(space.keys).toEqual(measured.keys);
  expect(space.listed).toBe(3);
  expect(space.records).toBe(1);
});

/** Whether a binary can be run here. A bare name is looked for on the path. */
function runs(bin: string): boolean {
  if (existsSync(bin)) return true;
  return spawnSync(bin, ['--help'], { encoding: 'utf8' }).status === 0;
}

const bin = defaultBin();

describe.skipIf(!runs(bin))('the importer over the committed volume', () => {
  test('writes the bytes that are committed', () => {
    const out = mkdtempSync(join(tmpdir(), 'nex-view-import-'));
    try {
      const run = spawnSync(
        process.execPath,
        [
          join(HERE, 'tools/import-volume.mjs'),
          '--bin',
          bin,
          '--volume',
          'storage/ref/samples/volume',
          '--id',
          'sample-volume',
          '--title',
          'Sample volume',
          '--out',
          out,
        ],
        { encoding: 'utf8', cwd: HERE },
      );
      expect(run.status, run.stderr).toBe(0);

      for (const path of ['scenario/sample-volume.txt', 'projects.json']) {
        expect(readFileSync(join(out, path), 'utf8'), `${path} is stale`).toBe(
          readFileSync(join(HERE, path), 'utf8'),
        );
      }
    } finally {
      rmSync(out, { recursive: true, force: true });
    }
  });
});
