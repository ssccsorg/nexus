# nex

The nested workspace for the state model, the IO seam, the reference store, and the process
layer.

| Crate | Role |
|---|---|
| `fih` (`fih-model`) | `Fact`, `Intent`, `Hint`, `CoordId`, and the capability traits. `no_std + alloc` |
| `core` (`nex-core`) | the clock and error primitives a port supplies and the model consumes |
| `fih` (`nex-fih`) | the model with the read surface and the contract |
| `io` (`nex-io`) | the flat key-space IO seam |
| `process` (`nex`) | the process layer: scheduling and detection over a blackboard |

A storage implementation is a node, and it is not here. `storage/ref` is the reference node: it
keeps records in maps, writes them at the keys the format publishes, and answers the read
surface, so a consumer sees the shape before choosing an implementation. Durability, an index
over the axes, and a bounded device read are a storage layer's decisions.

The state model builds with no operating system. `cargo check -p nex-io --no-default-features
--target riscv32imac-unknown-none-elf` is the anchor the check script runs, and it covers the
crates that declare themselves portable.
