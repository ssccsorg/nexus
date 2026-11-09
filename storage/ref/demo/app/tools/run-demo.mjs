// Run the demonstration in one command, on any OS.
//
//   storage/ref/demo/app/run.sh
//   npm --prefix storage/ref/demo/app run demo
//
// The checkout is found by walking up to `.git`, the frame is cloned beside it when it is absent, and cargo, git, and npm
// are invoked by the names the platform ships. Anything this cannot do is reported as the command to run, never as a
// stack trace.
//
// The board is regenerated whole on every run, one project per committed volume, so a reader never looks at a board
// that lost a project when the file was written by an earlier one.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const APP = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const WIN = process.platform === 'win32';

/** One project per committed volume, in the order the board lists them. */
const VOLUMES = [
  {
    id: 'esg-scenario',
    title: 'ESG compliance - Alpine Precision AG, 2026-Q1',
    volume: 'storage/ref/demo/esg-volume',
    reading: 'esg',
    reset: true,
  },
  {
    id: 'sample-volume',
    title: 'Sample volume',
    volume: 'storage/ref/demo/volume',
    reading: 'keyspace',
  },
];

/** Cargo and git are executables; npm is a script, and Windows reaches it through the shell shim. */
const EXE = (name) => (WIN ? `${name}.exe` : name);
const NPM = WIN ? 'npm.cmd' : 'npm';

function repoRoot() {
  let dir = APP;
  while (!existsSync(join(dir, '.git'))) {
    const parent = dirname(dir);
    if (parent === dir) throw new Error(`no checkout above ${APP}`);
    dir = parent;
  }
  return dir;
}

function step(what, command, args, cwd) {
  console.log(`\n${what}`);
  const done = spawnSync(command, args, { cwd, stdio: 'inherit' });
  if (done.error) throw new Error(`could not run ${command}: ${done.error.message}`);
  if (done.status !== 0) throw new Error(`${command} ${args.join(' ')} exited ${done.status}`);
}

function main() {
  const repo = repoRoot();
  const beside = join(dirname(repo), 'skene');

  if (!existsSync(join(beside, 'packages', 'frame'))) {
    step(
      'the frame is not beside this checkout, so it is cloned from upstream',
      EXE('git'),
      ['clone', '--depth', '1', 'https://github.com/ssccsorg/skene', beside],
      repo,
    );
  }

  step('building the reference tool', EXE('cargo'), ['build', '-p', 'nexus-storage-ref'], repo);

  // A stale tree is the one case the install cannot heal, so it is left to the reader rather than deleted here.
  if (!existsSync(join(APP, 'node_modules'))) {
    step('installing the application', NPM, ['install'], APP);
  }

  // The first import starts a fresh board and the rest add to it. A board written by an earlier run is replaced rather
  // than appended to, because an append cannot restore a project that run never had.
  for (const each of VOLUMES) {
    step(`importing ${each.volume}`, process.execPath, [
      join(APP, 'tools', 'import-volume.mjs'),
      '--volume',
      each.volume,
      '--id',
      each.id,
      '--title',
      each.title,
      '--reading',
      each.reading,
      ...(each.reset ? ['--reset'] : []),
    ], APP);
  }

  console.log('\nserving the board, with the address Vite prints below');
  const dev = spawn(NPM, ['run', 'dev'], { cwd: APP, stdio: 'inherit' });
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => dev.kill(signal));
  dev.on('exit', (code) => process.exit(code ?? 0));
}

try {
  main();
} catch (error) {
  console.error(`\nrun-demo: ${error.message}`);
  process.exit(1);
}
