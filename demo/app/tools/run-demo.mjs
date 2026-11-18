// Run the demonstration in one command, on any OS.
//
//   demo/app/run.sh
//   npm --prefix demo/app run demo
//
// The checkout is found by walking up to `.git`, the frame is fetched beside it at skene's main, and cargo, git, and npm
// are invoked by the names the platform ships. Anything this cannot do is reported as the command to run, never as a
// stack trace.
//
// The board is regenerated whole on every run, one project per committed volume and the projects taken from the shared
// board, so a reader never looks at a board that lost a project when the file was written by an earlier one.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const APP = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const WIN = process.platform === 'win32';

/** One project per committed volume, in the order the board lists them. */
const VOLUMES = [
  {
    id: 'esg-scenario',
    title: 'ESG compliance - Alpine Precision AG, 2026-Q1',
    volume: 'demo/esg-volume',
    reading: 'chain',
    reset: true,
  },
  {
    id: 'sample-volume',
    title: 'Sample volume',
    volume: 'demo/volume',
    reading: 'esg',
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

function step(what, command, args, cwd, hint) {
  console.log(`\n${what}`);
  const done = spawnSync(command, args, { cwd, stdio: 'inherit' });
  if (done.error) throw new Error(`${command} could not start (${done.error.message}). ${hint}`);
  if (done.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} exited ${done.status}. ${hint}`);
  }
}

/** Whether the tree holds the packages the lock and the frame describe, or packages from another frame. */
function needsInstall() {
  const installed = join(APP, 'node_modules', '.package-lock.json');
  if (!existsSync(installed)) return true;
  return ['package-lock.json', '.skene-frame'].some((name) => {
    const input = join(APP, name);
    return existsSync(input) && statSync(input).mtimeMs > statSync(installed).mtimeMs;
  });
}

function main() {
  const repo = repoRoot();

  // The frame is fetched rather than left as whatever sits beside this checkout: the board's screen is the frame's, so a
  // run that reported neither its commit nor its age would be showing a frame nobody else runs.
  step('taking the frame', process.execPath, [join(APP, 'tools', 'frame.mjs')], APP,
    'skene is read from the network on every run: check the network, then run this again.');

  step('building the reference tool', EXE('cargo'), ['build', '-p', 'nex-refstorage'], repo,
    'Install a Rust toolchain from https://rustup.rs, then run this again.');

  // The frame's packages are packed into the tree, so a copy made from another commit stays stale until it is replaced:
  // the stamp the fetch writes is newer than the install exactly when the frame moved.
  if (needsInstall()) {
    step('installing the application', NPM, ['ci'], APP,
      'Install Node 20 or newer from https://nodejs.org, or delete app/node_modules and run this again.');
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
    ], APP,
      'The tool printed the reason above. Run the same command by hand to see it in full.');
  }

  // A project from another board is not a volume, so it arrives by copy rather than by import, and it is applied after
  // the imports because the first import starts the board fresh. Its source is committed beside this application, so a
  // run of this script is the same board on every machine.
  step('taking the shared projects', process.execPath, [
    join(APP, 'tools', 'sync-project.mjs'),
    '--from',
    join(APP, '..', 'shared'),
    '--all',
    '--into',
    APP,
  ], APP, 'The shared projects are kept at demo/shared; copy their files in if they are missing.');

  console.log('\nserving the board, with the address Vite prints below');
  const dev = spawn(NPM, ['run', 'dev'], { cwd: APP, stdio: 'inherit' });
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => dev.kill(signal));
  dev.on('exit', (code) => {
    if (code) {
      console.error(
        '\nrun-demo: the server exited ' +
          code +
          '. If the port was taken, end the earlier run, then start this again.',
      );
    }
    process.exit(code ?? 0);
  });
}

try {
  main();
} catch (error) {
  console.error(`\nrun-demo: ${error.message}`);
  process.exit(1);
}
