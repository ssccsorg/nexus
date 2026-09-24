# 209: the walk reads a page, and the order belongs to the reader

## Context

The record walk (#203, #204) made `for_each_fact` hold one record rather than
every record it matched. What still grew with the volume was the key list the
channel handed over. The walk read `io.list("facts/")` whole, merged the
session's unflushed writes into it, sorted it, and deduped it. Holding a key
costs about 165 bytes on riscv32, so a 64 KiB part capped a walk near 380
records, which is the record maps' wall a little further out. ktema measured
that wall from its side in its
`docs/devlogs/2026-09-14-what-an-mcu-class-format-would-cost.md`.

The list was not held for its own sake. It was held to sort it, and it was
sorted because the walk promised to visit in the identifier order a state read
reports.

## The decision

The order goes, and the walk reads the channel's keys a page at a time.

A streaming enumeration cannot sort what it does not hold, so the list does not
leave unless the promise does. `FileIo::list_page` (ssccsorg/chton#20, merged as
chton#21) is the surface this needs: at most `max` keys and the token that
continues the enumeration, with no order promised across the pages or within
one. Its default hands the list over whole, so a channel that cannot resume
keeps the shape it had, and a caller that needs a bounded page asks for a
channel that overrides it.

`for_each_fact` and `for_each_fact_record` are now one private `walk`, with and
without the content. It reads `WALK_PAGE = 16` keys from the channel, visits
them, and lets the page go before asking for the next. The `Walk` struct and its
`record_at` go with the sorted merge they existed for.

Measured on ktema's device tier, over volumes the device did not write, one run
each:

| records | peak before | peak after |
|---|---|---|
| 8 | 3312 B | 1392 B |
| 32 | 12144 B | 2144 B |
| 96 | 39792 B | 2160 B |

Three times the volume, from 32 to 96 records, moved the peak by 27648 bytes
before and 16 bytes after.

## Why the reader owns the order

The store is spatiotemporal. What orders a record for a reader is an axis the
record carries, most often its time, and an axis can be read off the record
without knowing anything about the volume. A sequence over the medium's keys is
a different thing: it is information about the whole key space, and a page of
memory cannot produce it. To hand it over, a channel would have to hold the keys
(which is the record map by another name) or re-read the medium per candidate
(which is quadratic in the entries). Keeping the promise meant buying back an
index the engine exists to avoid.

So the walk yields the channel's order and promises none. A reader that needs an
order states the one it needs: it derives the order from the record's own axes,
or it sorts what it keeps, which is bounded by what that reader keeps rather
than by the volume.

This is also the honest statement of what a medium can answer. A FAT32
directory enumerates in entry order and an append log in write order. Neither
can hand over an order the other holds, and neither should be asked to.

## What changes, and who it reached

The order was already inconsistent between the layers. ktema's
`Engine::fact_ids` documented its result as "in the order the medium reports
them" over a walk that promised the identifier order a state read reports, so
one of the two had to move, and it moved in the walk.

A state read is untouched. `read_state` and the field filters report the
identifier order they always did, and they get it by sorting the record map
(`sorted_matches`) rather than from the walk. A walk and a state read can
therefore differ in sequence, and a consumer that compares the two compares
sets.

The in-tree consumers of the walk, checked at the call sites in ktema and this
repository. No application in this repository consumes the walk: every one of
them reaches the store through the state read or the field filters, which are
untouched, so the walk's in-tree consumers here are its tests. The production
consumers live in ktema:

- ktema's device run folds the identifiers into the run digest and sorts them
  first, because a digest that answers whether two runs wrote the same volume
  cannot depend on which of them enumerated it. An order-independent fold would
  remove even that sort.
- rem's intent tool takes the smallest identifier, and now sorts to settle one.
- ktema's C surface reports the walk through `ktema_each_fact`, whose header and
  crate doc say the facts arrive "in order". That is now stronger than the
  behavior, and the wording is a follow-up for ktema. This note is where the
  reason is recorded.

No production consumer of the walk requires a global order. The one place that
required the identifier order was the comparison against a state read, and it is
a test. External consumers cannot be checked from this tree, so the statement is
about this one.

## What is not this change

The paged walk does not turn the medium's own order into a promise. ktema's
`docs/devlogs/2026-09-14-the-log-order-is-the-time-axis.md` wants the log's
order surfaced, so that a time window becomes a contiguous byte range rather
than a scan of the volume. For a log channel the channel's order is that order,
so this change points the same way, and it stops short of the two things that
devlog asks for: a contract that promises a channel's order where the channel
has one, and a positional read that can seek to the start of a band. Those are
future work, and this note is where their relationship to this change is
recorded.

## Tests

`nex/fih/tests/walk.rs`:

- `a_walk_visits_in_the_order_a_state_read_reports` becomes
  `a_walk_and_a_state_read_reach_the_same_records`, which compares the two as
  sets.
- `a_walk_reads_the_keys_a_page_at_a_time_in_the_channel_s_order` drives the
  walk through a `PagedIo` double that serves a prefix a page at a time in
  reverse write order and hands over nothing from `list`, so a walk that read
  the list rather than the pages would visit no record. It asserts the order the
  walk visited in and that the bound it asked for was the page rather than the
  volume.
- `a_framing_walk_visits_the_same_records_without_reading_their_content` still
  holds: the paging default reads the same keys.

## Depends on

ssccsorg/chton#21 (`list_page` in `FileIo`, merged). The method takes a cursor
rather than a visitor, so one signature and one override serve both an
`IoFuture` that must be `Send` and one that must not. `Durable` forwards it, so
a wrapped channel keeps the paging rather than falling back to the default.

Closes #209.
