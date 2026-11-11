// The records as the network their links make: a requirement bounds work, sources are evidence for it, and it
// concludes a result.
//
// The shape is the chain's own and not the screen's. A fact, an intent and a hint are three kinds of record, and the
// links between them are fields the records carry: an intent names the facts it read and the fact it concluded, and a
// result names the requirement it answers. So every edge below comes from a record, and a record that states no link
// draws none.
//
// The words are the chain's, not the format's. The frame marks a record's kind; this reading names the place a record
// takes in a chain, which `roleOf` reads from the same links the edges are drawn from. Two readings of one volume
// therefore agree about what each record is, because they ask the same question of the same fields.
//
// A record with no link on it is drawn and dimmed rather than left out. A quarter's inventory holds readings no
// calculation read, and an audit's first finding is often that one: leaving them out would draw a plant that looks fully
// accounted for.

import { useEffect, useRef } from 'react';
import { KindTag, registerPack, useKindWords, type Board } from '@ssccsorg/skene-frame';
import { LABEL, ROW, cn } from '@ssccsorg/skene-frame/ui';
import { concludedOf, roleOf, type EsgChain, type EsgRecord } from './pack';
import { editedOf, pick, useBranch, usePicked, type Branch } from './workbench';

/** The place a record takes in the chain, which is the column it stands in. */
export type ChainRole = 'requirement' | 'measurement' | 'transformation' | 'result';

/**
 * How one record reaches another, in the words the model uses.
 *
 * `evidence` is a fact an intent read, `conclusion` is the fact an intent answered, and `bounds` is the requirement a
 * conclusion states it answers. The three are the directions the chain runs in, and none of them is a guess: a link
 * exists only where a record states one.
 */
export type ChainLinkRole = 'evidence' | 'conclusion' | 'bounds';

export interface ChainNode {
  /** The record's address, which is what the other records name it by. */
  id: string;
  role: ChainRole;
  /** What the record is, read from its own fields. */
  title: string;
  /** What it holds, one fact per line, in the order the record writes them. */
  detail: string[];
  /** The whole of what the record says, for a reader who asks the node. */
  says: string;
  /** Who wrote it, which is the unit a press on the node picks. */
  creator: string;
  /** Whether any record names it or it names one, which is what makes it part of the chain. */
  linked: boolean;
  /** Whether the branch is taken at this record. */
  edited: boolean;
  /** Whether this record is downstream of the branch, so what it says rests on the pilot's declaration. */
  derived: boolean;
}

export interface ChainLink {
  id: string;
  from: string;
  to: string;
  role: ChainLinkRole;
}

export interface CausalChain {
  nodes: ChainNode[];
  links: ChainLink[];
  /** How many measured records no calculation read, which is stated rather than drawn as a gap. */
  idle: number;
}

/** A field name as a reader's words, which is all the payload's spelling needs. */
function words(name: string): string {
  return name.replaceAll('_', ' ');
}

/** The code a requirement is cited by: `ESRS E1-6` out of the sentence that discloses it. */
export function codeOf(content: string): string {
  const at = content.indexOf(':');
  return (at === -1 ? content : content.slice(0, at)).trim();
}

/**
 * A node's title, read from the record.
 *
 * A measured fact is what reported it, a result is the quantity it states, a transformation is the work it declares,
 * and a requirement is the code it is cited by. Each is a field the record holds, which is what keeps the drawing from
 * naming a record the volume does not.
 */
function titleOf(record: EsgRecord, role: ChainRole): string {
  const payload = record.payload ?? {};
  if (record.kind === 'hint') return codeOf(String((record.record as { content?: string }).content ?? ''));
  if (record.kind === 'intent') return record.description ?? 'a transformation';
  if (role === 'result') {
    // A scope is the quantity's own name; a figure that states none is named for what it is a rate of.
    if (typeof payload.scope === 'number') return `Scope ${payload.scope}`;
    const leading = Object.keys(payload).find(
      (key) => ['string', 'number', 'boolean'].includes(typeof payload[key]),
    );
    return leading ? words(leading) : 'a result';
  }
  return record.creator ?? 'an observation';
}

