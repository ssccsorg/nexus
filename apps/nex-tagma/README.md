# nex-tagma

Standard reference implementation that bridges the Tagma three-axis coordinate
space and the neXus FIH three-dimensional storage. It demonstrates that the
O(1) direct addressing, collision-free uniqueness, axis decomposition, and
proximity search properties of the Tagma syllabic coordinate system form an
isomorphic mapping with the Fact-Inference-Hint three-dimensional cube.

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

100k operations, single-threaded, Rust release, ARMv8.4-A Firestorm:

| Metric | SHA256 | Tagma 1-syll | Tagma 6-syll | Tagma 19-syll |
|--------|--------|-------------|-------------|--------------|
| Latency | 227 ns/op | 2 ns/op | 11 ns/op | 35 ns/op |
| ID size | 32 bytes | 2 bytes | 12 bytes | 38 bytes |
| Addressable | 2^256 | 1.12e4 | 1.94e24 | 2^256 |
| Speedup vs SHA256 | -- | ~115x | ~20x | ~6x |

## Dependencies

- **tagma-core** — Core TagmaCoord type, composed via `libs/tagma` subtree
  from [github.com/ssccsorg/syntagma](https://github.com/ssccsorg/syntagma)
- **sha2** — SHA256 baseline for benchmark comparison

## Design Doc

Detailed design including 3D FIH mapping, property propagation, evolution
phases, and identity flow is documented in
`ssccs/docs/projects/nexus/apps/tagma.qmd`.
