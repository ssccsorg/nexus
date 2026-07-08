# nex-tagma

Case PoC: SHA256-free identity generation using the Hangul syllable coordinate space.

## Quick start

    cargo run -p nex-tagma -- check 가
    cargo run -p nex-tagma -- compose 0 0 1
    cargo run -p nex-tagma -- decompose 각
    cargo run -p nex-tagma -- dist 가 각
    cargo run -p nex-tagma -- bench

Issue: [#150](https://github.com/ssccsorg/nexus/issues/150) — case PoC for SHA256-free identity generation.

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

| Metric | SHA256 | Tagma 1-syll | Tagma 2-syll | Tagma 6-syll | Tagma 19-syll |
|--------|--------|-------------|-------------|-------------|--------------|
| Latency | 227 ns/op | 2 ns/op | 2 ns/op | 11 ns/op | 35 ns/op |
| ID size | 32 bytes | 2 bytes | 4 bytes | 12 bytes | 38 bytes |
| Addressable | $2^{256}$ | $1.12 \times 10^4$ | $1.25 \times 10^8$ | $1.94 \times 10^{24}$ | $2^{256}$ |
| Use case | — | Sensor tags | DB records | UUID scale | SHA256-equivalent |
| Speedup vs SHA256 | — | **115x** | **115x** | **20x** | **6x** |

## Architecture

- `coord.rs` — TagmaCoord type with compose, decompose, validation, Hamming distance, dense index
- `main.rs` — CLI dispatch

20 integration tests in tests/tagma.rs covering all 11,172 valid coordinates over the full (19 x 21 x 28) space, plus dense index, parse_val, and benchmark verification.

## Relationship to Tagma

This is a case PoC — one application of the Tagma principle. Tagma itself (SSCCS's fundamental tag/id pillar) is broader: combinational silicon decoder, 3D SRAM, radiation-tolerant error detection. See tagma/docs/wp.qmd.