/** What a record holds, one fact per line: the figures for a fact, the sources for a transformation. */
function detailOf(record: EsgRecord, role: ChainRole): string[] {
  const payload = record.payload ?? {};
  if (record.kind === 'hint') return record.creator ? [record.creator] : [];
  if (record.kind === 'intent') {
    const sources = ((record.record as { from_facts?: string[] }).from_facts ?? []).length;
    return [`${sources} ${sources === 1 ? 'source' : 'sources'}`];
  }
  if (role === 'measurement') {
    const metric = typeof payload.metric === 'string' ? words(payload.metric) : null;
    const figure = [payload.value, payload.unit].filter((each) => each !== undefined).join(' ');
    return [metric, figure || null].filter((line): line is string => Boolean(line));
  }
  // A result states its figures in its own order, so the first two are the ones its author led with.
  return Object.entries(payload)
    .filter(([, value]) => ['string', 'number', 'boolean'].includes(typeof value))
    .slice(0, 2)
    .map(([key, value]) => `${words(key)} ${value}`);
}

/**
 * The network the records make.
 *
 * Every edge is a field: `from_facts` is evidence, `Concluded.to_fact` is the conclusion, and the `requirement` a
 * conclusion states is the requirement that bounded the work which reached it. A requirement no conclusion cites is a
 * node with no edge, which is a fact about the volume rather than a gap in the drawing.
 *
 * A branch is read here rather than downstream, because an edited requirement changes the code a conclusion is matched
 * by: edit `ESRS E1-6` into another code and the edges that cited it stop being drawn, which is the honest answer to a
 * constraint the evidence no longer names.
 */
export function chainOf(records: EsgRecord[], branch: Branch = { at: null, edit: null }): CausalChain {
  const spent = records.map((record) => editedOf(record, branch));
  const concluded = new Set(
    spent.map((record) => concludedOf(record)).filter((at): at is string => Boolean(at)),
  );

  // A requirement is cited by its code, which is what a conclusion names and what a hint is titled with.
  const requirement = new Map(
    spent
      .filter((record) => record.kind === 'hint')
      .map((record) => [
        codeOf(String((record.record as { content?: string }).content ?? '')),
        record.address,
      ]),
  );

  const links: ChainLink[] = [];
  const link = (from: string, to: string, role: ChainLinkRole) => {
    links.push({ id: `${role}-${from}-${to}`, from, to, role });
  };

  const concludedBy = new Map<string, string>();
  for (const record of spent) {
    if (record.kind !== 'intent') continue;
    for (const source of (record.record as { from_facts?: string[] }).from_facts ?? []) {
      link(source, record.address, 'evidence');
    }
    const result = concludedOf(record);
    if (!result) continue;
    link(record.address, result, 'conclusion');
    concludedBy.set(result, record.address);
    const named = spent.find((each) => each.address === result)?.payload?.requirement;
    if (typeof named !== 'string') continue;
    const bounds = requirement.get(named);
    if (bounds) link(bounds, record.address, 'bounds');
  }

  const touched = new Set(links.flatMap((each) => [each.from, each.to]));
  // What the branch reaches: the result an edited transformation concluded. A requirement's text is not a change to the
  // work that cited it, so nothing downstream of a requirement is marked.
  const derived = new Set<string>();
  if (branch.at) {
    for (const each of links) {
      if (each.from === branch.at && each.role === 'conclusion') derived.add(each.to);
    }
  }

  const nodes: ChainNode[] = spent.map((record) => {
    const role = roleOf(record, concluded) as ChainRole;
    return {
      id: record.address,
      role,
      title: titleOf(record, role),
      detail: detailOf(record, role),
      says:
        record.description ??
        record.payloadText ??
        String((record.record as { content?: string }).content ?? ''),
      creator: record.creator ?? '',
      // A requirement stands on its own: it is a bound, and a volume holding an unanswered bound is the finding.
      linked: touched.has(record.address) || record.kind === 'hint',
      edited: branch.at === record.address,
      derived: derived.has(record.address),
    };
  });

  return {
    nodes,
    links,
    idle: nodes.filter((node) => !node.linked).length,
  };
}

/** The order the columns stand in, left to right, which is the order the chain runs. */
const COLUMNS: ChainRole[] = ['requirement', 'measurement', 'transformation', 'result'];

