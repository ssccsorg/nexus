# Tagma Coordinate Space Specification

This document defines the Tagma coordinate space in a language-independent,
implementation-independent manner. Any implementation — Rust, C, assembly,
Verilog — must satisfy these definitions to be Tagma-compatible.

## 1. The Composition Formula

Every valid Tagma coordinate corresponds to a Unicode Hangul syllable code point
via the closed-form composition formula defined in ISO/IEC 10646:

$$C(i, m, f) = \text{U+AC00} + 588i + 28m + f$$

where:

| Variable | Domain | Name | Example |
|----------|--------|------|---------|
| $i$ | $0 \leq i < 19$ | Initial (choseong, 초성) | $i=0 \to \text{ㄱ}$ |
| $m$ | $0 \leq m < 21$ | Medial (jungseong, 중성) | $m=0 \to \text{ㅏ}$ |
| $f$ | $0 \leq f < 28$ | Final (jongseong, 종성) | $f=0 \to \text{(none)}$ |

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

Conditions 2--4 are logically redundant if condition 1 holds (every code point
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
- Bits 14--10: initial (0--18)
- Bits 9--5: medial (0--20)
- Bits 4--0: final (0--27)

## 3. N-Syllable Product Space

A single syllable provides 11,172 unique coordinates. For larger address spaces,
syllables compose via the Cartesian product:

$$S(N) = 11,172^N$$

### 3.1 Linearization (Row-Major Order)

An N-syllable coordinate $(c_1, c_2, \ldots, c_N)$ where each $c_k$ is a valid
single-syllable coordinate index ($0 \leq c_k < 11,172$) is mapped to a linear
index by:

$$\text{linear\_index}(c_1, \ldots, c_N) = \sum_{k=1}^{N} c_k \times 11,172^{\,N-k}$$

Equivalently, in iterative form:

```
index = 0
for k = 1 to N:
    index = index * 11172 + c_k
```

### 3.2 O(1) Guarantee

For a fixed N, the linearization requires exactly:

$$(N-1) \text{ multiplications} + (N-1) \text{ additions}$$

This is $O(N)$ arithmetic operations. Since $N$ is a compile-time constant
(number of syllables), the total cost is $O(1)$ with respect to the size of the
addressable space $S(N)$.

**Proof sketch:**

1. The linear index is computed by a fixed sequence of $2(N-1)$ arithmetic
   operations.
2. No operation depends on the contents of the stored data; only on the
   coordinate values themselves.
3. After computing the index, accessing the backing array is $O(1)$
   (direct address, no probing, no resolution).
4. Therefore, the total lookup cost is $O(1)$ for any fixed N.

### 3.3 Examples

| Syllables | Linearization | Addressable space |
|-----------|--------------|------------------|
| 1 | $\text{index} = c_1$ | $1.12 \times 10^4$ |
| 2 | $\text{index} = c_1 \times 11172 + c_2$ | $1.25 \times 10^8$ |
| 6 | $\text{index} = ((((c_1 \times 11172 + c_2) \times 11172 + c_3) \times 11172 + c_4) \times 11172 + c_5) \times 11172 + c_6$ | $1.94 \times 10^{24}$ |
| 19 | (18 multiplications, 18 additions) | $1.94 \times 10^{77}$ |

## 4. Serialization (Base11172)

A Tagma coordinate may be serialized to a printable, self-validating string
using the Base11172 encoding:

- Each coordinate index $0 \leq \text{idx} < 11,172$ maps to exactly one Hangul
  syllable: $\text{char} = \text{U+AC00} + \text{idx}$.
- A pair of syllables encodes a 16-bit value (2 bytes):
  $\text{value} = \text{hi} \times 11172 + \text{lo}$.
- An N-syllable coordinate serializes to N consecutive Hangul syllables.

The encoding is self-validating: any character outside U+AC00..U+D7AF is
immediately detectable as invalid.

## 5. Compliance

An implementation is Tagma-compatible iff:

1. It implements the composition formula (Section 1) correctly for all
   11,172 valid coordinates.
2. It rejects all 54,364 structurally invalid 16-bit values.
3. It provides $O(1)$ worst-case access for single-syllable coordinates.
4. It provides $O(1)$ worst-case access for any fixed N-syllable composition
   via linearization (Section 3).
5. All implementations must be bit-exact: the same coordinate must produce
   the same decomposition and the same linear index across all languages and
   platforms.

---

*Version 1.0 — Part of the Tagma specification family.*
