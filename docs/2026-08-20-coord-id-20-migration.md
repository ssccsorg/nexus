# CoordId<20> migration (breaking)

## What changed

Issue #176 switched the record id scheme to a full-injective encoding:

- `COORD_ID_DEPTH` went from 6 to 20. The id is a canonical 20-Hangul
  string instead of a 6-Hangul string.
- `CoordId::content_id` rehashes `content_hash + entity + origin + creator`
  and encodes the 256-bit digest injectively into 20 base-11172
  coordinates. The coords are opaque: the semantic axis layout
  (entity/origin/creator on fixed axes) no longer exists in the id.
- `CoordId::resolve` treats a string of exactly 20 Hangul characters as
  canonical; any other string is a label derived through
  `CoordId::from_label`. A legacy 6-Hangul canonical id is therefore
  read as a label and derives a different id.

## Impact on persisted data

The record layer keys files by id:

- `facts/{id}.fact`
- `intents/i_{id}.intent`
- `hints/h_{id}.hint`

Under the new scheme the same logical record derives a different id
string, so files written with 6-syllable ids are not addressable by
their old spelling. Blob files are unaffected: they are keyed by the
content hash (`blob/{hash}.bin`), which did not change.

## Migration options

An automatic in-place tool is not provided, because the id derivation
changed. The pieces that survive in the old files:

- Facts: the old `FactRecord` carries `origin`, `creator`, `blob_hash`.
  The content hash is recoverable from the blob (or parsed from
  `blob_hash` when it is 64-hex). A new id is then derivable with
  `CoordId::content_id(0, origin, creator, content_hash)`.
- Intents and hints: their ids were label-derived; the label derivation
  changed, so their ids must be remapped. Intents also reference facts
  (`from_facts`, `to_fact_id`), so a full migration needs a fact id
  mapping pass applied to those references.

For pre-1.0 development data the recommended path is re-ingestion: load
the old content, derive the new ids through the semantic layer, and
submit. A migration tool is worth building only if production data
exists that cannot be regenerated.

## Verification

`bash run.sh --core` passes after the switch. The breaking boundary is
pinned by the test `six_syllable_strings_are_labels_after_the_depth_migration`
in `fih/tests/semantic_id.rs`.
