# nex-calc as the design basis: from the standard skeleton to deterministic path accumulation

## Context

nex-calc is the pure and standard proof of the nex concept. It reduces the
FIH record model to a minimal complete skeleton: `put 3` creates a Fact at a
coordinate, `put 5` creates another, `add` creates an Intent that is a
directional vector, and `resolve` traverses Fact to Intent to Fact, producing
a new Fact that persists. The algebraic shape is F x I x H -> F'. Nothing is
added to the model; the calculator is the model, running unchanged.

The role of such a skeleton is not to be an efficient artifact. It is to be
the design basis. Complex systems are designed by extending this minimal
semantics, composing new Intents and Hints over the same immutable record,
rather than by inventing a separate execution model. What nex-calc proves,
purely and in the standard form, is that the nex concept is complete enough
to compute; what the skeleton provides is the reference shape that complex
designs inherit.

Two consequences follow from the skeleton.

The first is that computation is accumulated knowledge. An operation is a
traversal recorded as an immutable Fact, the record is the state, and the
traversal is the computation. This consequence is established by the skeleton
itself and by the record layer it runs on.

The second is that the same record semantics can ground systems that learn.
An agent records the Intents it proposes, the Facts it observes, and the
Hints that bound the search, and knowledge grows by accumulation of recorded
paths instead of by overwriting continuous weights. This consequence is a
hypothesis, and this document turns it into a falsifiable research plan.

External evidence for the structural family on real hardware is measured in
the ssccs docs: the coordinate-addressed ROOT read path drops the full-file
read on CMS Run2016G DoubleMuon NanoAOD from 192.8 s to 1.06 s (181.7x),
raising throughput from 11.2 MB/s to 5,584 MB/s. The scope of that result is
coordinate addressing, not FIH record semantics, and the boundary is kept
explicit throughout.

## The skeleton as the design basis

A complex system built on the nex concept does not replace the record model;
it composes it. Arithmetic operators in nex-calc are Intents, and the
vector and transform placeholders in `src/ops.rs` mark the same extension
point for matrix and signal operations. Constraints are Hints that bound
traversal. Every new capability is a new Intent or Hint over the same
immutable Facts, and the properties the skeleton demonstrates, that replay
equals computation and that no Fact is overwritten, are the properties a
complex design is required to preserve.

The record layer in `nex/fih` is the substrate the skeleton runs on and the
complex system inherits: immutable facts, the content-hash conflict guard in
`submit_fact`, the `fact_to_intents` inverse index, the deterministic
`rebuild_cache` replay, and `scan_partition` per-channel flows. The L2
restructure devlog records the measured cost of the substrate: the per-fact
heap dropped from about 3.4 MB to about 40 KB, and the conflict check dropped
from 135 to 390 ms per call to 445 ns.

## Claims decomposed

The fact and conjecture boundary matters because the insight blends both.

Facts established in this repository: nex-calc is a pure, minimal, and
standard proof of the nex concept; the FIH store owns the record semantics
listed above; and the L2 restructure measurements place the cost of the
substrate.

Facts established in the ssccs docs: the coordinate-addressed ROOT fork
achieves the 192.8 s to 1.06 s result, validating coordinate addressing on
legacy hardware. The boundary is explicit: this is not by itself evidence for
FIH record semantics.

Conjectures that this plan will test: that complex system designs inherit the
skeleton semantics without a separate execution model; that a computation is
fully reconstructible from its FIH record alone; that a decision procedure
can learn by path accumulation at any competitive cost; and that the record
semantics generalize from arithmetic to learning. These are hypotheses, not
results.

## Research questions

1. What is the smallest FIH substrate on which a decision procedure learns by
   path accumulation? nex-calc covers arithmetic; the next step is a search or
   constraint procedure whose explored paths accumulate as Facts.
2. What does deterministic replay cost? The property is claimed; the cost must
   be measured per step, per episode, and per accumulated history.
