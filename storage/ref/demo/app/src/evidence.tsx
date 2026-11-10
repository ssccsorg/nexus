// The picked record, opened in the frame's own column.
//
// A press in the evidence list or on a node of the chain is the pick, and this is where the pick is read: the record's
// fields, the links it carries, and the one field a reader may change. It stands as a panel of the frame rather than as
// a card over the space, because the frame already has a place for a column and a card would be a second place to look
// for the same record.
//
// The form is the demonstration's own half of the record model, and it is deliberately small. What a branch edits is a
// declaration: what a transformation says it did, or what a requirement says. The figures stay as the controller filed
// them, because the arithmetic is the reporter's and the volume holds its answer. What the branch changes is what the
// chain is read against, so the screen says which records now rest on the pilot's declaration.

import { useEffect, useState } from 'react';
import { GitBranch, RotateCcw, ScrollText } from 'lucide-react';
import { KindTag, registerPanel, useScenario } from '@ssccsorg/skene-frame';
import { LABEL, ROW, TITLE, Button, cn } from '@ssccsorg/skene-frame/ui';
import { chainOf, codeOf } from './chain';
import { concludedOf, roleOf, type EsgChain, type EsgRecord } from './pack';
import { clearBranch, editedOf, takeBranch, useBranch, usePicked } from './workbench';

/** The width the panel stands at, which is its own because the frame hands a panel a place and not a measure. */
const WIDTH = 340;

/** What a record declares, in the field a branch can change. Empty for a fact, which declares nothing. */
function declaredOf(record: EsgRecord): string {
  if (record.kind === 'intent') return record.description ?? '';
  if (record.kind === 'hint') return String((record.record as { content?: string }).content ?? '');
  return '';
}

/** The field a declaration is edited in, named once so the form and the branch agree about which one it is. */
function fieldOf(record: EsgRecord): 'description' | 'content' | null {
  if (record.kind === 'intent') return 'description';
  if (record.kind === 'hint') return 'content';
  return null;
}

/** A value as a row reads it: a number stays a number, a list says how long it is. */
function shown(value: unknown): string {
  if (Array.isArray(value)) return `${value.length} entries`;
  if (value && typeof value === 'object') return 'a record';
  return String(value);
}

/** One line of the panel's own table. */
function Field({ name, value }: { name: string; value: string }) {
  return (
    <div className="flex flex-col gap-0.5 border-b border-border/50 px-3 py-1.5">
      <span className={LABEL}>{name}</span>
      <span className="text-[11.5px] break-words">{value}</span>
    </div>
  );
}

