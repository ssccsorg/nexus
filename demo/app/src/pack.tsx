// The records a volume holds, drawn as the storage layer's own picture.
//
// A record list is where a volume is read, and the frame's own readings are named for what a record is. So this arrives as
// a pack: one resource kind and one reading, declared by the project that wants it, and the frame learns neither. The
// numbers on the screen are the ones the reference tool printed, carried in the resource by the importer rather than
// counted here.

import { KindTag, registerPack, registerResource, useKindWords, type Board } from '@ssccsorg/skene-frame';
import { LABEL, ROW, cn } from '@ssccsorg/skene-frame/ui';
import { useEffect, useRef } from 'react';
import { editedOf, pick, useBranch, usePicked } from './workbench';

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
  /** The transformation an intent declares, which the record names by hash and the tool resolves. */
  description: string | null;
  /** What the tool's `verify` answered for this record's address, and null where it has no answer to give. */
  addressState: string | null;
}

export interface EsgChain {
  volume: string;
  records: EsgRecord[];
}

/**
 * The fields a fact's payload carries, as one line: what the record says, not what it counts.
 *
 * The order is the record's own. A payload arrives with the fields in the order its author wrote them, so the first one is
 * the one the author put first, and a screen that re-sorted them would decide for the record what it leads with. Every
 * scalar field is on the line and the cell clips it; the tooltip carries the whole of it.
 */
export function payloadLine(record: EsgRecord): string {
  if (!record.payload) return '';
  return Object.entries(record.payload)
    .filter(([, value]) => ['string', 'number', 'boolean'].includes(typeof value))
    .map(([key, value]) => `${key}=${value}`)
    .join(' · ');
}

/** The fact an intent concluded, which is the link the record itself carries. */
export function concludedOf(record: EsgRecord): string | undefined {
  const held = record.record as { status?: Record<string, { to_fact?: string }> };
  return held.status?.Concluded?.to_fact;
}

/** The addresses an intent reads and the one it concluded, which is the link the record itself carries. */
function intentLine(record: EsgRecord): string {
  const held = record.record as { from_facts?: string[] };
  const from = (held.from_facts ?? []).map((at) => at.slice(0, 12)).join(', ');
  const concluded = concludedOf(record);
  return concluded ? `${from} → ${concluded.slice(0, 12)}` : from;
}

/**
 * What a record does in the chain, in a reader's words.
 *
 * The format's kinds are the record's, and they are marked by the frame's own tag. What differs here is the place a
 * record takes in the chain, which the records state themselves: an intent concluded a fact, and that fact is a result
 * rather than an observation. It is read from the links, so the words follow the data rather than a list kept beside it.
 */
export function roleOf(record: EsgRecord, concluded: Set<string>): string {
  if (record.kind === 'hint') return 'requirement';
  if (record.kind === 'intent') return 'transformation';
  return concluded.has(record.address) ? 'result' : 'measurement';
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

function EsgView({ project, pickedId, onPick }: Board) {
  const chain = (project as { esg?: EsgChain } | null)?.esg;
  const branch = useBranch();
  const picked = usePicked();
  const words = useKindWords();
  const focus = useRef<HTMLDivElement | null>(null);

  // The record's own time is the order the chain happened. The listing arrives by key, which is the storage layer's
  // order and not the audit's. What a branch changes is what a record says, so it is read here rather than kept in a
  // second copy of the volume.
  const records = chain
    ? [...chain.records]
        .sort((left, right) => left.written - right.written)
        .map((record) => editedOf(record, branch))
    : [];
  const concluded = new Set(
    records.map((record) => concludedOf(record)).filter((at): at is string => Boolean(at)),
  );
  const isFocus = (record: EsgRecord) => Boolean(pickedId) && record.creator === pickedId;

  // A unit picked on the map brings its first record into view, which is the half of the link a press on a pin means.
  useEffect(() => {
    if (!pickedId) return;
    focus.current?.querySelector('[data-focus="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [pickedId, records.length]);

  if (!chain) {
    return (
      <div className={cn(ROW, 'text-muted-foreground')}>this project declares no records</div>
    );
  }

  // The tool recomputed an address for the facts it can, which are the ones whose address is their own content.
  const measured = records.filter((record) => record.addressState !== null);
  const verified = measured.filter((record) => record.addressState === 'valid').length;

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className={cn(ROW, 'gap-2 border-b')}>
        <span className={cn(LABEL, 'shrink-0')}>Evidence</span>
        <span
          className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground"
          title={chain.volume}
        >
          {chain.volume}
        </span>
        <span className="shrink-0 tabular-nums text-muted-foreground">
          {records.length} records
        </span>
        <span
          className="shrink-0 tabular-nums text-muted-foreground"
          title={`the tool recomputed each address from the origin, the creator, and the payload, for the facts whose
address is their own; a result is filed at its intent's address, and an intent and a hint are named`}
        >
          {verified} of {measured.length} addresses recomputed
        </span>
      </div>

      <div data-region="esg" ref={focus} className="min-h-0 flex-1 overflow-y-auto">
        {records.map((record) => {
          // A press is the pick, and the pick is the pair: the record this row stands for and the unit that wrote it.
          // The same press on a pin on the map is the other half, so the two readings cannot point at two things.
          const here = record.address === picked;
          const focusHere = isFocus(record);
          return (
            <div
              key={record.key}
              data-focus={focusHere || here ? 'true' : undefined}
              onClick={() => {
                pick(record.address);
                if (record.creator) onPick(record.creator);
              }}
              className={cn(
                ROW,
                'cursor-pointer gap-2 hover:bg-muted/40',
                (here || focusHere) && 'bg-muted/60',
                record.addressState === 'invalid' && 'text-destructive',
              )}
            >
              <KindTag kind={record.kind as 'fact' | 'intent' | 'hint' | 'claim'} className="shrink-0" />
              <span className={cn(LABEL, 'w-24 shrink-0 truncate')}>
                {/* The place in the chain, unless the kind's own word already says it: a requirement is a requirement. */}
                {roleOf(record, concluded) === words[record.kind as 'fact' | 'intent' | 'hint']
                  ? ''
                  : roleOf(record, concluded)}
              </span>
              <span
                className="min-w-0 flex-1 truncate font-mono"
                title={[lineOf(record), record.key, record.addressState ? `address ${record.addressState}` : null]
                  .filter(Boolean)
                  .join('\n')}
              >
                {lineOf(record)}
              </span>
              <span className="max-w-36 shrink-0 truncate text-muted-foreground" title={record.creator ?? ''}>
                {record.creator}
              </span>
              <span className="shrink-0 tabular-nums text-muted-foreground" title={`${record.written} ns`}>
                {when(record.written)}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** The records the volume holds, drawn as the tool showed them. */
registerResource('esg', (at) => at as EsgChain);

/** The pack itself. A module registers itself where it is imported, the way the frame imports its own views. */
registerPack({
  id: 'volume',
  readings: [
    {
      key: 'esg',
      path: '/esg',
      View: EsgView,
      button: { order: 10, title: 'records', Icon: ChainMark },
    },
  ],
});
