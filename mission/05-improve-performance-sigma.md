# ADR 05: Sigma Renderer Performance Invariants

- Status: Accepted
- Date: 2026-04-11
- Owners: WebUI / Graph Runtime

## Context

The Sigma-based knowledge-graph viewer had crossed a performance boundary where
ordinary user actions triggered work that scaled with the full graph instead of
the changed state.

The main faults were structural:

1. Layout changes could tear down and recreate the Sigma instance.
2. Selection highlighting depended on a perpetual pulse loop that refreshed the
   canvas continuously.
3. Hover and selection transitions mutated broad portions of graph state instead
   of using render-time reducers.
4. Layout behavior was duplicated across components, so tuning drifted.
5. Edge identity rules were not centralized, which risked inconsistent updates
   during streaming and re-layout operations.

From first principles, the renderer must treat these concerns differently:

- Instance lifecycle is expensive and must be stable.
- Render state should be derived at paint time where possible, not baked into
  graph mutations.
- Refreshes should be scheduled only when the visual output changes.
- Shared graph rules must live behind one abstraction boundary to satisfy DRY
  and preserve SOLID responsibilities.

## Decision

We will keep the Sigma instance stable across layout changes and move graph
performance policy into shared, testable primitives.

### Adopted rules

1. Layout changes must reuse the active Sigma instance and animate positions
   instead of rebuilding the renderer.
2. Hover and selection emphasis must be expressed through Sigma reducers and
   scheduled refreshes, not broad graph mutation sweeps.
3. Selection must not depend on any perpetual animation or polling loop.
4. Layout algorithm thresholds and profiles must be defined once and consumed by
   all layout entry points.
5. Edge identifiers must be deterministic and shared across store, renderer, and
   streaming code paths.

## Implementation

### Shared primitives

- [`edgequake_webui/src/lib/graph/ids.ts`](/Users/raphaelmansuy/Github/03-working/edgequake/edgequake_webui/src/lib/graph/ids.ts)
  centralizes deterministic edge-key generation.
- [`edgequake_webui/src/lib/graph/layouts.ts`](/Users/raphaelmansuy/Github/03-working/edgequake/edgequake_webui/src/lib/graph/layouts.ts)
  centralizes layout selection, adaptive thresholds, and algorithm profiles.

### Renderer behavior

- [`edgequake_webui/src/components/graph/graph-renderer.tsx`](/Users/raphaelmansuy/Github/03-working/edgequake/edgequake_webui/src/components/graph/graph-renderer.tsx)
  now preserves the Sigma lifecycle during layout changes.
- Selection highlighting is render-derived; the old pulse loop was removed.
- Hover and edge emphasis now use reducers plus scheduled refreshes.

### DRY layout control

- [`edgequake_webui/src/components/graph/layout-control.tsx`](/Users/raphaelmansuy/Github/03-working/edgequake/edgequake_webui/src/components/graph/layout-control.tsx)
  and [`edgequake_webui/src/components/graph/layout-controller.tsx`](/Users/raphaelmansuy/Github/03-working/edgequake/edgequake_webui/src/components/graph/layout-controller.tsx)
  now consume the shared layout policy instead of re-implementing it.

### Related release hardening

- [`edgequake_webui/src/components/query/markdown/MermaidBlock.tsx`](/Users/raphaelmansuy/Github/03-working/edgequake/edgequake_webui/src/components/query/markdown/MermaidBlock.tsx)
  was corrected so Mermaid labels containing literal angle brackets are not
  stripped by the sanitizer, restoring the frontend test suite.

## Alternatives Rejected

### Rebuild Sigma on every layout change

Rejected because it treats a coordinate update as a renderer lifecycle event.
That is the wrong abstraction boundary and guarantees avoidable churn.

### Keep the pulse loop and lower its frequency

Rejected because the defect is not the chosen frequency. The defect is making
selection depend on perpetual whole-canvas work.

### Tune each layout caller independently

Rejected because it duplicates policy, violates DRY, and makes future
performance regressions likely.

## Consequences

### Positive

- Layout changes are materially cheaper.
- Selection and hover work scale with rendered state, not continuous mutation.
- Graph identity rules are consistent across streaming and rendering.
- Performance behavior is now unit-testable.

### Tradeoffs

- More logic now lives in shared graph utilities, so incorrect changes there
  affect multiple entry points.
- Reducer-based highlighting requires the renderer contract to stay disciplined;
  direct graph mutation should remain the exception.

## Verification

- Unit coverage added for shared graph helpers:
  - [`edgequake_webui/src/lib/graph/ids.test.ts`](/Users/raphaelmansuy/Github/03-working/edgequake/edgequake_webui/src/lib/graph/ids.test.ts)
  - [`edgequake_webui/src/lib/graph/layouts.test.ts`](/Users/raphaelmansuy/Github/03-working/edgequake/edgequake_webui/src/lib/graph/layouts.test.ts)
- Frontend publication checks:
  - `cd edgequake_webui && pnpm lint`
  - `cd edgequake_webui && pnpm typecheck`
  - `cd edgequake_webui && pnpm test`
- Rust publication checks:
  - `cargo fmt --check --manifest-path edgequake/Cargo.toml`
  - `cargo clippy --workspace --all-targets --manifest-path edgequake/Cargo.toml`
  - `cargo test --workspace --lib --manifest-path edgequake/Cargo.toml`

## Publication Note

The default frontend lint gate now targets the publishable surface area:
shipped application code and committed unit-test support files. Exploratory
Playwright audit specs under `edgequake_webui/e2e/` remain available, but they
do not block a release build.
