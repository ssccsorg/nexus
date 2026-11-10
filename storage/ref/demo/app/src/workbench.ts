// What the pilot is working on: the record they picked, and the branch they have taken from one.
//
// Two pieces of state, because a demonstration that only reads has one thing to show and a demonstration that answers
// needs the question. Neither is the volume's: a pick names a record the tool printed, and a branch is an alternative
// declaration about a record, held beside the records rather than written into them. So the volume stays the evidence it
// was, and the branch is what the screen adds to it.
//
// A module and not a store library: two values, two actions, and a subscription. The frame's own state is the frame's
// and is not exported for a deployment to add to, and a second state library in one application would be a second
// convention for the same question.

import { useSyncExternalStore } from 'react';
import type { EsgRecord } from './pack';

/** What a branch changes about the record it is taken at. A field left out is a field the pilot left alone. */
export interface Edit {
  /** A transformation's declared work, for an intent. */
  description?: string;
  /** A requirement's text, for a hint. */
  content?: string;
}

/** The branch: the record it is taken at, and what it says there. */
export interface Branch {
  at: string | null;
  edit: Edit | null;
}

const NONE: Branch = { at: null, edit: null };

let picked: string | null = null;
let branch: Branch = NONE;
const listeners = new Set<() => void>();
let revision = 0;

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

function emit(): void {
  revision += 1;
  for (const listener of listeners) listener();
}

/** Pick a record, which is what a press in the evidence list or on a node of the chain does. */
export function pick(address: string | null): void {
  if (picked === address) return;
  picked = address;
  emit();
}

/** Take a branch at a record, replacing any earlier one: one branch is one alternative to compare. */
export function takeBranch(at: string, edit: Edit): void {
  branch = { at, edit };
  emit();
}

/** Give the volume back. */
export function clearBranch(): void {
  if (!branch.at) return;
  branch = NONE;
  emit();
}

export function usePicked(): string | null {
  return useSyncExternalStore(
    subscribe,
    () => picked,
    () => picked,
  );
}

export function useBranch(): Branch {
  return useSyncExternalStore(
    subscribe,
    () => branch,
    () => branch,
  );
}

/**
 * What a record says once the branch is taken.
 *
 * A record the branch does not name is the record. One it names says what the pilot wrote, and everything downstream of
 * it is read as the branch's: the result an edited transformation concluded, and the requirement an edited conclusion
 * now cites. Nothing is recomputed, because the arithmetic is the reporting controller's and the volume holds its
 * answer: what a branch changes is the declaration, and the screen says which records rest on it.
 */
export function editedOf(record: EsgRecord, branch: Branch): EsgRecord {
  if (!branch.at || branch.at !== record.address || !branch.edit) return record;
  if (record.kind === 'intent' && branch.edit.description !== undefined) {
    return { ...record, description: branch.edit.description };
  }
  if (record.kind === 'hint' && branch.edit.content !== undefined) {
    return { ...record, record: { ...record.record, content: branch.edit.content } };
  }
  return record;
}
