# Tagma

**Content-addressable structural primitive defined by the Unicode Hangul syllable block.**

Tagma replaces hash-based identity generation with direct structural addressing over a fixed 16-bit coordinate space. Every valid 16-bit value in the Hangul syllable block (U+AC00--U+D7AF) decomposes into three independent axes (initial, medial, final), serving simultaneously as a 1-D address and a 3-D coordinate. The reference implementation is a `no_std` + `alloc` Rust library.

## Core types

| Type | Description | File |
|------|-------------|------|
| **Coord** | 16-bit atomic coordinate (0..11172), 3-axis composition/decomposition, Hamming distance, Hangul display | `core/src/coord.rs` |
| **CoordMap\<V\>** | Direct-address table, O(1) worst-case, no hashing. Entry API, drain, retain, IntoIterator | `core/src/map.rs` |
| **CoordSet** | Bit array over 11,172 slots (1.4 KB, no allocator). Union, intersection, difference, subset tests | `core/src/set.rs` |
| **base11172** | Self-validating serialization: arbitrary bytes to Hangul syllable strings | `base11172/src/lib.rs` |

Test coverage: 103 unit tests + 14 doc-tests, all passing. CI runs `cargo fmt --check`, `cargo clippy`, `cargo build --release`, `cargo test --release`.

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
cargo bench -p tagma-benchmarks -- stress  # 500k mixed-operation stress benchmark
```

## Usage

```rust
use tagma_core::{Coord, CoordMap, CoordSet};

// Compose a coordinate from three axes
let c = Coord::from_axes(5, 10, 15).unwrap();
assert_eq!(c.to_axes(), (5, 10, 15));
assert_eq!(c.to_char(), '걐');  // Hangul syllable display
assert_eq!(c.hamming_distance(c), (0, 0, 0));

// Direct-address map (no hashing, O(1) worst-case)
let mut map = CoordMap::new();
map.insert(c, "tagma");
assert_eq!(map.get(c), Some(&"tagma"));

// Bit-array set
let mut set = CoordSet::new();
set.insert(c);
assert!(set.contains(c));
```
 
## How it works

A Tagma coordinate is computed from three structural axes via the Hangul composition
formula defined in ISO/IEC 10646:

$$C(i,m,f) = \text{U+AC00} + 588i + 28m + f, \quad 0 \leq i < 19,\; 0 \leq m < 21,\; 0 \leq f < 28$$

Of 65,536 representable 16-bit states, only 11,172 satisfy this formula. The remaining
54,364 are structurally invalid and hardware-detectable. Each valid value is:

- A 1-D address (Unicode code point) for flat array indexing.
- A 3-D coordinate (initial, medial, final) for structural queries.
- A Hangul syllable for human-readable display.

N-syllable sequences (CoordPath) extend the address space to $11172^N$ identifiers
via row-major linearization. A 6-syllable identifier covers UUID-scale space;
19 syllables match SHA-256's $2^{256}$ identifier space at 6x lower latency in
software (Apple M1: 35 ns vs 227 ns).

## Documentation

- **[White Paper](docs/wp.qmd)** — Full technical analysis: coordinate space, decoder, hardware implementation, benchmarks
- **[Master Document](docs/index.qmd)** — Project overview, paradigm shift, core data structures
- **[Specification](spec/coord-space.md)** — Language-independent coordinate space definition
- **Rustdoc** — `cargo doc --no-deps -p tagma-core` for API reference

## License

Apache 2.0 — see [LICENSE](LICENSE).