function EvidencePanel() {
  const model = useScenario((state) => state.model);
  const picked = usePicked();
  const branch = useBranch();
  const chain = (model?.project as { esg?: EsgChain } | undefined)?.esg;
  const records = chain?.records ?? [];
  const ordered = [...records].sort((left, right) => left.written - right.written);
  const graph = chainOf(ordered, branch);
  const concluded = new Set(
    records.map((record) => concludedOf(record)).filter((at): at is string => Boolean(at)),
  );

  const record = records.find((each) => each.address === picked) ?? null;
  const node = graph.nodes.find((each) => each.id === picked) ?? null;
  const field = record ? fieldOf(record) : null;
  const [draft, setDraft] = useState('');

  // The draft follows the pick, and a branch applied to this record puts the branch's text in the field. A record's
  // own object is the volume's and does not change under a keystroke, so typing is never overwritten.
  useEffect(() => {
    setDraft(record ? declaredOf(editedOf(record, branch)) : '');
  }, [picked, branch.at, branch.edit, record]);

  if (!record || !node) {
    return (
      <div
        data-region="evidence"
        style={{ width: WIDTH }}
        className="relative flex min-h-0 shrink-0 flex-col border-l bg-card/40"
      >
        <div className={cn(ROW, 'gap-2 border-b')}>
          <span className={LABEL}>Evidence</span>
          <span className="min-w-0 flex-1 truncate text-muted-foreground">nothing is picked</span>
        </div>
        <p className="px-3 py-2 text-[11.5px] text-muted-foreground">
          Press a record in the chain or in the list. A press on a machine on the map is the same pick read the other
          way: it brings that machine&apos;s records into view.
        </p>
      </div>
    );
  }

  const sources = graph.links.filter((each) => each.to === record.address && each.role === 'evidence');
  const concludedTo = graph.links.filter(
    (each) => each.from === record.address && each.role === 'conclusion',
  );
  const boundedBy = graph.links.filter((each) => each.to === record.address && each.role === 'bounds');
  const bounds = graph.links.filter((each) => each.from === record.address && each.role === 'bounds');
  const named = concludedTo
    .map((each) => records.find((held) => held.address === each.to)?.payload?.requirement)
    .find((value): value is string => typeof value === 'string');
  const titleOf = (id: string): string => graph.nodes.find((each) => each.id === id)?.title ?? id;
  const taken = branch.at === record.address;
  const unchanged = draft === declaredOf(editedOf(record, branch));

  return (
    <div
      data-region="evidence"
      style={{ width: WIDTH }}
      className="relative flex min-h-0 shrink-0 flex-col border-l bg-card/40"
    >
      <div className={cn(ROW, 'gap-2 border-b')}>
        <KindTag kind={record.kind as 'fact' | 'intent' | 'hint' | 'claim'} />
        <span className="min-w-0 flex-1 truncate text-muted-foreground">{roleOf(record, concluded)}</span>
        {taken ? (
          <Button size="xs" variant="ghost" title="give the volume back" onClick={clearBranch}>
            <RotateCcw />
            revert
          </Button>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="flex flex-col gap-0.5 border-b px-3 py-2">
          <span className={cn(TITLE, 'break-words')}>{node.title}</span>
          <span className="font-mono text-[10px] break-all text-muted-foreground">{record.address}</span>
        </div>

        <Field name="kind" value={record.kind} />
        <Field name="creator" value={record.creator ?? 'none'} />
        {record.origin ? <Field name="origin" value={record.origin} /> : null}
        <Field name="written" value={new Date(record.written / 1_000_000).toISOString()} />
        <Field
          name="address"
          value={
            record.addressState
              ? `recomputed from the origin, the creator, and the payload: ${record.addressState}`
              : 'filed at the address of the conclusion that produced it'
          }
        />

        {record.payload
          ? Object.entries(record.payload).map(([name, value]) => (
              <Field key={name} name={name} value={shown(value)} />
            ))
          : null}

        {sources.length ? (
          <Field name="read from" value={sources.map((each) => titleOf(each.from)).join(' · ')} />
        ) : null}
        {concludedTo.length ? (
          <Field name="concluded" value={concludedTo.map((each) => titleOf(each.to)).join(' · ')} />
        ) : null}
        {named ? <Field name="requirement stated" value={named} /> : null}
        {boundedBy.length ? (
          <Field name="bounded by" value={boundedBy.map((each) => titleOf(each.from)).join(' · ')} />
        ) : null}
        {bounds.length ? (
          <Field name="bounds" value={bounds.map((each) => titleOf(each.to)).join(' · ')} />
        ) : null}
        {record.kind === 'hint' && !bounds.length ? (
          <Field name="bounds" value="nothing in this volume cites this requirement" />
        ) : null}

        {field ? (
          <div className="flex flex-col gap-1.5 px-3 py-2">
            <span className={LABEL}>{field === 'content' ? 'the requirement' : 'the declared work'}</span>
            <textarea
              data-region="declared"
              value={draft}
              rows={5}
              spellCheck={false}
              onChange={(event) => setDraft(event.target.value)}
              className="w-full resize-y rounded-sm border bg-transparent px-1.5 py-1 font-mono text-[11px] leading-4 outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
            />
            <div className="flex items-center gap-1.5">
              <Button
                size="xs"
                variant="outline"
                disabled={!draft.trim() || unchanged}
                title="take a branch at this record"
                onClick={() =>
                  takeBranch(record.address, field === 'content' ? { content: draft } : { description: draft })
                }
              >
                <GitBranch />
                take the branch
              </Button>
              <span className="min-w-0 flex-1 truncate text-[10.5px] text-muted-foreground">
                {taken ? 'taken at this record' : "the figures stay the controller's"}
              </span>
            </div>
            {taken ? (
              <span className="text-[10.5px] leading-4 text-muted-foreground">
                {record.kind === 'hint'
                  ? bounds.length
                    ? `the chain is read against ${codeOf(draft)}, so the conclusions that cite the old code no longer reach it`
                    : 'no conclusion cites this code, so no requirement reaches a transformation'
                  : 'everything this record concluded is read as the branch\'s, and the chain says so'}
              </span>
            ) : null}
          </div>
        ) : (
          <p className="px-3 py-2 text-[11.5px] text-muted-foreground">
            a measured record: the bytes are the reading and there is nothing here for a branch to change
          </p>
        )}
      </div>
    </div>
  );
}

registerPanel({
  key: 'evidence',
  at: 'right',
  order: 1,
  side: 'right',
  label: 'evidence',
  title: 'the picked record',
  starts: true,
  Icon: ScrollText,
  View: EvidencePanel,
});
