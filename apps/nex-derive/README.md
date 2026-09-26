# nex-derive

A goal derives a fact. The goal is the product of work, intent, and contract;
resolving a goal derives a fact, and the derivation is kept as the record
`goal -> fact`.

## Concept

The FIH primitives read as a directed relation. A goal is a point in the
product space of work, intent, and hint (the contract). Resolving a goal
derives a fact, and the derivation is the pair `(goal, fact)`. The direction is
the point of the app, so the record carries `goal -> fact` rather than a set of
co-equal items.

The intersection that resolves a goal is symmetric: it does not know which
condition is the premise. That is fine, because the intersection is the inner
step and the direction lives in the record. A name taken from the intersection
would describe the step and not the relation, so the name is taken from the
relation.

nex-calc executes the relation once, as a state transition on an Intent
(submit, claim, conclude) that writes the resulting Fact. nex-derive keeps the
derivation instead, so an accumulated board can be read back and a conjunctive
goal resolved against it. Neither app carries a rule engine; the resolution is
an intersection of posting bitsets.

```
derive parse plan strict   → goal (work=parse, intent=plan, hint=strict) derives a fact
resolve parse * strict     → every goal with work=parse and hint=strict, by intersection
```

## Architecture

| Primitive | Role | nex-derive mapping |
|-----------|------|-----------------|
| F (Fact)  | Immutable data at a coordinate | The fact a goal derives, recorded with its goal |
| I (Intent)| Directional function | The intent axis: the direction the goal carries |
| H (Hint)  | Constraint or transform | The hint axis: the contract that bounds the goal |

A goal is the product of the three axes. The board keeps one derivation per
goal, over one coordinate index space:

| Structure | Role |
|-----------|------|
| occupancy     | The goals that hold a derivation |
| postings      | One bitset per axis value: the goals carrying that value |
| slots         | Goal coordinate to derivation index, for reading it back |

A conjunctive goal is resolved by the AND of the postings its conditions name.
The result is exact, so no candidate filter runs afterwards.

## Usage

```bash
cargo run -p nex-derive
```

```
> define work parse scan audit
work[0] = parse
work[1] = scan
work[2] = audit
> define intent plan execute
intent[0] = plan
intent[1] = execute
> define hint strict lax
hint[0] = strict
hint[1] = lax
> derive parse plan strict design the parser
derived 0 [work=parse intent=plan hint=strict] -> design the parser
> derive scan execute lax scan, best effort
derived 4161 [work=scan intent=execute hint=lax] -> scan, best effort
> resolve parse * *
resolved: 1 derivation(s) over 2 goal(s), 1 constrained axis/axes
  [work=parse intent=plan hint=strict] -> design the parser
scan agrees: yes (1)
> explain parse * *
candidate elimination:
  goals               2
  intersect work      1
  intersect intent    1
  intersect hint      1
> bench
       n    goals    matched  intersect/q       scan/q   speedup  agree
    1000      500      16913     13.585µs      1.772µs      0.1x  yes
   10000      500     205861     13.739µs     15.328µs      1.1x  yes
   50000      133     271942     16.491µs     83.223µs      5.0x  yes
  200000       33     225637     14.287µs    371.233µs     26.0x  yes

resolution cost is set by the geometry (4096 words per posting and
64 values per axis), so it stays flat as goals grow while
the scan rises with the goal count.
```

The bench figures are one run on an Apple M1 in the dev profile (optimized plus
debuginfo), so read the shape rather than the absolute values.

## Cost

The resolution cost is set by the geometry, not by the goal count:

- A single-value condition costs one posting copy plus one AND, so `WORDS`
  word operations, where `WORDS` is `COORD_SPACE / 64`.
- An unconstrained axis (`*`) is skipped entirely; it narrows nothing.
- The scan costs one predicate per goal per constrained axis, so its cost rises
  with the goal count.

The bench sweep shows the two lines crossing. At 1,000 goals the scan wins by
roughly an order of magnitude, the two are even near 10,000, and above that the
resolution dominates and the gap widens. That crossover is the honest
statement: the resolution is not faster in every regime, it is faster once the
goal count exceeds the coordinate space in words.

The resolution's absolute cost is dominated by copying the posting and
occupancy bitsets, so it is bounded by the geometry (`WORDS` words per bitset)
and not by the goal count. That is why the resolution column stays near 14
microseconds from 1,000 goals to 200,000 while the scan column rises by more
than two orders of magnitude.

## Boundary

The resolution is the whole computation inside a stated fragment, and the
boundary is part of the design rather than a caveat:

- Conjunction only. Negation and disjunction leave the fragment; a negated
  condition is a difference of sets, which is still cheap, but it breaks the
  monotone reading that makes the resolution self-explanatory.
- Materialized and monotone. A derivation is written once per goal. Retraction
  and update need the postings to be maintained.
- Fixed radix. Each axis has `AXIS_CARD` values and one derivation per goal.
  Choosing the encoding so the axes are independent and the radix is fixed is
  the real work, and it is the same problem a one-axis Tagma index solves by
  fixing the coordinate space.
- One derivation per goal. A goal resolves to a single fact. Several facts for
  one goal need a second index on top, which this app does not carry.
- No recursion, aggregation, ranking, or temporal validity. Transitive closure
  needs a fixpoint; a count needs a pass over the result; a rank needs an order.
  Each one is machinery on top of the resolution, and this app does not carry
  it.

The app records derivations and resolves them. It does not compute a fact from
its premise; producing a new fact is `nex-calc`'s job. `nex-derive` is the
ledger of `goal -> fact` that a resolution reads.

`nex-tagma` is the same intersection at a larger coordinate space for one axis;
`nex-derive` is the minimal directed three-axis form, written without
dependencies so the structure is legible on its own.

## Build & Test

```bash
cd apps/nex-derive
cargo build
cargo test
cargo run
./run.sh --demo
```
