// The key space, drawn as the storage layer's own picture.
//
// A key space is where a record is kept, and the frame's own readings are named for what a record is. So this arrives as
// a pack: one resource kind and one reading, declared by the project that wants it, and the frame learns neither. The
// numbers on the screen are the ones the reference tool printed, carried in the resource by the importer rather than
// counted here.

import { registerPack, registerResource, type Board } from '@ssccsorg/skene-frame';
import { LABEL, ROW, TITLE, cn } from '@ssccsorg/skene-frame/ui';

/** What a project declares under `{ "kind": "keyspace", "at": ... }`. */
export interface KeySpace {
  /** The volume the keys were read from, as the importer was given it. */
  volume: string;
  /** Every key the listing handed over, in the order the channel listed them. */
  keys: string[];
  /** How many keys the tool reported, which is its number and not this list's length. */
  listed: number;
  /** The records the tool reported for a box over one axis, when the run asked for one. */
  box: { axis: number; from: string; until: string; records: number } | null;
  /** How many record keys the volume holds. */
  records: number;
}

/** The keys under the prefix each one names, in the order the listing reached them. */
function grouped(keys: string[]): { prefix: string; keys: string[] }[] {
  const byPrefix = new Map<string, string[]>();
  for (const key of keys) {
    const at = key.indexOf('/');
    const prefix = at === -1 ? '' : key.slice(0, at + 1);
    const held = byPrefix.get(prefix);
    if (held) held.push(key);
    else byPrefix.set(prefix, [key]);
  }
  return [...byPrefix].map(([prefix, held]) => ({ prefix, keys: held }));
}

function KeyMark() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round">
      <circle cx="8" cy="12" r="4" />
      <path d="M12 12h8M17 12v3M20 12v2" />
    </svg>
  );
}

function KeySpaceView({ project }: Board) {
  const space = (project as { keyspace?: KeySpace } | null)?.keyspace;
  if (!space) {
    return (
      <div className={cn(ROW, 'text-muted-foreground')}>this project declares no key space</div>
    );
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className={cn(ROW, 'gap-2 border-b')}>
        <span className={cn(LABEL, 'shrink-0')}>Key space</span>
        <span className="min-w-0 flex-1 truncate font-mono text-muted-foreground" title={space.volume}>
          {space.volume}
        </span>
        <span className="shrink-0 tabular-nums text-muted-foreground">{space.listed} keys</span>
        <span className="shrink-0 tabular-nums text-muted-foreground">
          {space.records} {space.records === 1 ? 'record' : 'records'}
        </span>
      </div>

      <div data-region="keyspace" className="min-h-0 flex-1 overflow-y-auto">
        {grouped(space.keys).map((group) => (
          <div key={group.prefix}>
            <div className={cn(ROW, 'gap-2 border-b bg-muted/30')}>
              <span className={cn(LABEL)}>{group.prefix}</span>
              <span className="ml-auto tabular-nums text-muted-foreground">{group.keys.length}</span>
            </div>
            {group.keys.map((key) => (
              <div key={key} className={cn(ROW, 'gap-2')}>
                <span className={cn(TITLE, 'min-w-0 truncate font-mono')}>
                  {key.slice(group.prefix.length)}
                </span>
              </div>
            ))}
          </div>
        ))}
      </div>

      {space.box && (
        <div className={cn(ROW, 'gap-2 border-t')}>
          <span className={cn(LABEL, 'shrink-0')}>box</span>
          <span className="min-w-0 truncate text-muted-foreground">
            axis {space.box.axis} {space.box.from} to {space.box.until}
          </span>
          <span className="ml-auto shrink-0 tabular-nums text-muted-foreground">
            {space.box.records} in the box
          </span>
        </div>
      )}
    </div>
  );
}

/** What a project declares under `{ "kind": "esg", "at": ... }`: the records the tool showed. */
export interface EsgRecord {
  key: string;
  kind: string;
  address: string;
  origin: string | null;
  creator: string | null;
  written: number;
  record: Record<string, unknown>;
  payload: Record<string, unknown> | null;
  payloadText: string | null;
}

export interface EsgChain {
  volume: string;
  records: EsgRecord[];
}

