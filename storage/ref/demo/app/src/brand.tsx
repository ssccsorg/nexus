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
  title: 'neXus Reference Demo',
  name: 'nexus reference storage viewer',
  version: 'v0.1.0',
  // This deployment reads a committed volume and never writes, and the bar says so beside the name.
  status: 'read-only',
  // What the bar says about this deployment in two words. The card behind the name says what it is, which is more than a
  // bar can hold.
  note: '',
  // The deployment's vocabulary, which a project inherits when it declares none. The ESG project declares the same three
  // words, and the projects taken from another board declare none, so they are read in these.
  words: { fact: 'evidence', intent: 'calculation', hint: 'requirement' },
  // What a reader is looking at, in the order that answers it: the infrastructure, this view's relation to it, the two
  // questions a reader asks next (how the bytes move, and what becomes of the data already held), and the data. The stack
  // below names Skene, so these lines say what it is rather than naming it twice.
  about: [
    'neXus is the underlying state-space fabric: a deterministic record format and identity space for heterogeneous systems.',
    'This viewer is not related to the infrastructure as an independent demonstration, showcasing what can be built on top of the neXus data model.',
    'Rendered by Skene, a default in-house modular interface library for the data model, reading directly from the reference storage.',
    'Signal and command transport is not this layer: the io seam belongs to chton, and a transport is a consumer of it.',
    'A row of an existing database can become a fact. The mapping is for the consumer to write, and a converter over this contract performs it.',
    'All displayed records are synthetic mock data for demonstration purposes.',
  ],
  // What the screen is made of, bottom up: the reference storage that wrote the records, and the frame that draws them.
  // Each carries the repository it is kept in, which is where a reader who wants the code goes.
  layers: [
    { name: 'nexus', href: 'https://github.com/ssccsorg/nexus' },
    { name: 'skene', href: 'https://github.com/ssccsorg/skene' },
  ],
  Mark,
  mark: { path: MARK, viewBox: MARK_VIEWBOX },
};
