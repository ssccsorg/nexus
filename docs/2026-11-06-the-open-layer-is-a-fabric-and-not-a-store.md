# The open layer is a fabric and not a store

## Context

This extends the layering decisions recorded in #159, #172, and #176, and in
`docs/2026-08-20-thin-nexus-principle.md`. Those moves pushed storage down
inside the crate graph. This one takes the storage implementation out of the
repository: nexus connects, addresses, and records, and it does not carry a
store. Tracking issue: #229.

## The decision

This repository does not implement storage. Holding records, indexing them,
walking a medium, and persisting them are out of scope for this layer, and the
code that does those things is removed from this tree. What remains is the
contract that a deployment implements.

## The test

For a surface in this tree, ask whether the surface can hold records, index
them, walk a medium, or persist durably. If it can, it is out of scope and it
leaves the repository. If it establishes connection, identity, protocol,
lifecycle, or a demonstration of the contract, it stays.

## Evidence in the current code

The surfaces that fail the test are concentrated in one directory and two
verification crates.

| Surface | Location | Role | Action |
|---|---|---|---|
| Record store | `nex/fih/src/core/store.rs` | holds records, flushes, walks | remove |
| Session | `nex/fih/src/core/session.rs` | hydrate, buffer, flush | remove |
| Ordered index | `nex/fih/src/core/index.rs` | index and set intersection | remove |
| Structural search | `nex/fih/src/core/structural.rs` | multi-axis candidate search | remove |
| State bundle | `nex/fih/src/core/export.rs` | packages chain files and blob store | remove |
| Record formats | `nex/fih/src/core/record.rs` | on-disk record encoding | split: transfer encoding stays, medium layout goes |
| Blackboard wrapper | `nex/fih/src/core/fih_blackboard.rs` | synchronous view of the contract | stays |
| Lifecycle | `nex/fih/src/core/intent_status.rs` | transitions with compile-time checks | stays |
| Device verification | `verify/mcu` | runs the read path on an MCU-class target | remove |
| Storage benches | `benches` | measures index and walk | remove |

## The contract

A fabric without a port is not a fabric, so the contract stays.

- the key derivation from a coordinate
- the record encoding used on the wire and in transfer
- the accept rule of the conflict guard, the lifecycle transitions, and the evidence chain
- the operation set: submit, read, query over a coordinate box, enumerate
- conformance tests over the operation set, which test behaviour and not performance

A deployment supplies an implementation of the operation set. The demo store
implements it in memory with a linear scan.

## What the server ships

`nex-server` takes a store. Its default is the demo store: in memory, linear
scan, documented as non-durable.

The open server demonstrates the fabric. Durability is the deployment's
responsibility, and the server documentation and the protocol document say so.

## What leaves the repository

The measurements are properties of the implementation that leaves, so they go
with it.

- the storage benches and their fixtures
- `verify/mcu`
- the whitepaper sections on storage architecture, the memory bound, and measured
  performance, with the storage appendix
- the number set those sections carry: the per-value index footprint, the
  per-fact marginal cost, the flat index size, the per-fact record cost, the
  filter speedups, and the walk peak against a whole-volume read

The fabric keeps its own measured claims: addressing, provenance, lifecycle,
conflict detection, and replay. Those describe behaviour this repository still
carries.

## Order

Milestone 1 defines the operation set as a trait and writes its conformance
tests in this repository.

Milestone 2 completes the conformance tests, so the contract is testable without
the removed modules.

Milestone 3 makes the demo store the server default, so the open tree builds and
tests without the removed modules.

Milestone 4 removes the modules, the device verification, and the storage benches
from this tree.

Milestone 5 revises the whitepaper and removes the storage material from it.

Milestone 6 adds the boundary rule to `AGENTS.md`.

## Gates

- the open tree builds and passes its tests with the demo store only
- the open tree reads no medium and writes no medium
- the conformance tests of the operation set pass during the transition
- the whitepaper stops claiming storage performance once milestone 4 lands

## Risks

Implementations of one contract can drift. The conformance tests stay here, and
a consumer pins this repository, which is the arrangement that already governs
chton.

The open server becomes non-durable by default, which weakens an adoption story
built on running a real store. The limitation is stated in the server
documentation and in the protocol document.

The whitepaper loses its performance evidence at milestone 5. The fabric's own
claims remain measured, and the storage numbers leave with the code they
describe.

## What this does not change

The FIH model, the lifecycle, the coordinate identity, the conflict rule, the
wire protocol, the applications, and the license of this repository stay as they
are. The demo reader in `storage/ref` stays: it reads a volume through the
contract, creates none, and holds no index.
