# nex — FIH Blackboard Storage Engine

nex (노드 연결망, node expansion) implements the Fact-Inference-Hint (FIH)
blackboard coordination protocol. It provides persistent, indexed storage for
distributed agent workflows over an abstract flat key-space I/O layer.

## Execution Model

nex is **not a general-purpose library**. It is an **execution unit** — each
`FihStorage` instance runs on a single thread with exclusive ownership of its
in-memory state and I/O channel. There is no internal concurrency.

### Key properties

- **Single-threaded, single-owner**: Each instance owns its `FihCoord` indices,
  entity stores, and pending write buffer. No thread synchronization primitives
  (Mutex, RwLock) exist anywhere in the hot path.
- **Async-only trait contract**: Every I/O-bound operation is async. The trait
  `AsyncFileIo` and the trait `SemanticStore` are both `#[async_trait(?Send)]`.
  Sync wrappers (`block_on`) must not be used inside a nex instance — they
  would block the sole thread and stall all pending I/O.
- **`RefCell` for interior mutability**: nex uses `RefCell` (not `Mutex`)
  because the single-threaded model guarantees no concurrent access. Holding a
  `RefCell` borrow across an `.await` point is sound as long as the
  `SemanticStore` implementations do not re-enter `FihCoord` — a property
  enforced by convention, not the compiler.
- **No shared state between instances**: Two nex instances never share a
  `FihCoord` or `EntityStore`. Coordination between instances happens
  exclusively through the FIH protocol (facts, intents, hints submitted via
  the external I/O layer).

## Scaling Model: Physical Replication

nex does not scale by adding threads or internal sharding. It scales by
**physical instance replication**:

```
CF Workers Durable Objects:
  NexusCfDO (name="shard-a") ─── FihStorage ─── R2 bucket A
  NexusCfDO (name="shard-b") ─── FihStorage ─── R2 bucket B
  NexusCfDO (name="shard-c") ─── FihStorage ─── R2 bucket C

  Cross-instance communication: only via FIH protocol over R2

Native processes:
  Process 1 (port 3000) ─── FihStorage ─── FsIo("./data/shard-a")
  Process 2 (port 3001) ─── FihStorage ─── FsIo("./data/shard-b")
```

Each instance is an island with exclusive state. Instances communicate by
reading and writing facts, intents, and hints through the shared I/O layer
(R2 bucket, filesystem directory). No direct RPC, no shared memory, no
distributed locks.

## Why async-first design

In the Cloudflare Workers runtime (and WASM environments generally), blocking
primitives are unavailable:

| Primitive | Behavior in CF Workers / WASM |
|-----------|-------------------------------|
| `std::thread::spawn` | Panics or no-op |
| `std::sync::Mutex::lock` | Hangs (park/unpark not implemented) |
| `futures_executor::block_on` | Hangs (blocks the only thread, starving all other futures) |
| `async fn` + `.await` | Works correctly (cooperative multitasking) |

Because CF Workers is the primary deployment target, **async is the design
center**. The entire I/O stack, from `AsyncFileIo` to `SemanticStore` to
`FihStorage`'s public API, is async-only.

## Architecture overview

```
┌─────────────────────────────────────────────────┐
│                  FihBlackboard                    │
│  (sync Blackboard trait impl, delegates to       │
│   FihStorage via block_on — native only)         │
├─────────────────────────────────────────────────┤
│                  FihStorage                       │
│  ┌─────────────┐  ┌──────────────┐               │
│  │ EntityStore  │  │  FihCoord    │               │
│  │ (fact,       │  │  ┌─────────┐ │               │
│  │  intent,     │  │  │  by_fact │ │               │
│  │  hint)       │  │  │ by_origin│ │               │
│  └─────────────┘  │  │by_creator│ │               │
│                   │  │ by_status │ │               │
│  ┌─────────────┐  │  │by_semantic│ │  SemanticStore│
│  │ pending buf  │  │  │ (plug-in) │─▷ BM25, Vectorize│
│  └─────────────┘  │  └─────────┘ │               │
│                   └──────────────┘               │
│                         │                        │
│                         ▼                        │
│              AsyncFileIo (trait)                 │
│         ┌──────────┼──────────┐                  │
│         ▼          ▼          ▼                  │
│     FsIo      CfFihIo    SimIo (tests)           │
└─────────────────────────────────────────────────┘
```

## Trait design: flashlight pattern

`SemanticStore` implementations do not hold references to `FihStorage`. They
receive a `RecordLoad` handle at `insert()` time and call only the accessors
they need:

```rust
pub trait RecordLoad {
    fn content(&self, id: u32) -> Option<Vec<u8>>;
    fn text(&self, id: u32) -> Option<String>;
    fn features(&self, id: u32) -> Option<Vec<f32>>;
}
```

