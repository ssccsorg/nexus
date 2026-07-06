# nex-calc

FIH-based calculator where computation is state space traversal.

## Concept

A conventional calculator evaluates `3 + 5` as a single CPU instruction.
nex-calc does it differently:

```
put 3        → Fact(3) created at coordinate in FIH space
put 5        → Fact(5) created at coordinate in FIH space
add 3 5      → Intent(Add, from 3 to 5) — a directional vector
resolve      → Traverse FIH space: Fact(3) → Intent(Add) → Fact(8)
             → Fact(8) persists at a new coordinate forever
```

The traversal IS the computation. Every Fact is immutable. Every
operation leaves a permanent trace in the state space.

## Architecture

| Primitive | Role | nex-calc mapping |
|-----------|------|-----------------|
| F (Fact)  | Immutable data at a coordinate | Number stored as content-addressed Fact |
| I (Intent)| Directional function | Arithmetic operator (add, sub, mul, div) |
| H (Hint)  | Constraint or transform | Dynamic boundary on computation |

The algebraic structure: **F x I x H → F'**

## Usage

```bash
cargo run
```

```
> put 3
Fact 8940..d8aa = 3
> put 5
Fact ef2d..6e1f = 5
> add 8940 ef2d
Intent a1b2..c3d4 (+ 8940.. ef2d..)
> resolve a1b2
3 + 5 = 8  → Fact e4e6..6fe7 = 8
> get e4e6
Fact e4e6..6fe7 = 8
> constrain gt 10
Hint d5e6..a1f7 (result > 10)
> add 8940 ef2d
Intent f6g7..b2c3 (+ 8940.. ef2d..)
> resolve f6g7
error: constraint violated [d5e6..]: result > 10 (got 8)
> constrain double
Hint ab12..cd34 (double operands)
> add 8940 ef2d
> resolve ...
(3*2) + (5*2) = 16  → Fact ...
> stats
facts:     4
pending:   0
hints:     1
```

## Why Inefficient

nex-calc is intentionally inefficient. A normal calculator computes
`3 + 5 = 8` instantly. nex-calc creates Facts, Intents, and traverses
a coordinate space. Every intermediate state persists.

This inefficiency is the philosophy:

- Computation is not a result; it is a traversal.
- Data is not ephemeral; every Fact is immutable history.
- Constraints are not hardcoded; they are dynamic Hints.
- The system records its own reasoning.

## Build & Test

```bash
cd apps/nex-calc
cargo build
cargo test
cargo run
```
