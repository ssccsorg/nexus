# nex-tagma

Case PoC: SHA256-free identity generation using the Hangul syllable coordinate space.

## Quick start

    cargo run -p nex-tagma -- check 가
    cargo run -p nex-tagma -- compose 0 0 1
    cargo run -p nex-tagma -- decompose 각
    cargo run -p nex-tagma -- dist 가 각
    cargo run -p nex-tagma -- bench

## Commands

| Command | Description |
|---------|-------------|
| `check <char|hex>` | Validate a Tagma coordinate |
| `compose <i> <m> <f>` | Compose three axis values into a coordinate |
| `decompose <char>` | Decompose a coordinate into (initial, medial, final) |
| `dist <a> <b>` | Field-wise Hamming distance between two coordinates |
| `bench` | SHA256 vs Tagma latency comparison |

## Results

100,000 operations, single-threaded, Rust release, Apple M4:

| Metric | SHA256 | Tagma 1-syllable | Tagma 6-syllable |
|--------|--------|-----------------|-----------------|
| Latency | 237 ns/op | 2 ns/op | 12 ns/op |
| ID size | 32 bytes | 2 bytes | 12 bytes |
| Addressable | 2^256 | 1.12 x 10^4 | 1.94 x 10^24 |
| Collision | probabilistic | deterministic zero | deterministic zero |

## Architecture

- `coord.rs` — TagmaCoord type with compose, decompose, validation, Hamming distance, dense index
- `main.rs` — CLI dispatch

11 integration tests in tests/tagma.rs covering all 11,172 valid coordinates over the full (19 x 21 x 28) space.

## Relationship to Tagma

This is a case PoC — one application of the Tagma principle. Tagma itself (SSCCS's fundamental tag/id pillar) is broader: combinational silicon decoder, 3D SRAM, radiation-tolerant error detection. See tagma/docs/wp.qmd.
