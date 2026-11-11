// The notice a window too narrow for the board reads.
//
// The board is three columns, a split, and a map, and every one of them is sized for a window. A phone draws the same
// markup as slivers, so the page says what it is rather than letting a reader take a cramped screen for a broken one.

import { useState } from 'react';

function Warning({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className}
    >
      <path d="M12 4 2.8 20h18.4Z" />
      <path d="M12 10v4M12 17.2v.1" />
    </svg>
  );
}

export function Narrow() {
  const [read, setRead] = useState(false);
  if (read) return null;
  return (
    <div
      data-region="narrow"
      className="fixed inset-x-0 bottom-0 z-[300] flex items-start gap-2 border-t border-border bg-sidebar px-3 py-2 text-[11.5px] leading-snug text-muted-foreground md:hidden"
    >
      <Warning className="mt-px size-3.5 shrink-0 text-idle" />
      <span className="min-w-0 flex-1">
        <span className="text-foreground">A window is required.</span> The columns, the split, and the map are sized for
        one; on a phone this board is readable and not operable.
      </span>
      <button
        type="button"
        onClick={() => setRead(true)}
        className="shrink-0 rounded-sm border border-border px-1.5 py-px text-foreground"
      >
        dismiss
      </button>
    </div>
  );
}
