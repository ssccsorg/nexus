# The address is a value and the text is a boundary

## The question

An identifier has two forms. The value is the coordinate sequence, twenty `u16` axes below 11,172. The text is
its spelling, sixty base-36 characters, three per axis. The two are one to one: `CoordId::from_string` reads the
text back, `Display` writes it, and neither drops information.

A benchmark asked whether holding the value and re-rendering the text on every call costs anything. It does. The
question this note answers is where the choice between the two forms is made, and how the code decides it so a
caller does not have to remember.

## What the code shape already decides

Three properties hold today, and each is a property of the type rather than of a convention.

| Property | Where it is decided |
|:---|:---|
| the value is the authority | `tagma_core::CoordId<20>` is the only identity type in the record layer |
| there is one canonical text | `CoordId::from_string` refuses a wrong length, a byte outside the alphabet, and an axis value above the space |
| the text sorts and prefixes as the value does | fixed width, three digits an axis, alphabet in ascending code point order |

The spelling the owner described as native is the value. The spelling described as ascii is the text. Both are
available at all times, they convert both ways, and a caller chooses per surface. What the code does not do is
hold both at once in a place where one of them is enough.

## The measurement that raised it

The identity text moved from one Hangul syllable an axis to three base-36 digits an axis. The value space did
not move. A controlled comparison of the two commits, with an unchanged benchmark source, put every identity
benchmark within seven percent except one.

| Benchmark | Native text (`772d9d1a`) | Base-36 text (`5095f29b`) | Change |
|:---|---:|---:|---:|
| `fih/intents_by_fact_100` | 26.7 us | 33.4 us | +25% |
| `fih/filter_creator_50k` | 3.871 ms | 3.880 ms | +0.2% |
| `fih/write_10k_facts` | 289 ms | 283 ms | -2.3% |
| `fih/and_query_50k` | 987 us | 955 us | -3.2% |
| `fih/axis_hints_with` | 125 us | 117 us | -6.6% |

One regression, repeated three times, on a lookup that takes a name.

## The diagnosis

`intents_by_fact` took a name and the inverse index was keyed by text.

```rust
let normalized = CoordId::resolve(fact_id).to_string();
self.fact_to_intents.borrow().get(&normalized)
```

Two conversions per call. `resolve` parses sixty characters, or hashes the name into a label when it is not
canonical. `to_string` renders sixty characters back, and the map then hashes them. The text was already the
key, and the caller already held the value or could hold it, so both conversions were avoidable.

The cost follows the text length, which is why the notation change surfaced it and the value change did not.
Sixty characters is three times twenty, and the render was the larger half of the pair.

## The policy

Four rules, each one enforceable by the compiler rather than by review.

| Rule | What enforces it |
|:---|:---|
| an internal container is keyed by the address | the field type; a `String` cannot be inserted without editing the field |
| text is resolved once, at the boundary of a surface | one resolve per text entry point, then delegation to a typed function |
| a typed door stands beside every text door | the function signature; a caller holding a `CoordId` cannot reach the parse |
| a text door states that it converts | its doc comment; the cost is visible where the choice is made |

The first rule is the one that matters. A map keyed by `CoordId` makes the fast path the only path inside the
module. The third rule gives an outside caller the same path once it holds the value. Neither rule asks a
developer to remember which form is cheaper at a call site.

`CoordId` carries what a key needs: `Copy`, `Eq`, `Hash`, and `Ord`. The last one is required because the
no_std build uses a `BTreeMap` where the host build uses a `HashMap`, and both builds take the same index.

## What was applied

The inverse index is keyed by the address. The text is resolved once, where the intent record arrives.

```rust
fact_to_intents: Cell2<HashMap<CoordId<20>, Vec<String>>>
```

A lookup by name resolves once and delegates. A lookup by address is the typed door.

```rust
pub fn intents_by_fact(&self, fact_id: &str) -> Vec<String> {
    self.intents_by_fact_id(CoordId::<20>::resolve(fact_id))
}

pub fn intents_by_fact_id(&self, fact: CoordId<20>) -> Vec<String> { ... }
```

| Benchmark | Before | After | Change |
|:---|---:|---:|---:|
| `fih/intents_by_fact_100` | 33.4 us | 13.0 us | -61% |

The result is below the 26.7 us the native spelling measured, because the index now avoids both conversions
and the native form paid a render of its own.

The link and unlink paths pay one resolve per `from_facts` entry, once per record placement. That is the
opposite trade and the correct one: a record is placed once and looked up many times.

