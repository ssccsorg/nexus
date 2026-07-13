# Tagma Coordinate Space Specification

This document defines the Tagma coordinate space in a language-independent,
implementation-independent manner. Any implementation — Rust, C, assembly,
Verilog — must satisfy these definitions to be Tagma-compatible.

## 1. The Composition Formula

Every valid Tagma coordinate corresponds to a Unicode Hangul syllable code point
via the closed-form composition formula defined in ISO/IEC 10646:

$$C(i, m, f) = \text{U+AC00} + 588i + 28m + f$$

where:

| Variable | Domain | Name |
|----------|--------|------|
| $i$ | $0 \leq i < 19$ | Initial (choseong, 초성) |
| $m$ | $0 \leq m < 21$ | Medial (jungseong, 중성) |
| $f$ | $0 \leq f < 28$ | Final (jongseong, 종성) |

The valid coordinate range is:

$$0 \leq C(i,m,f) - \text{U+AC00} < 11,172$$

Any 16-bit value outside this range is **structurally invalid** and must be
rejected by a Tagma-compatible decoder.

### 1.1 Inverse (Decomposition)

Given a valid code point $c \in [\text{U+AC00}, \text{U+D7AF}]$:

$$
\begin{aligned}
\text{offset} &= c - \text{U+AC00} \\
i &= \text{offset} \div 588 \\
m &= (\text{offset} \bmod 588) \div 28 \\
f &= \text{offset} \bmod 28
\end{aligned}
$$

### 1.2 Validation

A decoder must verify three conditions:

1. $\text{U+AC00} \leq c \leq \text{U+D7AF}$ (range check)
2. $0 \leq i < 19$ (initial bound)
3. $0 \leq m < 21$ (medial bound)
4. $0 \leq f < 28$ (final bound)

Conditions 2-4 are logically redundant if condition 1 holds (every code point
in the block satisfies the axis bounds by construction), but a compliant decoder
must still implement them for hardware fault detection.

## 2. The Coordinate Index

Every valid coordinate maps to a unique 16-bit index:

$$\text{index} = c - \text{U+AC00}, \quad 0 \leq \text{index} < 11,172$$

This index IS the coordinate address. No hashing, no indirection, no collision
resolution.

### 2.1 Field Layout (16-bit)

```
Bit:  15  14  13  12  11  10   9   8   7   6   5   4   3   2   1   0
     ┌───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┬───┐
     │ 0 │      initial (5)      │       medial (5)       │  final (5)  │
     └───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┘
```

- Bit 15: reserved (always 0 for valid coordinates)
- Bits 14-10: initial (0-18)
- Bits 9-5: medial (0-20)
- Bits 4-0: final (0-27)

### 2.2 The Coord Type

`Coord` is the atomic coordinate value: a 16-bit unsigned integer in the range
0..11172, guaranteed to be structurally valid. It represents exactly one Hangul
syllable (one triplet of initial, medial, final axes).

## 3. N-Syllable Composition

A single Coord provides 11,172 unique identifiers. For address spaces larger
than 11,172, multiple Coord values compose into a CoordPath:

$$1 \text{ Coord} = 3 \text{ axes} = 16 \text{ bits} = 11,172 \text{ identifiers}$$
$$N \text{ Coords} = 3N \text{ axes} = N \times 16 \text{ bits} = 11,172^N \text{ identifiers}$$

A CoordPath is an ordered sequence of N independent Coord values, each valid
individually. The axis slots across all N Coords form a flat 3N-dimensional
coordinate space.

### 3.1 Linearization (Row-Major Order)

An N-Coord sequence $(c_0, c_1, \ldots, c_{N-1})$ where each $c_k$ is a valid
Coord index ($0 \leq c_k < 11,172$) maps to a single linear index:

$$\text{linear\_index}(c_0, \ldots, c_{N-1}) = \sum_{k=0}^{N-1} c_k \times 11,172^{\,N-1-k}$$

Equivalently, in iterative form:

```
index = 0
for k = 0 to N-1:
    index = index x 11172 + c_k
```

This is pure arithmetic. No hash function, no collision resolution, no
indirection. The linear index is the address.

### 3.2 O(1) Guarantee

For a fixed N, linearization requires exactly $2(N-1)$ arithmetic operations
($N-1$ multiplications and $N-1$ additions). Since N is known at the call site,
the total cost is $O(1)$ with respect to the size of the addressable space
$S(N) = 11,172^N$.

### 3.3 Examples

| Coords | Linearization | Addressable space |
|--------|--------------|------------------|
| 1 | $\text{index} = c_0$ | $1.12 \times 10^4$ |
| 2 | $\text{index} = c_0 \times 11172 + c_1$ | $1.25 \times 10^8$ |
| 6 | (5 multiplications, 5 additions) | $1.94 \times 10^{24}$ |
| 19 | (18 multiplications, 18 additions) | $1.94 \times 10^{77}$ |

### 3.4 CoordPath: Dynamic-Length Sequence

