// Copy one project and its run between two boards, in either direction.
//
//   node tools/sync-project.mjs --from <board> --id <project> --into <board> [--replace]
//
// A board is a directory holding `projects.json` and `scenario/`. The project travels whole, its resources and its run,
// because a board that names a run it does not hold is half a project. The copy is symmetric: the command that takes a
// project from another board is the command that gives it back, with the two directories the other way round. That is
// what makes a shared project two-way rather than a vendored snapshot.
//
// Nothing is deleted. A project the destination already holds is left alone unless `--replace` says which side wins, so
// a sync run twice is the second run saying it is already in step.

import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

function main() {
  const args = process.argv.slice(2);
  const option = (name, fallback = null) => {
    const at = args.indexOf(`--${name}`);
    if (at === -1) return fallback;
    const value = args[at + 1];
    if (value === undefined || value.startsWith('--')) throw new Error(`--${name} needs a value`);
    args.splice(at, 2);
    return value;
  };

  const given = option('from');
  const written = option('into');
  const named = option('id');
  if (!given) throw new Error('--from needs the board the project is taken from');
  if (!written) throw new Error('--into needs the board the project is written to');
  const replace = args.includes('--replace');
  if (replace) args.splice(args.indexOf('--replace'), 1);
  if (args.length) throw new Error(`unexpected argument ${args[0]}`);

  const from = resolve(given);
  const into = resolve(written);

  const source = readBoard(from);
  const id = named ?? (source.projects.length === 1 ? source.projects[0].id : null);
  if (!id) throw new Error(`${from} holds ${source.projects.length} projects, so --id is needed`);

  const project = source.projects.find((each) => each.id === id);
  if (!project) {
    const held = source.projects.map((each) => each.id).join(', ');
    throw new Error(`${from} holds no project ${id}; it holds ${held}`);
  }

  const runFrom = join(from, 'scenario', `${id}.txt`);
  if (!existsSync(runFrom)) throw new Error(`${from} holds no run for ${id} at scenario/${id}.txt`);

  // A destination board that does not exist yet is made from the source's own note, so a copy carries the vocabulary of
  // the board it came from rather than a second one written here.
  const target = existsSync(join(into, 'projects.json'))
    ? readBoard(into)
    : { note: source.note, projects: [] };

  const at = target.projects.findIndex((each) => each.id === id);
  const runTo = join(into, 'scenario', `${id}.txt`);
  const held = at === -1 ? null : target.projects[at];
  const runHeld = existsSync(runTo) ? readFileSync(runTo, 'utf8') : null;
  const same = held !== null && runHeld !== null && JSON.stringify(held) === JSON.stringify(project)
    && runHeld === readFileSync(runFrom, 'utf8');

  if (same) {
    console.log(`${id}: already in step between ${from} and ${into}`);
    return;
  }
  if (held !== null && !replace) {
    throw new Error(
      `${into} holds ${id} and it differs; pass --replace to take the copy from ${from}, ` +
        `or run this with the two directories the other way round`,
    );
  }

  if (at === -1) target.projects.push(project);
  else target.projects[at] = project;

  mkdirSync(join(into, 'scenario'), { recursive: true });
  copyFileSync(runFrom, runTo);
  writeFileSync(join(into, 'projects.json'), `${JSON.stringify(target, null, 2)}\n`);
  console.log(`${id}: copied from ${from} into ${into}`);
}

function readBoard(dir) {
  const path = join(dir, 'projects.json');
  if (!existsSync(path)) throw new Error(`no board at ${dir}; ${path} is missing`);
  const board = JSON.parse(readFileSync(path, 'utf8'));
  if (!Array.isArray(board.projects)) throw new Error(`${path} holds no projects`);
  return board;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(`\nsync-project: ${error.message}`);
    process.exit(1);
  }
}
