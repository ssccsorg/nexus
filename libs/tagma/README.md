# Tagma

**Content‑addressable structural primitive defined by the Hangul syllabic space.**

Tagma is a production-level, `no_std` Rust foundation library (Apache 2.0) that replaces hash-based identity generation with a combinational decoder over a fixed 16‑bit coordinate space. The coordinate space is an open international standard (Unicode) and a public good — the Hangul syllable block (U+AC00–U+D7AF).

## Reference implementation

The reference implementation is written in Rust, targets `no_std` + `alloc`, and is resource-competitive with C across all dimensions. It builds for native targets and `wasm32-unknown-unknown`.

| Component | Description | Location |
|-----------|-------------|----------|
| **TagmaCoord** | 16‑bit coordinate (0..11172), 3‑axis decomposition, Hamming distance | `sw/rust/core/src/coord.rs` |
| **TagmaMap\<V\>** | Direct-address table, O(1) worst-case, Entry API, drain, retain | `sw/rust/core/src/map.rs` |
| **TagmaSet** | Bit array over 11,172 slots, bitwise union/intersection/difference | `sw/rust/core/src/set.rs` |
| **base11172** | Self-validating serialization: coordinate ↔ Hangul string | `sw/rust/base11172/` |

Total: **103 unit tests**, all passing. CI: `cargo fmt --check` → `cargo clippy` → `cargo build --release` → `cargo test --release`.

## Repository structure

```
tagma/
├── docs/               # White paper (wp.qmd) and master document (index.qmd)
├── sw/rust/            # Rust workspace
│   ├── core/           # tagma-core — TagmaCoord, TagmaMap, TagmaSet
│   └── base11172/      # Native serialization format
├── hw/                 # Verilog decoder, XIF interface, 3D array (future)
├── poc/                # Archived early proofs of concept
├── run.sh              # Single-entry CI pipeline
└── Makefile
```

## Quick start

```sh
git clone https://github.com/ssccsorg/tagma
cd tagma
./run.sh                # fmt → clippy → build → test
```

Or directly:

```sh
cd sw/rust
cargo test --release    # Run all 103 tests
cargo bench -- stress   # 500k mixed-operation stress benchmark
```

## Documentation

- **[White Paper](docs/wp.qmd)** — Full technical analysis: coordinate space, decoder, hardware implementation, benchmarks
- **[Master Document](docs/index.qmd)** — Project overview, paradigm shift, core data structures
- **Rustdoc** — `cargo doc --no-deps -p tagma-core` for API reference

## License

Apache 2.0 — see [LICENSE](LICENSE).