A vector store calls `features()`. A BM25 store calls `text()`. An ngram
origin store calls `text()` on a concatenated origin field. The core
(`FihCoord`) never knows which methodology is in use. This is the
**flashlight pattern**: the core shines a light on the data and the plugin
reads only what it needs.

## I/O layer

`AsyncFileIo` abstracts all I/O behind a flat key-space:

```rust
#[async_trait(?Send)]
pub trait AsyncFileIo {
    async fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String>;
    async fn write(&self, path: &str, data: &[u8]) -> Result<(), String>;
    async fn list(&self, prefix: &str) -> Result<Vec<String>, String>;
    async fn delete(&self, path: &str) -> Result<(), String>;
    async fn apply_batch(&self, ops: &[WriteOp]) -> Result<(), String>;
}
```

Key-space layout:

| Path pattern | Content |
|---|---|
| `facts/f_{hash}.fact` | Fact record (postcard) |
| `intents/i_{hash}.intent` | Intent record (postcard) |
| `hints/h_{hash}.hint` | Hint record (postcard) |
| `blob/{hash}.bin` | Raw content bytes |
| `blob/{hash}.bin.meta` | Content metadata (mime_type, size) |
| `flush/{part}/cursor_{t}.chain` | Delta chain entries |

## Write path

All writes go through a pending buffer and are committed in a single
`apply_batch` call:

1. `submit_fact` enqueues blob write + fact record write in `self.pending`
2. `submit_intent` enqueues intent record write in `self.pending`
3. Caller calls `flush_pending()` which calls `apply_batch(&ops)`

This reduces N R2 PUT calls to 1 for bulk operations (e.g., document
ingestion with 100 paragraphs). The trade-off: unflushed data is lost on
crash. The caller controls durability by choosing when to flush.

## Cold start recovery

When a `FihStorage` instance starts for the first time (or after a restart),
its in-memory caches are empty. The `rebuild_cache` method reads all records
from the I/O layer and populates `FihCoord` indices and entity stores. The
`rebuild_semantic` method then re-populates all registered `SemanticStore`
implementations from the cached fact content.

In the CF Workers Durable Object deployment, cold start is detected by an
empty `fact_store`. The DO's `fetch` handler calls `rebuild_cache` and
`rebuild_semantic` on the first request that encounters an empty store.

## Local simulation

The `gateway/nex-cf/mock` crate provides a local HTTP server that simulates
the full nex-cf pipeline without Cloudflare bindings. It uses an in-memory
HashMap as the R2 mock and `InMemoryBm25` as the semantic store.
Usage:

```
cargo run -p nex-cf-mock
curl http://localhost:8080/ingest?text=semantic+search&origin=test
curl http://localhost:8080/search?q=semantic
```

## Concurrency invariants

These rules hold across all nex code:

1. No `Mutex`, `RwLock`, or any thread-blocking primitive in hot paths.
2. No `block_on` inside `FihStorage` methods.
3. `RefCell` borrows are never held across `.await` points unles explicitly
   allowed and justified by the single-threaded execution model.
4. `SemanticStore` implementations must not re-enter `FihCoord` during
   `insert()` / `search()` / `remove()`.
5. `Arc` is used only for immutable configuration data shared across the
   outer wrapper (e.g., `Env` in `NexusCfDO`), never for mutable state.
6. Two instances never share a `FihCoord` or `EntityStore`.

## Extending nex

### Adding a new I/O backend

Implement `AsyncFileIo` for any flat key-space:

```rust
struct MyIo { /* ... */ }

#[async_trait(?Send)]
impl AsyncFileIo for MyIo {
    async fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> { /* ... */ }
    async fn write(&self, path: &str, data: &[u8]) -> Result<(), String> { /* ... */ }
    async fn list(&self, prefix: &str) -> Result<Vec<String>, String> { /* ... */ }
    async fn delete(&self, path: &str) -> Result<(), String> { /* ... */ }
    // apply_batch has a default impl: sequential write/delete
}
```

### Adding a new semantic store

Implement `SemanticStore`:

```rust
struct MyStore { /* ... */ }

#[async_trait(?Send)]
impl SemanticStore for MyStore {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let text = load.text(id).ok_or("no text")?;
        // index text
        Ok(())
    }
    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        // search and return (id, score) pairs
    }
    async fn remove(&mut self, id: u32) -> Result<(), String> { /* ... */ }
    fn len(&self) -> usize { /* ... */ }
}
```

Register it:

```rust
storage.register_semantic_store(Box::new(MyStore::new()));
```

### Wrapping for sync use (native only)

On native platforms where `block_on` is safe (dedicated thread, not WASM),
use `FihBlackboard`:

```rust
let bb = FihBlackboard::new(fs_io, "my-project");
FactCapable::submit_fact(&bb, &fact)?; // sync, uses block_on internally
```

`FihBlackboard` is not available on `wasm32` targets.
