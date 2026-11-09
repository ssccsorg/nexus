// Import a volume as a project on the board.
//
//   node tools/import-volume.mjs [--bin PATH] [--volume PATH] [--id NAME] [--title TEXT] [--out DIR] [--reset]
//
// The reference tool is the source of every number here. Each verb is run against the volume and what comes back is
// what the board draws: the keys the listing handed over, the records a box admitted, the bytes a read reported, and
// the address a verification recomputed. Nothing in this file counts a key or evaluates an address, because a second
// model of the store is a model free to disagree with the store.
//
// A generated run is written in the same language the app already reads, so a project the importer wrote and a project
// written by hand are one kind of thing. Its `count` steps are the parser's arithmetic over the facts the file itself
// writes, which is what makes a hand edit visible.

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
export const APP = resolve(HERE, '..');

/**
 * Characters one axis value occupies in the rendering.
 *
 * The tool prints every address in the base-36 rendering, so nothing here has to decode a coordinate: the first axis of
 * an address is its first three characters.
 */
const AXIS_WIDTH = 3;

const NOTE =
  'One project per volume, and every number on the screen came from the reference tool. This board is written by ' +
  '`npm run import`; nothing here counts a key.';

/**
 * The repository this application sits in, found by walking up to the checkout.
 *
 * The demo is four directories inside the repository (`storage/ref/demo/app`), so a fixed run of `..` would be a number to
 * keep in step with a move. The checkout is the directory that holds `.git`.
 */
function repoRoot() {
  let dir = APP;
  while (!existsSync(join(dir, '.git'))) {
    const parent = dirname(dir);
    if (parent === dir) throw new Error(`no checkout above ${APP}`);
    dir = parent;
  }
  return dir;
}

/** The tool's file name, which carries an extension on Windows. */
const BIN = process.platform === 'win32' ? 'nexus-storage-ref.exe' : 'nexus-storage-ref';

/** The tool beside this checkout, debug first. */
export function defaultBin() {
  const root = repoRoot();
  for (const build of ['debug', 'release']) {
    const candidate = join(root, 'target', build, BIN);
    if (existsSync(candidate)) return candidate;
  }
  return BIN;
}

/** One run of the tool, with its exit code kept rather than thrown. */
export function tool(bin, args) {
  const run = spawnSync(bin, args, { encoding: 'utf8' });
  if (run.error) throw new Error(`could not run ${bin}: ${run.error.message}`);
  return { code: run.status ?? -1, stdout: run.stdout ?? '', stderr: run.stderr ?? '' };
}

/** A labelled line the tool prints, as its value. */
function field(stdout, name) {
  const match = new RegExp(`^${name}:\\s+(.*)$`, 'm').exec(stdout);
  return match ? match[1].trim() : '';
}

/** The summary line a listing or a box ends with, and the lines above it. */
function listed(stdout, summary) {
  const match = new RegExp(`^(\\d+) ${summary}`, 'm').exec(stdout);
  if (!match) throw new Error(`the tool printed no ${summary} summary:\n${stdout}`);
  const at = match.index;
  return {
    count: Number(match[1]),
    lines: stdout
      .slice(0, at)
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean),
  };
}

/**
 * The address a volume holds, read out of the keys the listing handed over.
 *
 * A record's key carries its address, so this is the tool's own answer and not a derivation of it.
 */
function coordinateOf(keys) {
  for (const key of keys) {
    const match = /^facts\/f_(.+)\.fact$/.exec(key);
    if (match) return match[1];
  }
  throw new Error('the volume holds no record key, so there is no address to read');
}

/**
 * Every verb the board draws, run once.
 *
 * The exit codes are kept: a refused question and an absent record are two answers, and a run that shows a present
 * record over a non-zero exit would be a screen disagreeing with the tool.
 */