/** What a column is headed with, which names the place rather than the record. */
const HEADING: Record<ChainRole, string> = {
  requirement: 'requirements',
  measurement: 'observed',
  transformation: 'transformations',
  result: 'results',
};

const NODE = { w: 168, h: 82 } as const;
const GAP = { x: 44, y: 12 } as const;
const PAD = 18;
const HEAD = 22;

interface Placed {
  node: ChainNode;
  x: number;
  y: number;
}

/** The nodes in columns, and the size of the whole picture. A node on the chain stands above one that is not. */
function placed(nodes: ChainNode[]): { at: Placed[]; w: number; h: number } {
  const at: Placed[] = [];
  let tallest = 0;
  COLUMNS.forEach((role, column) => {
    const held = nodes
      .filter((node) => node.role === role)
      .sort((left, right) => Number(right.linked) - Number(left.linked));
    tallest = Math.max(tallest, held.length * NODE.h + Math.max(0, held.length - 1) * GAP.y);
    held.forEach((node, row) => {
      at.push({
        node,
        x: PAD + column * (NODE.w + GAP.x),
        y: PAD + HEAD + row * (NODE.h + GAP.y),
      });
    });
  });
  return {
    at,
    w: PAD * 2 + COLUMNS.length * NODE.w + (COLUMNS.length - 1) * GAP.x,
    h: tallest + PAD * 2 + HEAD,
  };
}

/** The ink a role is drawn in. The four are peers, so no role is drawn larger than another. */
const INK: Record<ChainRole, string> = {
  requirement: 'var(--hint)',
  measurement: 'var(--fact)',
  transformation: 'var(--intent)',
  result: 'var(--fact)',
};

/** A line cut to what its slot holds, so a long declaration does not run over its neighbour. */
function clip(text: string, width: number): string {
  if (text.length <= width) return text;
  const cut = text.lastIndexOf(' ', width);
  return `${text.slice(0, cut > width / 2 ? cut : width - 1)}…`;
}

/** A title over two lines, which is what a declared transformation needs to be read at all. */
function wrap(text: string, width: number): string[] {
  if (text.length <= width) return [text];
  const cut = text.lastIndexOf(' ', width);
  const at = cut > width / 2 ? cut : width;
  return [text.slice(0, at), clip(text.slice(at).trimStart(), width)];
}

