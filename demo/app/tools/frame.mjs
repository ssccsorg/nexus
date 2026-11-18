// The frame, at the commit skene's own main is on.
//
// This board is a deployment of skene, and a pinned copy draws with a frame nobody else runs: the find field, the cell
// outline, and the strip a reader sees are the frame's, so a board built against a checkout from months ago is a screen
// skene no longer produces. Every install therefore fetches, and the run prints the commit it built from.
//
// The checkout is left on the fetched commit and nothing else: local work in the frame is refused rather than erased,
// because the frame is a repository of its own and a board has no business discarding it.
import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const FRAME = fileURLToPath(new URL('../../../../skene', import.meta.url));
const REF = process.env.SKENE_REF || 'main';
const REMOTE = process.env.SKENE_URL || 'https://github.com/ssccsorg/skene.git';
// Written only when the commit moves, so a run can tell that the packages it installed were packed from another frame.
const STAMP = join(ROOT, '.skene-frame');

/** A git run that answers from its output. */
const git = (...args) =>
  execFileSync('git', ['-C', FRAME, ...args], { encoding: 'utf8' }).trim();

/** A git run that changes the checkout, named by what it was doing when git's own message comes from git. */
function run(args, failure) {
  try {
    return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] });
  } catch {
    throw new Error(failure);
  }
}

function clone() {
  run(['clone', '--depth', '1', '--branch', REF, REMOTE, FRAME], `the clone from ${REMOTE} failed`);
}

function main() {
  if (!existsSync(join(FRAME, '.git'))) {
    if (existsSync(FRAME)) {
      throw new Error(`${FRAME} is not a git checkout; move it aside and run this again`);
    }
    clone();
  } else if (lstatSync(join(FRAME, '.git')).isFile()) {
    // A submodule records its commit in another repository's index, which is the pin this script exists to remove.
    rmSync(FRAME, { recursive: true, force: true });
    clone();
  } else {
    if (git('status', '--porcelain') !== '') {
      throw new Error(
        `${FRAME} has uncommitted changes; commit or discard them before the frame moves to ${REF}`,
      );
    }
    const depth = git('rev-parse', '--is-shallow-repository') === 'true' ? ['--depth', '1'] : [];
    run(
      ['-C', FRAME, 'fetch', ...depth, 'origin', REF],
      `the fetch of ${REF} from ${REMOTE} failed`,
    );
    // A checkout on another branch is moved without it: resetting there would move that branch's own pointer.
    const on = git('rev-parse', '--abbrev-ref', 'HEAD');
    run(
      on === REF
        ? ['-C', FRAME, 'reset', '--hard', 'FETCH_HEAD']
        : ['-C', FRAME, 'checkout', '--detach', 'FETCH_HEAD'],
      `the checkout could not be moved to ${REF}`,
    );
  }

  const commit = git('rev-parse', 'HEAD');
  const before = existsSync(STAMP) ? readFileSync(STAMP, 'utf8').trim() : '';
  if (before !== commit) writeFileSync(STAMP, `${commit}\n`);
  const moved = before && before !== commit ? `, was ${before.slice(0, 12)}` : '';
  console.log(`frame: ${FRAME} at ${commit.slice(0, 12)} (${REF}${moved})`);
}

try {
  main();
} catch (error) {
  // A step in a run or an install reports the one thing to fix, and git's own message is already above it.
  console.error(`frame: ${error.message}`);
  process.exitCode = 1;
}
