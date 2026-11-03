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

/** The reader for the kind, which is what a project's `keyspace` resource arrives through. */
registerResource('keyspace', (at) => at as KeySpace);

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
  ],
});