## What was measured and left

Nothing. The two filter benchmarks and the ktema read path this note first recorded as left are applied here.

## The record layer, keyed by the address

The three record maps and the structural index hold `CoordId`. A text entry point resolves once and delegates;
everything behind it compares values.

| Change | Effect |
|:---|:---|
| `fact_records`, `intent_records`, `hint_records` keyed by `CoordId` | a lookup that holds an address renders nothing |
| `WantedIds` holds addresses and compares them | the wanted text is parsed once per call, not once per candidate |
| the structural index holds addresses | the structural path stops parsing the candidates it materializes |
| `CoordId` defaults to `COORD_ID_DEPTH` | the depth literal is gone, and one constant sets the width |
| the by-name reads take the typed doors | a name is parsed once and rendered once, and the store parses nothing |

| Benchmark | Text keys | Address keys | Change |
|:---|---:|---:|---:|
| `fih/wanted_ids/scan` | 133.6 us | 95.6 us | -28% |
| `fih/wanted_ids/struct` | 1.536 ms | 1.015 ms | -34% |
| `fih/intents_by_fact_100` | 33.4 us | 13.0 us | -61% |

The structural path is the one that shows why the container matters and the argument alone does not. Keying only
the record maps made `struct` 50% slower, because the index handed over text that the lookup then parsed. The
index is a container too, so it took the address, and the 50% became a 34% gain.

Three things are unchanged by design.

- The serialized record keeps the text, so no volume changes and no migration exists.
- Sorting by the key keeps its order, because the notation is order preserving. A map keyed by the address
  sorts as a map keyed by its text did, which is what let the sort expressions stay as they were.
- The text a caller prints is the id the record already carries, so iteration renders nothing it did not
  already hold.

The cost moves to the boundary: a text entry resolves once per call, and `place_record` resolves once per record
it places. A record is placed once and read many times, so the trade faces the reads.

## Verification

```bash
cargo test --workspace --no-fail-fast          # run it with TMPDIR short, see below
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p nexus-bench --bench bench -- "fih/intents_by_fact_100"
cargo bench -p nexus-bench --bench wanted_ids
```

The `nexd` integration tests bind a Unix socket under the system temporary directory. A deep `TMPDIR` exceeds
`SUN_LEN`, the server fails to bind, and all twenty-eight tests fail with a message that names the path rather
than the length. Run them with `TMPDIR=/tmp` in a sandbox that sets a long temporary path; they pass in 3.6
seconds once the socket fits.

The measurement set is sensitive to the machine's load. Six runs over four builds produced 1000-record query medians
of 127.2, 127.5, 127.8 and 180.4, 201.0 microseconds on code paths that differ by nothing that touches that query.
The slow runs were taken while builds were running in other shells. A comparison counts only between runs taken
on an idle machine, and two runs that agree within a few percent are worth more than one run that does not.

Against the text-keyed build, with both sides measured on an idle machine, the ktema set is neutral.

| Volume | Metric | Text keys | Address keys |
|:---|:---|---:|---:|
| 1000 records | query | 127.2 us | 127.8 us |
| 1000 records | point read | 1.084 us | 1.083 us |
| 10000 records | query | 2638 us | 2583 us |
| 10000 records | point read | 1.625 us | 1.583 us |

The answers and the medium bytes are identical in both builds, which is the property that makes the comparison
meaningful at all.

For a comparison across commits, build the earlier commit in a worktree and run the same filter in both trees.
The criterion baseline is the previous run in the same target directory, so two runs of one tree compare a tree
to itself and the resulting percentages measure load rather than code. That mistake produced a set of numbers
between +2% and +339% on unchanged source before the worktree comparison replaced them.

## Numbers

| Quantity | Value | Source |
|:---|:---|:---|
| axes | 20 | `COORD_ID_DEPTH` |
| values an axis carries | 11,172 | `Coord::N_VALID` |
| characters an axis takes | 3 | `DIGITS_PER_AXIS` |
| text length | 60 bytes | 20 axes times 3 characters, and every character is one byte |
| native text length it replaced | 60 bytes | 20 syllables at three bytes each |
| value size | 40 bytes | 20 axes at two bytes each |

The byte length is the same in both spellings, which is why the medium's numbers did not move. The conversion cost
did move. The base-36 parse reads three characters an axis where the syllable parse reads one, and the render
writes three digits an axis where the syllable render writes one code point.