For N greater than 1, the coordinate space is accessed through CoordPath, a
dynamic-length sequence of Coord values. CoordPath has no fixed compile-time
size; its length is determined at runtime.

CoordPath is distinct from Coord:
- Coord is a single 16-bit value (atomic).
- CoordPath is a sequence of zero or more Coord values (composite).

The linearization formula applies uniformly regardless of CoordPath length.

## 4. Axis Slot Mapping

A sequence of N Coords provides 3N axis slots. Not all slots need to carry
application semantic weight. The mapping from logical application axes to
Coord axis slots is determined by the consumer, not by Coord itself.

Coord is a 16-bit value. It does not interpret its three axis fields as any
particular application-level semantics (e.g. sensor ID, time bucket, coordinate
dimension). That interpretation belongs to the consumer's schema.

### 4.1 Axis-Slot Assignment

Example: a 4-axis identifier requiring 2 Coords:

```
Coord 0:  [axis_0, axis_1, axis_2]    (slots 0, 1, 2)
Coord 1:  [axis_3,     _,     _]      (slots 3, 4, 5)
```

Slots 4 and 5 are unused in this schema. They contain valid Coord values
(every slot in a CoordPath must hold a valid Coord), but their values carry
no semantic weight. The consumer decides which slots to read and which to
ignore.

This is possible because:

1. Every Coord in the path is independently valid (passes hardware decode).
2. The linearization formula accepts all N Coord values regardless of which
   axis slots are semantically active.
3. The consumer projects only the slots it defined in its schema.

### 4.2 Unused Slot Value Convention

Unused slots contain Coord values that are structurally valid. The choice of
value is a consumer policy, not a Coord concern:

| Convention | Value | Use case |
|-----------|-------|----------|
| Zero | `Coord(0)` = `가` = (0,0,0) | Minimal bit pattern, debugging clarity |
| Boundary | `Coord(11171)` = `힣` = (18,20,27) | Distinct from valid data ranges |
| Replicate | Duplicate adjacent active axis value | Predictable for compression |

## 5. Serialization (Base11172)

A Tagma coordinate may be serialized to a printable, self-validating string
using the Base11172 encoding:

- Each coordinate index $0 \leq \text{idx} < 11,172$ maps to exactly one Hangul
  syllable: $\text{char} = \text{U+AC00} + \text{idx}$.
- A pair of syllables encodes a 16-bit value (2 bytes):
  $\text{value} = \text{hi} \times 11172 + \text{lo}$.
- A CoordPath of N Coords serializes to N consecutive Hangul syllables.

The encoding is self-validating: any character outside U+AC00..U+D7AF is
immediately detectable as invalid.

## 6. Compliance

An implementation is Tagma-compatible iff it satisfies all of the following
conditions:

### 6.1 Composition Correctness

The composition formula $C(i,m,f) = \text{U+AC00} + 588i + 28m + f$ must
produce the correct Unicode code point for every valid combination of axes:

$$\forall i \in [0,19),\; \forall m \in [0,21),\; \forall f \in [0,28): \quad
C(i,m,f) \in [\text{U+AC00}, \text{U+D7A3}]$$

### 6.2 Structural Validity

1. Every 16-bit value $v$ in the range $[\text{U+AC00}, \text{U+AC00} +
   11,172)$ must decode to a valid $(i,m,f)$ triplet satisfying the axis bounds
   (Section 1.1).
2. Every 16-bit value $v$ outside this range must be rejected as structurally
   invalid.
3. The rejection includes the 12 filler positions U+D7A4..U+D7AF which lie
   within the Unicode block but outside the composition formula's range.

Total: 11,172 valid values and 54,364 invalid values in the 16-bit space.

### 6.3 Decomposition Correctness

Decomposition must be the functional inverse of composition:

$$\text{decompose}(\text{compose}(i,m,f)) = (i,m,f)$$

for all 11,172 valid triplets.

### 6.4 Linearization Uniqueness

The linearization function $L$ (Section 3.1) must be injective over the product
space:

$$L(c_0, \ldots, c_{N-1}) = L(c'_0, \ldots, c'_{N-1}) \iff
\forall k: c_k = c'_k$$

for any N ≥ 1. Distinct N-Coord tuples must produce distinct linear indices.

### 6.5 Bit-Exactness

All implementations must produce identical results for the same input:

| Operation | Input | Required output |
|-----------|-------|-----------------|
| Composition | $(i,m,f)$ | Same Coord value |
| Decomposition | Same Coord value | Same $(i,m,f)$ |
| Linearization | Same N-Coord tuple | Same linear index |

This must hold across all languages, platforms, and hardware configurations.

### 6.6 Coord Atomicity

Coord is a single-syllable atomic value. An implementation must not:

- Impose application-level semantics on Coord's three axis fields.
- Require Coord to validate or reject axis slot assignments (Section 4).
- Assume any particular storage strategy for CoordPaths.

Coord's only invariant is structural validity (Section 6.2). All higher-level
interpretation is the consumer's responsibility.

---

*Version 2.0 — Tagma Coordinate Space Specification.*
