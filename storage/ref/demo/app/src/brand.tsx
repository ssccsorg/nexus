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
      aria-label="nexus storage demo"
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
  title: 'REF STORAGE DEMO',
  name: 'nexus storage demo',
  version: 'v0.1.0',
  // This deployment reads a committed volume and never writes, and the bar says so beside the name.
  status: 'read-only',
  // The board is one arrangement of the records, drawn by this application over the frame. neXus defines no view.
  note: 'not a neXus view · mock data',
  // The deployment's vocabulary, which a project inherits when it declares none. The ESG project declares the same three
  // words, and the project taken from another board declares none, so it is read in these.
  words: { fact: 'evidence', intent: 'calculation', hint: 'requirement' },
  // What a reader is looking at, in the order that answers it: the infrastructure, this view's relation to it, and the
  // data. The stack below names Skene, so these lines say what it is rather than naming it twice.
  about: [
    'neXus is a state-space fabric: one format and one identity space, joining heterogeneous systems.',
    'This view is not part of neXus. It is one example of what an implementation on that infrastructure can build.',
    'Nothing here is neXus-specific: the view is built with Skene, a general-purpose interface library.',
    'Every record on the board is fictional mock data.',
  ],
  // The surface stands on the frame and nothing else: the board, the router, and the panels are skene packages.
  layers: ['skene'],
  Mark,
  mark: { path: MARK, viewBox: MARK_VIEWBOX },
};