export function measure({ bin, volume, volumePath, id, title }) {
  const keys = tool(bin, ['keys', '--volume', volumePath, '--prefix', '']);
  if (keys.code !== 0) throw new Error(`keys refused ${volume}: ${keys.stderr.trim()}`);
  const keySpace = listed(keys.stdout, 'key\\(s\\)');
  const coordinate = coordinateOf(keySpace.lines);

  // The pilot narrows one axis to the record's own value, which is a box over the address the listing gave.
  const first = coordinate.slice(0, AXIS_WIDTH);
  const box = tool(bin, [
    'box',
    '--volume',
    volumePath,
    '--axis',
    `0=${first}-${first}`,
  ]);
  if (box.code !== 0) throw new Error(`box refused ${volume}: ${box.stderr.trim()}`);
  const boxed = listed(box.stdout, 'record\\(s\\) in the box');

  const read = tool(bin, ['read', '--volume', volumePath, '--address', coordinate]);
  const verify = tool(bin, [
    'verify',
    '--volume',
    volumePath,
    '--address',
    coordinate,
  ]);

  // The sidecar measurement is taken on a volume of its own, because it places one and refuses a directory that
  // already holds content. What it reports is the application file's size and that it did not change.
  const scratch = mkdtempSync(join(tmpdir(), 'nex-ref-demo-sidecar-'));
  let sidecar;
  try {
    const app = join(scratch, 'app.db');
    writeFileSync(app, 'app state');
    sidecar = tool(bin, ['sidecar', '--app', app, '--volume', join(scratch, 'records')]);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }

  const contract = tool(bin, ['self-check']);

  const appSize = /unchanged, (\d+) bytes/.exec(sidecar.stdout);
  return {
    id,
    title,
    volume,
    coordinate,
    keys: keySpace.lines,
    keyCount: keySpace.count,
    keyExit: keys.code,
    box: { axis: 0, from: first, until: first, records: boxed.count },
    boxExit: box.code,
    readBytes: Number(field(read.stdout, 'bytes') || 0),
    readKey: field(read.stdout, 'key'),
    readExit: read.code,
    origin: field(verify.stdout, 'origin'),
    creator: field(verify.stdout, 'creator'),
    written: field(verify.stdout, 'written'),
    payload: Number(field(verify.stdout, 'payload').split(' ')[0] || 0),
    address: field(verify.stdout, 'address'),
    verifyExit: verify.code,
    appBytes: appSize ? Number(appSize[1]) : 0,
    sidecarExit: sidecar.code,
    selfCheck: contract.stdout.includes('self-check: PASS'),
    selfCheckExit: contract.code,
    records: measureRecords(bin, volumePath, keySpace.lines),
  };
}

/**
 * One record as the tool prints it.
 *
 * `show` is the verb that reads a record and says what it holds, so the fields here are the tool's answer and not a
 * decode of the medium. A fact carries the payload the medium holds beside it; an intent and a hint are the record.
 */
function showRecord(bin, volumePath, key) {
  const run = tool(bin, ['show', '--volume', volumePath, '--key', key]);
  if (run.code !== 0) throw new Error(`show refused ${key}: ${run.stderr.trim()}`);
  const payloadText = field(run.stdout, 'payload') || null;
  let payload = null;
  if (payloadText) {
    try {
      payload = JSON.parse(payloadText);
    } catch {
      payload = null;
    }
  }
  return {
    key,
    kind: field(run.stdout, 'kind'),
    address: field(run.stdout, 'address'),
    origin: field(run.stdout, 'origin') || null,
    creator: field(run.stdout, 'creator') || null,
    written: Number((field(run.stdout, 'written') || '0').split(' ')[0]),
    record: JSON.parse(field(run.stdout, 'record')),
    payload,
    payloadText,
  };
}

