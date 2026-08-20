# 123-125: EntityStore async, AsyncFileIo rename, ingest-one cleanup

## Three issues, one PR

### Issue #123 — EntityStore async

EntityStore was the last synchronous trait in FihStorage's async core. Every EntityStore method (get, insert, len, values, clear, replace_from) was sync, hiding behind `Cell2::borrow()` (Mutex or RefCell). Inside an async context, these sync calls forced callers to hold borrows across `.await` points — a borrow-checker headache that was technically safe because MemoryEntityStore operations are instantaneous, but structurally inconsistent.

**Change**: Made all EntityStore methods `async fn` via `#[async_trait]`. Both native (`Send + Sync`) and wasm (`?Send`) conditional trait definitions updated.

**Key decision**: `retain` was removed from the trait entirely. It was the only method taking a `FnMut` closure, which creates lifetime conflicts with async_trait's generated futures (RefMut/MutexGuard borrows cannot be held across await). Instead, callers (`evict_before`, `evict_stale_intents`) now use `values().await + filter + replace_from().await` — a pattern that is cleaner and avoids the borrow issue entirely.

**Sync path elimination**: Removing `retain` exposed a deeper problem: `FihStorage` implemented sync traits `RecordLoad` and `FihRecordLoad` purely to support `self.coord.semantic_insert(fact_idx, self)`. These sync trait impls required sync accessors (`get_sync`, `values_sync`, etc.) on `MemoryEntityStore` — a clear design smell. Removed both the sync trait impls and the sync accessors. Replaced `self` passing with a local `FactTextRecord` struct (same pattern as `rebuild_semantic`'s `TextRecord`).

### Issue #124 — AsyncFileIo rename + BatchIo split

The `Async-` prefix on `AsyncFileIo` was redundant — the trait returns `IoFuture` (which is `Pin<Box<dyn Future<...>>>`), making every method inherently async. No sync variant exists. Renamed to `FileIo`.

More importantly, `apply_batch` was extracted into a separate `BatchIo` lego trait. Rationale:

- `apply_batch` is a deployment concern, not a core IO primitive. Some backends (R2 with concurrent JS promises) benefit from custom batch dispatch; others (simple filesystem) are fine with sequential iteration.
- `FileIo` stays small: `read`, `write`, `list`, `delete`. Any backend can implement it.
- `BatchIo: FileIo` adds `apply_batch`. Implementors: CfFihIo, MockIo, BatchIo wrapper. Non-implementors: SimIo, WasmerIo, FsIo.
- `default_apply_batch()` free function provides the default sequential implementation for any `FileIo`.
- `FihStorage::flush_pending` uses `default_apply_batch` instead of `self.io.apply_batch()`, so `FihStorage` never needs a `BatchIo` bound.
- `BatchIo` wrapper (both nex-cf and wasmer versions) now conditionally implements `BatchIoTrait` only when `I: FileIo + BatchIoTrait`. This means the batch adapter can itself be used in contexts that require batch support, without forcing all inner backends to support it.

### Issue #125 — ingest-one dead code

PR #122 changed nex-cf ingestion from paragraph-level to document-level Facts. The `/ingest-one` handler retained the old paragraph-level branch behind a `flush=0` query parameter check. Removed ~30 lines of dead code. `/ingest-one` always calls `ingest_document`.

### Snapshot bug (caught by test)

During CI validation, `test_snapshot_roundtrip` was failing. Root cause: `write_snapshot` called `s.io.write(...)` which went through `BatchIo`'s write buffer, but `BatchIo::flush()` was never called afterward. The snapshot sat in the buffer while the test tried to read it from a fresh store on the same filesystem.

Fix: `write_snapshot` now calls `s.io.flush()` before and after writing, and writes the snapshot directly to inner IO via `s.io.io().write(...)`, bypassing the batch buffer. This required adding `BatchIo::io() -> &I` accessor and specializing `write_snapshot`/`restore_from_snapshot`/`ingest_document` from generic `<I: FileIo>` to concrete `<BatchIo<WasmerIo>>` — appropriate since this app only uses one IO backend.

### CI

- 23 files changed, ~340 insertions, ~380 deletions
- All 99 nex tests pass, all 7 wasmer tests pass
- Native + wasm32 check, clippy (9 crates, -D warnings) all green
