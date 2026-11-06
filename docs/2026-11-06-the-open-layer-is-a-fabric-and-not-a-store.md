# The open layer is a fabric and not a store

## Context

This extends the layering decisions recorded in #159, #172, and #176, and in
`docs/2026-08-20-thin-nexus-principle.md`. Those moves pushed storage down inside the crate
graph. This one removes it from the repository: nexus is a network of states, and it is
neutral about the medium those states live on. Tracking issue: #229.

## The decision

nexus names states, bounds how a state changes, and answers what a network holds. Where the
records are held is a layer that is implemented using this network. That layer does not live
here.

The distinction is not size or speed. A surface in this tree fails the test when it decides
where a record is held, how it is found, how it is walked, or when it is written out. The
state network decides none of those.

## The test

For a surface in this tree, ask two questions. Does it decide where a record is held, how it
is indexed, how it is walked over a medium, or when it is flushed? Then it is a storage layer
and it leaves. Does it name a state, bound a transition, or answer over states? Then it is the
network and it stays.

## Evidence in the current code

| Surface | Location | Decision |
|---|---|---|
| Record store | `nex/fih/src/core/store.rs` | holds records, so it leaves |
| Session | `nex/fih/src/core/session.rs` | buffers and flushes, so it leaves |
| Ordered index | `nex/fih/src/core/index.rs` | finds records, so it leaves |
| Structural search | `nex/fih/src/core/structural.rs` | answers by axis over held records, so it leaves |
| State bundle | `nex/fih/src/core/export.rs` | packages held state, so it leaves |
| Record formats | `nex/fih/src/core/record.rs` | the medium layout leaves, the transfer encoding stays |
| Device verification | `verify/mcu` | verifies a medium read path, so it leaves |
| Storage benches | `benches` | measure held-state cost, so they leave |
| Lifecycle | `nex/fih/src/core/intent_status.rs` | transitions, so it stays |
| Read surface | `nex/fih/src/contract/state.rs` | answers over states, so it stays |

## What stays

The network itself, which is the part a storage layer is written against.

- the record kinds and the lifecycle transitions
- coordinate identity and the key derivation
- the accept rule of the conflict guard and the evidence chain
- the read surface: one record by identity, and an enumeration that answers without
  materializing the network
- conformance tests over the read surface

Nothing in that list flushes, rebuilds, or names a medium. Those are the storage layer's
operations, and a layer that keeps records supplies them to itself.

## What the server ships

`nex-server` holds its state in memory and is documented as a demonstration of the fabric. A
deployment that needs durability supplies the layer that holds the records, because that is
where durability is decided.

## What leaves the repository

The storage layer takes its own material with it.

- the storage benches and their fixtures
- `verify/mcu`
- the whitepaper sections on storage architecture, the memory bound, and measured
  performance, with the storage appendix
- the number set those sections carry: the per-value index footprint, the per-fact marginal
  cost, the flat index size, the per-fact record cost, the filter speedups, and the walk peak
  against a whole-volume read

The network keeps its own measured claims: addressing, provenance, lifecycle, conflict
detection, and replay. Those describe behaviour this repository still carries.

## Order

Milestone 1 states the read surface as traits and writes its conformance tests here. It is
done.

Milestone 2 makes the demonstration the server default, so the tree builds and tests without
the removed modules.

Milestone 3 removes the modules, the device verification, and the storage benches from this
tree.

Milestone 4 revises the whitepaper and removes the storage material from it.

Milestone 5 adds the boundary rule to `AGENTS.md`. It is done.

## Gates

- the tree builds and passes its tests with the in-memory demonstration only
- the tree holds no record, walks no medium, and writes no medium
- the read surface's conformance tests pass over any implementation a consumer supplies
- the whitepaper stops claiming storage performance once milestone 4 lands

## Risks

The demonstration becomes the only thing this tree can run, which weakens an adoption story
built on running a real volume. The documentation states what the demonstration is for.

The whitepaper loses its performance evidence at milestone 4. The network's own claims remain
measured, and the storage numbers leave with the code they describe.

## What this does not change

The record kinds, the lifecycle, the coordinate identity, the conflict rule, the wire
protocol, the applications, and the license of this repository stay as they are. The demo
reader in `storage/ref` stays: it reads a volume through the read surface, creates none, and
holds no index.