3. Where is the boundary between path-accumulated knowledge and stochastic
   optimization? SGD optimizes continuous high-dimensional functions;
   path accumulation is discrete. The boundary is the deliverable, not an
   assumed superiority.
4. Is accumulation the point or the cost? Knowledge as the record grows
   without bound; the structural filter index exists precisely to keep access
   sublinear in the record. Whether that holds under a learning workload is an
   open measurement.

## Executable plan

Phase 1: formalize the skeleton as the conformance reference. Write a
specification that a computation is a path of Facts connected by Intents and
bounded by Hints, and add tests asserting that a nex-calc run is fully
reconstructible from the FIH record alone. Replay equals computation is the
first falsifiable claim, and the skeleton is the conformance target that any
complex design must pass.

Phase 2: build a path learner on the FIH store. A rule induction or
constraint search procedure over the existing structural filter index, where
every step is an Intent and every outcome a Fact. Measure three properties:
replay determinism, in which identical inputs produce an identical recorded
path; cumulative growth, in which Facts are added and none are overwritten;
and auditability, in which any output resolves to its full recorded path.

Phase 3: benchmark the learning trace on the existing store surfaces. The
relevant numbers are the per-step record cost, path reconstruction latency
through `rebuild_cache` and the inverse index, and storage growth per learning
episode. This converts the conceptual claim into measured behavior.

Phase 4: run the learner on a constrained target. The osless MCU storage line
tests whether the record fits where memory is tight, mirroring the role the
ROOT result played for coordinate addressing.

Phase 5: express one small SGD-trained function as accumulated paths. A
linear regression or a small classifier is enough. Compare reproducibility,
auditability, accuracy, and cost. The purpose is to find the boundary, not to
declare a winner.

## Falsifiability

The thesis fails in specific, testable ways. If reconstructing a recorded
computation requires information outside the FIH record, such as timing,
ordering, or hidden state, then computation equals record is false for that
case. If path-accumulated learning cannot match a trivial SGD baseline on a
toy problem at any cost, the learning extension is unsupported. Both
conditions are measurable in the phases above.

## Counterpoints

1. Efficiency. Recording everything is expensive, and nex-calc is
   intentionally inefficient. The skeleton is a reference, not a product; the
   claim is not that the record is free, it is that the record is the value.
   Cost must be measured, and the measurement may reject the claim at scale.
2. Evidence scope. The CERN result is coordinate addressing, not FIH record
   semantics. It supports the structural family but does not by itself
   validate computation as record. The boundary is marked, not blurred.
3. The nature of learning. Stochastic optimization adjusts continuous
   functions; path accumulation is discrete and symbolic. The generalization
   from arithmetic to learning is a hypothesis that the experiments are
   designed to test, and it may fail on representational grounds.

## Judgment

Analytic conclusion: nex-calc is a pure and standard proof of the nex
concept, and its value as a skeleton is that complex designs inherit the
properties it demonstrates. The record semantics in the store, the immutable
facts, the content-hash conflict guard, and the deterministic replay, are the
substrate on which replay and audit can be built and tested.

Value judgment, hedged: treating the skeleton as the design basis is the
correct posture for this repository, and one bounded experimental program is
worth running, because the reproducibility and auditability properties are
testable and valuable even if the strong learning claim weakens. The ROOT and
nex-calc artifacts are the starting evidence, and the phases above are the
next step.

## References

- `apps/nex-calc`: README and `src/ops.rs`, this repository
- `docs/2026-08-20-thin-nexus-principle.md`: the storage-down, semantics-up
  principle and the L2 restructure measurements
- `nex/fih`: the record layer, conflict guard, inverse index, and
  deterministic replay
- SSCCS docs, CERN ROOT TTree report: the 192.8 s to 1.06 s measurement
- SSCCS docs, synTagma project index: the ROOT TTree appendix with the full
  measurement table
