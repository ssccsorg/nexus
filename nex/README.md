# nex

The nested workspace for the state model, the IO seam, the reference store, and the process
layer.

| Crate | Role |
|---|---|
| `fih` (`fih-model`) | `Fact`, `Intent`, `Hint`, `CoordId`, and the capability traits. `no_std + alloc` |
| `core` (`nex-core`) | the clock and error primitives a port supplies and the model consumes |
| `fih` (`nex-fih`) | the model with the read surface, the contract, and the reference store |
| `io` (`nex-io`) | the flat key-space IO seam |
| `process` (`nex`) | the process layer: scheduling and detection over a blackboard |

The reference store in `nex-fih` is not the storage implementation. It keeps records in maps,
writes them at the keys the format publishes, and answers the read surface, so a consumer sees
the shape before choosing an implementation. Durability, an index over the axes, and a bounded
device read are a storage layer's decisions, and that layer is a node on the network rather
than a backend of the process layer.

The state model builds with no operating system. `cargo check -p nex-io --no-default-features
--target riscv32imac-unknown-none-elf` is the anchor the check script runs, and it covers the
crates that declare themselves portable.