/**
 * The fields a fact's payload carries, as one line: what the record says, not what it counts.
 *
 * The quantities lead. A payload arrives with its keys in the medium's order, which puts `tco2e` after `basis`, so a
 * line cut to the first few fields dropped the one figure the record exists to state. Numbers are what a record
 * measured, and the text beside them is what names it.
 */
export function payloadLine(record: EsgRecord): string {
  if (!record.payload) return '';
  const scalars = Object.entries(record.payload).filter(([, value]) =>
    ['string', 'number', 'boolean'].includes(typeof value),
  );
  const measured = scalars.filter(([, value]) => typeof value !== 'string');
  const named = scalars.filter(([, value]) => typeof value === 'string');
  return [...measured, ...named].map(([key, value]) => `${key}=${value}`).join(' · ');
}

/** The addresses an intent reads and the one it concluded, which is the link the record itself carries. */
function intentLine(record: EsgRecord): string {
  const held = record.record as {
    from_facts?: string[];
    status?: Record<string, { to_fact?: string }>;
  };
  const from = (held.from_facts ?? []).map((at) => at.slice(0, 12)).join(', ');
  const concluded = held.status?.Concluded?.to_fact;
  return concluded ? `${from} → ${concluded.slice(0, 12)}` : from;
}

/** What a row says: the requirement for a hint, the payload for a fact, the link for an intent. */
function lineOf(record: EsgRecord): string {
  if (record.kind === 'hint') return String((record.record as { content?: string }).content ?? '');
  if (record.kind === 'intent') return intentLine(record);
  return payloadLine(record) || record.address;
}

/** The tool printed nanoseconds; a reader reads a date. */
function when(nanos: number): string {
  if (!nanos) return '';
  return new Date(nanos / 1_000_000).toISOString().slice(0, 19).replace('T', ' ');
}

function ChainMark() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round">
      <path d="M4 7h6v4H4zM14 13h6v4h-6z" />
      <path d="M10 9h2a2 2 0 0 1 2 2v4" />
    </svg>
  );
}

function EsgView({ project }: Board) {
  const chain = (project as { esg?: EsgChain } | null)?.esg;
  if (!chain) {
    return (
      <div className={cn(ROW, 'text-muted-foreground')}>this project declares no records</div>
    );
  }

  // The medium writes each record a time, so sorting on it is the order the chain happened. The listing arrives by key,
  // which is the storage layer's order and not the audit's.
  const records = [...chain.records].sort((left, right) => left.written - right.written);

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className={cn(ROW, 'gap-2 border-b')}>
        <span className={cn(LABEL, 'shrink-0')}>Evidence</span>
        <span className="min-w-0 flex-1 truncate font-mono text-muted-foreground" title={chain.volume}>
          {chain.volume}
        </span>
        <span className="shrink-0 tabular-nums text-muted-foreground">
          {chain.records.length} records
        </span>
      </div>

      <div data-region="esg" className="min-h-0 flex-1 overflow-y-auto">
        {records.map((record) => (
          <div key={record.key} className={cn(ROW, 'gap-2')}>
            <span className={cn(LABEL, 'w-14 shrink-0')}>{record.kind}</span>
            <span
              className="min-w-0 flex-1 truncate font-mono"
              title={`${lineOf(record)}\n${record.key}`}
            >
              {lineOf(record)}
            </span>
            <span className="max-w-40 shrink-0 truncate text-muted-foreground" title={record.creator ?? ''}>
              {record.creator}
            </span>
            <span className="shrink-0 tabular-nums text-muted-foreground" title={`${record.written} ns`}>
              {when(record.written)}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** The reader for the kind, which is what a project's `keyspace` resource arrives through. */
registerResource('keyspace', (at) => at as KeySpace);

/** The records the volume holds, drawn as the tool showed them. */
registerResource('esg', (at) => at as EsgChain);

/** The pack itself. A module registers itself where it is imported, the way the frame imports its own views. */
registerPack({
  id: 'volume',
  readings: [
    {
      key: 'keyspace',
      path: '/keyspace',
      View: KeySpaceView,
      button: { order: 11, title: 'keys', Icon: KeyMark },
    },
    {
      key: 'esg',
      path: '/esg',
      View: EsgView,
      button: { order: 10, title: 'records', Icon: ChainMark },
    },
  ],
});
