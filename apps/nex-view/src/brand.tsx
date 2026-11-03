import type { Brand } from '@ssccsorg/skene-frame';

/** A key: the hole the rule turns on, and the stem the address is written along. */
export const MARK =
  'M6 16a5 5 0 1 0 10 0 5 5 0 1 0-10 0M16 16h11M22 16v4M26 16v3';
export const MARK_VIEWBOX = 32;

function Mark({ className }: { className?: string }) {
  return (
    <svg
      viewBox={`0 0 ${MARK_VIEWBOX} ${MARK_VIEWBOX}`}
      role="img"
      aria-label="neXus storage"
      className={className}
    >
      <path
        d={MARK}
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}

export const brand: Brand = {
  title: 'NEXUS',
  name: 'neXus storage',
  version: 'v0.1.0',
  layers: ['nexus', 'chton', 'skene'],
  Mark,
  mark: { path: MARK, viewBox: MARK_VIEWBOX },
};