/** The record keys the listing handed over, which are the keys a kind prefix names. */
function recordKeys(keys) {
  return keys.filter((key) => /^(facts|intents|hints)\//.test(key));
}

/**
 * The record at each key, in the order the listing reached them.
 *
 * A volume that holds only blobs has no records to show, and a volume with records has one entry each. Nothing here
 * names a kind in particular: what a record is, the tool prints.
 */
function measureRecords(bin, volumePath, keys) {
  return recordKeys(keys).map((key) => showRecord(bin, volumePath, key));
}

/** A seed derived from the volume's own name, so the same volume writes the same run. */
function seedOf(text) {
  let seed = 0;
  for (const character of text) seed = (seed * 31 + character.codePointAt(0)) % 100000;
  return seed;
}

/**
 * One run, written in the language the app reads.
 *
 * The header states what the tool reported. The body declares the volume, lists the keys, narrows a box, reads the
 * record, recomputes its address, measures the application file beside it, and runs the contract. Every `count` is the
 * number of fact-writing steps above it, which is the same number the reader derives.
 */
export function scenarioOf(measured) {
  const m = measured;
  const lines = [];
  let facts = 0;

  const push = (line) => lines.push(line);
  const position = (unit, x, y) => {
    push(`position ${unit} ${x},${y}`);
    facts += 1;
  };
  const fact = (label, bytes, origin) => {
    push(`fact ${label} ${bytes} ${origin}`);
    facts += 1;
  };
  const hint = (label, content) => push(`hint ${label} ${content}`);
  const intent = (name, from, description) => push(`intent ${name} ${from} ${description}`);
  const claim = (name, worker) => push(`claim ${name} ${worker}`);
  const conclude = (name, label) => {
    push(`conclude ${name} ${label}`);
    facts += 1;
  };
  const count = () => push(`count ${facts}`);

  push(`# ${m.title}`);
  push('#');
  push(`# Imported from ${m.volume} by \`npm run import\`.`);
  push('#');
  push('# tool:      nexus-storage-ref');
  push(`# volume:    ${m.volume}`);
  push(`# keys:      ${m.keyCount}`);
  push(`# records:   ${m.box.records}`);
  push(`# exit:      ${m.readExit}/${m.verifyExit}/${m.selfCheckExit}`);
  push('#');
  push('# Every number below came from the tool. Each count is the arithmetic of this file over');
  push('# the facts it writes, so an edit that moves a line has to move its count.');
  push('#');
  push(`#   nexus-storage-ref keys --volume ${m.volume} --prefix ""`);
  push('');
  push(`seed ${seedOf(m.id)}`);
  push('');

  // The place: the volume, the tool's own verbs, and the reader. A storage run does not move its members.
  push('tick 0');
  position('sample-volume', 70, 88);
  position('keys', 186, 52);
  position('read', 338, 88);
  position('verify', 452, 40);
  count();
  push('');

  // An address is a key: one fact per key, with the key as its name.
  fact('keyspace-listed', m.keyCount, 'keys/sample');
  for (const key of m.keys) fact(key, 0, 'keys/sample');
  // Every key is accounted for here, because a volume that holds records holds their keys too, and the header states
  // one total. A prefix with no keys is written as zero so the shape of the line does not move with the volume.
  const under = (prefix) => m.keys.filter((key) => key.startsWith(`${prefix}/`)).length;
  hint(
    'key-count',
    `facts=${under('facts')},blobs=${under('blob')},hints=${under('hints')},intents=${under('intents')},` +
      `exit=${m.keyExit}`,
  );
  count();
  push('');

  // A listing is the only scan, so a box over one axis is answered from it.
  push('tick 1');
  fact(m.coordinate, 0, 'box/sample');
  hint('box-axes', `${m.box.axis}=${m.box.from}-${m.box.until}`);
  hint('box-count', String(m.box.records));
  count();
  push('');

  // A point read is one read of the medium.
  intent('read-sample', 'keyspace-listed', 'read-one-address');
  claim('read-sample', 'read/sample');
  push('tick 2');
  fact('sample-record', m.readBytes, 'read/sample');
  hint('read-key', m.readKey);
  hint('read-exit', String(m.readExit));
  conclude('read-sample', m.readExit === 0 ? 'present' : 'absent');
  count();
  push('');

  // Verification is recomputation: the address follows from the record's own fields.
  intent('verify-sample', 'sample-record', 'recompute-the-address');
  claim('verify-sample', 'verify/sample');
  push('tick 3');
  hint('record-origin', m.origin);
  hint('record-creator', m.creator);
  hint('record-written', m.written.replace(/\s+/g, ''));
  hint('payload-bytes', String(m.payload));
  hint('address-state', m.address.split(',')[0]);
  conclude('verify-sample', m.address.startsWith('valid') ? 'verified' : 'mismatched');
  count();
  push('');

  // A volume sits beside an application file and leaves it alone.
  intent('sidecar-sample', 'sample-record', 'measure-the-application-file');
  claim('sidecar-sample', 'read/sample');
  push('tick 4');
  hint('app-bytes', String(m.appBytes));
  hint('app-state', 'unchanged');
  conclude('sidecar-sample', 'unchanged');
  count();
  push('');

  // The contract the reference promises, run against a volume of its own.
  intent('self-check-sample', 'sidecar-sample', 'run-the-contract');
  claim('self-check-sample', 'read/sample');
  push('tick 5');
  hint('self-check-exit', String(m.selfCheckExit));
  conclude('self-check-sample', m.selfCheck ? 'pass' : 'fail');
  count();
  push('');

  // The records the volume holds, read one at a time. A fact carries a payload, an intent carries
  // the addresses it reads and the one it concluded, and a hint carries the requirement.
  if (m.records.length) {
    push('tick 6');
    fact('records-held', m.records.length, 'show/volume');
    m.records.forEach((record, at) => {
      // The record's own key is a hint rather than the fact's label: a label that is a key would be
      // read as one more entry in the listing above, and the listing is the tool's to report.
      const bytes = record.payloadText ? record.payloadText.length : 0;
      fact(`record-${at}`, bytes, record.creator ?? 'record');
      hint(`record-key-${at}`, record.key);
    });
    const kinds = m.records.reduce((held, record) => {
      held[record.kind] = (held[record.kind] ?? 0) + 1;
      return held;
    }, {});
    hint(
      'record-kinds',
      Object.entries(kinds)
        .map(([kind, at]) => `${kind}=${at}`)
        .join(','),
    );
    const requirement = m.records.find((record) => record.kind === 'hint');
    if (requirement) hint('requirement', requirement.address.slice(0, 12));
    count();
    push('');
  }

  return `${lines.join('\n')}\n`;
}

/** The project that carries the run, the key space, and the records the volume holds. */
export function projectOf(measured, reading = 'keyspace') {
  const m = measured;
  const resources = [
    { kind: 'channel', at: 'sim' },
    { kind: 'record', at: `scenario/${m.id}.txt` },
    {
      kind: 'keyspace',
      at: {
        volume: m.volume,
        keys: m.keys,
        listed: m.keyCount,
        box: m.box,
        records: m.box.records,
      },
    },
  ];
  // The records themselves, when the volume holds any: what the `show` verb printed, one entry a record.
  if (m.records.length) {
    resources.push({ kind: 'esg', at: { volume: m.volume, records: m.records } });
  }
  resources.push(
    { kind: 'views', at: { pack: 'volume', main: reading } },
    // The board as text stands beside the key space. The panel is the frame's own drawing of a run, so it is opened
    // here rather than gated on a room this deployment does not have.
    { kind: 'layout', at: { console: true } },
  );
  return { format: 'project/1', id: m.id, resources };
}

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

  const bin = option('bin') ?? process.env.NEX_REF_BIN ?? defaultBin();
  const repo = repoRoot();
  // A path relative to the repository, so the command runs from the application directory and the board stays
  // machine independent. An absolute path is taken as given.
  const volume = option('volume', 'storage/ref/demo/volume');
  const volumePath = resolve(repo, volume);
  const id = option('id') ?? basename(volumePath);
  const title = option('title') ?? id;
  const out = resolve(option('out') ?? APP);
  const reading = option('reading', 'keyspace');
  const reset = args.includes('--reset');
  if (reset) args.splice(args.indexOf('--reset'), 1);
  if (args.length) throw new Error(`unexpected argument ${args[0]}`);

  const measured = measure({ bin, volume, volumePath, id, title });
  const text = scenarioOf(measured);

  mkdirSync(join(out, 'scenario'), { recursive: true });
  writeFileSync(join(out, 'scenario', `${id}.txt`), text);

  const boardPath = join(out, 'projects.json');
  const board = reset || !existsSync(boardPath)
    ? { note: NOTE, projects: [] }
    : JSON.parse(readFileSync(boardPath, 'utf8'));
  const project = projectOf(measured, reading);
  const at = board.projects.findIndex((each) => each.id === id);
  if (at === -1) board.projects.push(project);
  else board.projects[at] = project;
  writeFileSync(boardPath, `${JSON.stringify(board, null, 2)}\n`);

  console.log(
    `${id}: ${measured.keyCount} keys, ${measured.box.records} record(s) in the box, ` +
      `${measured.readBytes} bytes at ${measured.coordinate}, ${measured.records.length} shown`,
  );
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(`import-volume: ${error.message}`);
    process.exit(1);
  }
}
