// The graph the chain reading draws, held to the records it draws it from.
//
// An edge is a claim about how two records are related, and a claim a record does not make is a drawing saying more
// than the volume does. So every edge is checked against a field, and a requirement no conclusion cites is checked to
// draw nothing: the two requirements that are on file and unanswered are the case that would otherwise be invented.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

import { chainOf, codeOf } from '../src/chain';
import { concludedOf, roleOf, type EsgRecord } from '../src/pack';

const ROOT = fileURLToPath(new URL('../', import.meta.url));

const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as {
  projects: { id: string; resources: { kind: string; at: unknown }[] }[];
};

const records = (
  board.projects.find((project) => project.id === 'esg-scenario')?.resources.find(
    (resource) => resource.kind === 'esg',
  )?.at as { records: EsgRecord[] } | undefined
)?.records;

const ordered = [...(records ?? [])].sort((left, right) => left.written - right.written);
const chain = chainOf(ordered);

const concluded = new Set(
  ordered.map((record) => concludedOf(record)).filter((at): at is string => Boolean(at)),
);

test('every record is a node, in the place the records put it', () => {
  expect(chain.nodes).toHaveLength(ordered.length);
  expect(chain.nodes.map((node) => node.role)).toEqual(
    ordered.map((record) => roleOf(record, concluded)),
  );
  const counted = (role: string) => chain.nodes.filter((node) => node.role === role).length;
  expect({
    requirement: counted('requirement'),
    measurement: counted('measurement'),
    transformation: counted('transformation'),
    result: counted('result'),
  }).toEqual({ requirement: 3, measurement: 7, transformation: 3, result: 3 });
});

test('a source is evidence for the transformation that read it, and nothing else is', () => {
  const evidence = chain.links.filter((link) => link.role === 'evidence');
  const stated = ordered.flatMap((record) =>
    record.kind === 'intent'
      ? ((record.record as { from_facts?: string[] }).from_facts ?? []).map((source) => [
          source,
          record.address,
        ])
      : [],
  );
  expect(evidence.map((link) => [link.from, link.to])).toEqual(stated);
  for (const link of evidence) {
    const source = chain.nodes.find((node) => node.id === link.from);
    const work = chain.nodes.find((node) => node.id === link.to);
    expect(source?.role, 'a source is an observation or an earlier result').not.toBe('requirement');
    expect(work?.role, 'the work an edge runs into is a transformation').toBe('transformation');
  }
});

test('a transformation reaches the result it concluded', () => {
  const conclusions = chain.links.filter((link) => link.role === 'conclusion');
  expect(conclusions.map((link) => [link.from, link.to])).toEqual(
    ordered
      .filter((record) => record.kind === 'intent')
      .map((record) => [record.address, concludedOf(record)]),
  );
  for (const link of conclusions) {
    expect(chain.nodes.find((node) => node.id === link.to)?.role).toBe('result');
  }
});

test('the requirement a result cites bounds the work that reached it', () => {
  // The conclusion states which requirement it answers; the work that produced it is the work that requirement
  // bounded. Both ends come from records: the citation from the result, the work from the intent's own conclusion.
  const bounds = chain.links.filter((link) => link.role === 'bounds');
  expect(bounds).toHaveLength(3);
  for (const link of bounds) {
    const requirement = chain.nodes.find((node) => node.id === link.from);
    const work = chain.nodes.find((node) => node.id === link.to);
    expect(requirement?.role).toBe('requirement');
    expect(requirement?.title).toBe('ESRS E1-6');
    expect(work?.role).toBe('transformation');

    const intent = ordered.find((record) => record.address === link.to);
    const result = ordered.find((record) => record.address === concludedOf(intent ?? ordered[0]));
    expect(result?.payload?.requirement, 'the citation the edge is drawn from').toBe(requirement?.title);
  }
});

test('a requirement no conclusion cites draws no edge', () => {
  // Two requirements are on file and no result answers them. An edge there would be the drawing promising coverage the
  // volume does not have, which is the one claim an audit may not make.
  const answered = new Set(chain.links.filter((link) => link.role === 'bounds').map((link) => link.from));
  const requirements = chain.nodes.filter((node) => node.role === 'requirement');
  expect(requirements).toHaveLength(3);
  expect(requirements.filter((node) => !answered.has(node.id)).map((node) => node.title)).toEqual([
    'ESRS E3-4',
    'ESRS E5-5',
  ]);
});

test('no edge names a record the volume does not hold', () => {
  const known = new Set(chain.nodes.map((node) => node.id));
  for (const link of chain.links) {
    expect(known.has(link.from), `${link.from} is a record`).toBe(true);
    expect(known.has(link.to), `${link.to} is a record`).toBe(true);
  }
});

test('a node is titled by a field the record holds, never by its address', () => {
  // The drawing exists so a reader can follow the chain rather than the hashes, so a node that fell back to an address
  // would be the panel it replaced.
  for (const node of chain.nodes) {
    expect(node.title.trim()).not.toBe('');
    expect(node.title).not.toBe(node.id);
  }
  const reported = chain.nodes.filter((node) => node.role === 'measurement').map((node) => node.title);
  expect(reported).toContain('meter:m17');
  expect(reported).toContain('service:contractor');
});

test('a requirement is titled by the code it is cited by', () => {
  expect(codeOf('ESRS E1-6: disclose gross Scope 1 and Scope 2')).toBe('ESRS E1-6');
  expect(codeOf('a requirement with no colon')).toBe('a requirement with no colon');
});
