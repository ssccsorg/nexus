// The campaign the ESG project declares, read over the run it names.
//
// A campaign is a second reading of one record: its objectives are conditions over the labels the run writes, so what a
// reader sees is derived from the work and not from a list kept beside it. What this holds is that the declaration is
// readable at all, that every objective it names is satisfied by the run it is read over, and that it ends where it says.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from 'vitest';

import { bundleOf, parseScenario, type BoardDocument, type ProjectDocument } from '@ssccsorg/skene-frame';
import { campaignAt, campaignsNamed } from '@ssccsorg/skene-kernel';

import '../src/pack';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const board = JSON.parse(readFileSync(`${ROOT}projects.json`, 'utf8')) as BoardDocument;
const document = board.projects.find((project) => project.id === 'esg-scenario') as ProjectDocument;
const bundle = bundleOf(document, board.projects);
const text = readFileSync(`${ROOT}${bundle.record}`, 'utf8');
const parsed = parseScenario(text);
const end = parsed.events[parsed.events.length - 1]?.at ?? 0;

const campaigns = campaignsNamed(bundle.campaigns);

test('the declaration is readable, so the line is drawn at all', () => {
  expect(campaigns.length, 'the project declares a campaign').toBeGreaterThan(0);
});

test('every objective is met by the run it is read over', () => {
  for (const campaign of campaigns) {
    const progress = campaignAt(campaign, parsed.events, end);
    expect(progress.state, `${campaign.id} ends`).toBe('ended');
    expect(progress.failed, `${campaign.id} misses nothing`).toBe(false);
    expect(progress.met, `${campaign.id} meets every objective it declares`).toBe(progress.total);
  }
});

test('an objective is open before the step that satisfies it has arrived', () => {
  // A checklist that is complete at every point of the run would say nothing. The listing is the run's first step, so
  // reading the run at its opening leaves the later objectives open.
  for (const campaign of campaigns) {
    const opening = campaignAt(campaign, parsed.events, end);
    expect(opening.state, `${campaign.id} is read over a run that ends`).toBe('ended');

    const first = parsed.events[0]?.at ?? 0;
    const start = campaignAt(campaign, parsed.events, first);
    expect(start.met, `${campaign.id} is not complete at the opening`).toBeLessThan(
      start.total,
    );
  }
});
