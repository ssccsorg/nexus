# nex-tagma

Reference hub that consumes Tagma coordinate space on the neXus FIH storage layer.

## Quick Start

```
cargo run -p nex-tagma -- check 가
cargo run -p nex-tagma -- compose 0 0 1
cargo run -p nex-tagma -- decompose 각
cargo run -p nex-tagma -- dist 가 각
cargo run -p nex-tagma -- bench
```

## Commands

| Command | Description |
|---------|-------------|
| `check <char\|hex>` | Validate a Tagma coordinate |
| `compose <i> <m> <f>` | Compose three axis values into a coordinate |
| `decompose <char>` | Decompose a coordinate into (initial, medial, final) |
| `dist <a> <b>` | Field-wise Hamming distance between two coordinates |
| `bench` | SHA256 vs Tagma latency comparison |

## Benchmark

100k operations, single-threaded, Rust release, Apple M1:

```text
Benchmark: 100000 operations
  Method                    Latency      ns/op
  --------------------------------------------
  Tagma 1-syll         186.375µs        2 ns
  Tagma 2-syll         216.875µs        2 ns
  Tagma 6-syll         1.175459ms       12 ns
  Tagma 19-syll         3.715ms       37 ns
  SHA256               23.501208ms      235 ns

Speedup (vs SHA256):
  1-syll:   126x  (space: 1.1e4)
  6-syll:   20x  (space: 1.9e24, UUID-scale)
  19-syll:  6x  (space: 2^256, SHA256-equivalent)
```

| Metric | SHA256 | Tagma 1-syll | Tagma 2-syll | Tagma 6-syll | Tagma 19-syll |
|--------|--------|-------------|-------------|-------------|--------------|
| Latency | 227 ns/op | 2 ns/op | 2 ns/op | 11 ns/op | 35 ns/op |
| ID size | 32 bytes | 2 bytes | 4 bytes | 12 bytes | 38 bytes |
| Addressable | 2^256 | 1.12e4 | 1.25e8 | 1.94e24 | 2^256 |
| Speedup vs SHA256 | -- | 115x | 115x | 20x | 6x |

## Architecture

- `src/coord.rs` -- `Coord` type with compose, decompose, validation, Hamming distance, dense index
- `src/main.rs` -- CLI dispatch
- `tests/tagma.rs` -- 20 integration tests covering all 11,172 valid coordinates

## Design Doc

Detailed design including 3D FIH mapping, property propagation, evolution phases, and identity flow is documented in `ssccs/docs/projects/nexus/apps/tagma.qmd`.
