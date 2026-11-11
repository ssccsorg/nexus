// The deployment's own mark, held to what the frame draws it from.
//
// The frame draws `mark.path` in a `mark.viewBox` and `Mark` in the bar. The two are one drawing, so they are held equal
// here: a mark edited in one place and not the other is a tab and a bar showing two logos.

import { expect, test } from 'vitest';

import { MARK, MARK_VIEWBOX, brand } from '../src/brand';

test('the mark is a path in a view box', () => {
  expect(MARK.startsWith('M'), 'a path starts at a point').toBe(true);
  expect(MARK, 'the path holds drawing commands and numbers only').toMatch(
    /^[MmLlHhVvCcSsQqTtAaZz0-9.,\-\s]+$/,
  );
  expect(MARK_VIEWBOX, 'the box is a positive square').toBeGreaterThan(0);
});

test('the brand the bar draws is this deployment’s', () => {
  expect(brand.title).toBe('REF STORAGE DEMO');
  expect(brand.name).toBe('nexus storage demo');
  expect(brand.version).toMatch(/^v\d+\.\d+\.\d+$/);
  expect(brand.layers, 'the storage and the frame, bottom up').toStrictEqual(['nexus', 'skene']);
});

test('the bar says the board is not a neXus view', () => {
  expect(brand.note, 'the boundary is on the bar').toContain('not a neXus view');
  expect(brand.note, 'so is the invented data').toContain('mock data');
});

test('the card says what this view is, and that its data is invented', () => {
  const card = (brand.about ?? []).join(' ');
  expect(card, 'the infrastructure is named').toContain('state-space fabric');
  expect(card, 'the view is not part of it').toContain('not part of neXus');
  expect(card, 'the frame is a general-purpose library').toContain(
    'Skene, a general-purpose interface library',
  );
  expect(card, 'the records are invented').toContain('fictional mock data');
});

test('the mark the frame draws and the mark the bar draws are one drawing', () => {
  expect(brand.mark.path).toBe(MARK);
  expect(brand.mark.viewBox).toBe(MARK_VIEWBOX);
  expect(typeof brand.Mark, 'the bar takes a component').toBe('function');
});