function ChainView({ project, pickedId, onPick }: Board) {
  const chain = (project as { esg?: EsgChain } | null)?.esg;
  const branch = useBranch();
  const picked = usePicked();
  const words = useKindWords();
  const focus = useRef<HTMLDivElement | null>(null);

  // A unit picked on the map is a focus rather than a filter: the records it wrote stay where they are in the chain,
  // and the first of them is brought into view, because a filter would take away the edges that say what it feeds.
  const records = chain ? [...chain.records].sort((left, right) => left.written - right.written) : [];
  const model = chainOf(records, branch);
  const { at, w, h } = placed(model.nodes);
  const where = new Map(at.map((spot) => [spot.node.id, spot]));
  const counted = (role: ChainLinkRole) => model.links.filter((each) => each.role === role).length;
  const isFocus = (node: ChainNode) => Boolean(pickedId) && node.creator === pickedId;

  useEffect(() => {
    if (!pickedId) return;
    focus.current?.querySelector('[data-focus="true"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }, [pickedId, model.nodes.length]);

  if (!chain) {
    return <div className={cn(ROW, 'text-muted-foreground')}>this project declares no records</div>;
  }

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <div className={cn(ROW, 'gap-3 border-b')}>
        <span className={cn(LABEL, 'shrink-0')}>Chain</span>
        <span className="min-w-0 flex-1 truncate text-muted-foreground">
          {model.nodes.filter((node) => node.linked).length} on the chain · {counted('evidence')} sources cited ·{' '}
          {counted('conclusion')} conclusions · {counted('bounds')} bounded
          {model.idle ? ` · ${model.idle} on file, read by nothing` : ''}
        </span>
      </div>

      <div data-region="chain" ref={focus} className="min-h-0 flex-1 overflow-auto">
        <svg
          viewBox={`0 0 ${w} ${h}`}
          // The picture is as wide as four columns and as tall as the longest one, so it scales to the panel it is
          // given and keeps a floor at which a node's text is still the size a reader can read.
          className="block h-auto w-full"
          style={{ minWidth: 700 }}
          role="img"
          aria-label="the records and the links they carry"
        >
          {COLUMNS.map((role, column) => (
            <text
              key={role}
              x={PAD + column * (NODE.w + GAP.x)}
              y={PAD + 10}
              className="fill-muted-foreground"
              fontSize={9.5}
              letterSpacing={0.6}
            >
              {HEADING[role].toUpperCase()}
            </text>
          ))}

          {model.links.map((each) => {
            const from = where.get(each.from);
            const to = where.get(each.to);
            if (!from || !to) return null;
            const x1 = from.x + NODE.w;
            const y1 = from.y + NODE.h / 2;
            const x2 = to.x;
            const y2 = to.y + NODE.h / 2;
            const bend = (x2 - x1) / 2;
            return (
              <path
                key={each.id}
                d={`M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`}
                fill="none"
                stroke={each.role === 'bounds' ? 'var(--hint)' : 'var(--fact)'}
                strokeWidth={1.1}
                strokeDasharray={each.role === 'bounds' ? '4 3' : undefined}
                opacity={each.role === 'bounds' ? 0.7 : 0.5}
              />
            );
          })}

          {at.map(({ node, x, y }) => {
            const here = node.id === picked;
            const focusHere = isFocus(node);
            return (
              <g
                key={node.id}
                data-focus={focusHere || here ? 'true' : undefined}
                onClick={() => {
                  pick(node.id);
                  if (node.creator) onPick(node.creator);
                }}
                className="cursor-pointer"
                opacity={node.linked ? 1 : 0.45}
              >
                <title>{node.says}</title>
                <rect
                  x={x}
                  y={y}
                  width={NODE.w}
                  height={NODE.h}
                  rx={5}
                  className="fill-card"
                  stroke={node.edited ? 'var(--claim)' : here || focusHere ? 'var(--foreground)' : 'var(--border)'}
                  strokeWidth={here || focusHere ? 1.4 : 1}
                  strokeDasharray={node.derived && !node.edited ? '3 2' : undefined}
                />
                <rect x={x} y={y} width={2.5} height={NODE.h} rx={1} fill={INK[node.role]} />
                <text x={x + 12} y={y + 15} fontSize={9.5} fill={INK[node.role]} letterSpacing={0.4}>
                  {node.edited ? 'edited' : node.derived ? 'of the branch' : node.role}
                </text>
                {wrap(node.title, 27).map((line, index) => (
                  <text
                    key={line + index}
                    x={x + 12}
                    y={y + 30 + index * 12}
                    fontSize={10.5}
                    fontWeight={500}
                    className="fill-foreground"
                  >
                    {line}
                  </text>
                ))}
                {node.detail.slice(0, 2).map((line, index) => (
                  <text
                    key={line + index}
                    x={x + 12}
                    y={y + NODE.h - 21 + index * 11}
                    fontSize={9.5}
                    className="fill-muted-foreground"
                  >
                    {clip(line, 30)}
                  </text>
                ))}
              </g>
            );
          })}
        </svg>
      </div>

      {/* The key under the picture, where a diagram's key belongs. The letters in the lists are the project's words, and a
          reader handed three letters is owed them; drawing the key from the vocabulary is what keeps a project that
          renames them from having to rename this. */}
      <div className={cn(ROW, 'gap-1.5 border-t text-muted-foreground')}>
        <KindTag kind="fact" />
        {words.fact}
        <KindTag kind="intent" className="ml-2" />
        {words.intent}
        <KindTag kind="hint" className="ml-2" />
        {words.hint}
      </div>
    </div>
  );
}

function ChainMark() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round">
      <circle cx="5" cy="12" r="2.4" />
      <circle cx="19" cy="6" r="2.4" />
      <circle cx="19" cy="18" r="2.4" />
      <path d="M7.4 11.2 16.6 6.8M7.4 12.8l9.2 4.4" />
    </svg>
  );
}

registerPack({
  id: 'volume',
  readings: [
    {
      key: 'chain',
      path: '/chain',
      View: ChainView,
      button: { order: 9, title: 'chain', Icon: ChainMark },
    },
  ],
});
