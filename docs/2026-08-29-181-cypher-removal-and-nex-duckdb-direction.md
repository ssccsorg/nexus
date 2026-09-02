# 181: cypher and petgraph removal, the thin film, and the nex-duckdb direction

## Context

This devlog records two related outcomes of the 2026-08-27 to 2026-08-29 work session on the `179-multidim-search-bench` branch. First, the removal of the last query-language paradigms (cypher, and with it the remaining petgraph traces) from the nexus workspace. Second, the strategic direction that motivated and followed that removal: the apps ecosystem policy in which a thin `nex` film attaches to a concrete implementation, so query and storage backends such as DuckDB are built as separate `nex-*` instances rather than as nexus infrastructure. The unifying insight is the stigmergy net: the source of an agent's read is a nex, the destination of its write is another nex, and no central orchestrator exists.

## The decision context

The `2026-08-20-thin-nexus-principle.md` devlog established the storage-down, semantics-up split: nexus keeps the FIH lifecycle and the Tagma coordinate semantics, while low-level storage and IO move to chton and application-owned record maps. The 2026-08-27 session (issue 179) measured the structural filter index and confirmed flat indexing cost: the index stays constant in record count, linear in distinct axis values. The 2026-08-29 session completed the corollary of the thin principle on the query-language side.

Tagma is a coordinate space, not a query language. SQL and Cypher describe what to find and rely on an internal planner to traverse an index; Tagma addresses where data lives directly by coordinate. Forcing the Tagma coordinate space into the SQL or Cypher paradigm is a structural mismatch, and the reference implementations of that mismatch were removed: the DuckDB cold storage backend in commit `28c48fae` (2026-08-03) and the cypher interface in this session. The `nexus-*` infrastructure layer now holds no query language. It holds the contract only.

## What was removed

### The cypher interface

`interface/cypher` was a workspace member and part of the core CI gate in `scripts/run-core.sh`. It used `petgraph::Graph` as its hot execution engine (`translate.rs`, 786 lines) and held the cold-routing surface (`ColdQuery`, `CypherCapable`) for the DuckDB backend that no longer exists. After the DuckDB removal, `CypherCapable` was only an alias of `interface_query::QueryCapable`, and no backend in the workspace implemented `query_plan`. The interface had no live consumer beyond its own tests.

The interface was removed from the workspace members, the CI gate, and the source tree. Its source stays in git history as the extraction basis for the future `nex-duckdb` instance.

### The remaining petgraph traces

petgraph was removed from the storage engine in issue 157, but traces remained on the branch:

- `nex/fih/Cargo.toml`: dependency present, zero source usages.
- `fih/Cargo.toml` (`fih-model`): dependency present, used by `fih/src/storage/graph.rs` (`GraphRead`/`GraphWrite`/`NodeWeight`/`EdgeWeight` over `petgraph::Graph`).
- `interface/cypher/Cargo.toml`: dependency present (removed with the interface).
- `rem/Cargo.toml` (workspace deps): dependency present, removed from the rem workspace manifest in the same pass.
- `Cargo.lock` (nexus and rem): entries removed.

`fih/src/storage/graph.rs` was deleted and its exports were dropped from `fih/src/storage/mod.rs`. `playbooks/agents/src/main.rs` lost its GraphRead/Cypher comment references, and `fih/src/storage/evict.rs` lost the stale `PetgraphStorage` comment. The verified end state: zero petgraph references across the nexus workspace, 301 workspace tests passing, clippy clean.

### What stays

`interface/query` remains as the contract surface. It defines `ColdQuery` (tabular filter, projection, aggregate, sort) and the `QueryCapable` trait with a default error-returning method:

```interface/query/src/lib.rs#L118-131
pub trait QueryCapable: nex_fih::StorageRead {
    fn query_plan(&self, _plan: &ColdQuery) -> Result<Vec<HashMap<String, Content>>, String> {
        Err("QueryCapable: not yet implemented for this backend".into())
    }
}
```

The doc comment records that the ColdQuery-to-SQL emitter (DuckDB dialect over parquet-backed FIH views) is recoverable from git history and that ColdQuery is the intended endpoint of the cold-routing pipeline.

## The thin film and the apps ecosystem policy

The apps ecosystem document (docs.ssccs.org/projects/nexus/apps) defines the composition model as "Your Codebase + nex core library integration = nex-{implementation}". A nex is not a standalone server; it is a thin kernel grafted onto a domain engine, providing blackboard memory, the FIH protocol, stigmergic coordination, and signing, decoupled from storage. `nex-spinwasi-ssccsdocs` (Spin WASI) and `nex-ev` (ExaVerif) are the reference instances.

The query-language removal is the same policy applied to query and storage backends. DuckDB is a domain engine like any other. The correct shape is not a DuckDB module inside nexus, but a `nex-duckdb` instance: a separate repository that implements `QueryCapable` over DuckDB, attaches to the nexus network through the nex protocol, and becomes queryable end to end. The same pattern applies to any future tabular or spatial backend.

## The stigmergy net

The resulting data flow closes a loop in which every endpoint is a nex:

1. Source: data lives in a source nex under Tagma coordinates, not as files.
2. Read: an agent (for example actus) requests a coordinate range from the source nex. The nex resolves the coordinates without a scan.
3. Judgment: the agent synthesizes the read data and decides, at coordinate granularity, not file granularity.
4. Write: the agent sends the result to a destination nex (or several). The stored data carries Tagma coordinates.
5. Cycle: the destination nex becomes a source for another agent.

The source is a nex, the storage destination is a set of nex instances, and the same FIH primitives and wire protocol bind both directions. This is the stigmergy net: agents leave traces on blackboards and react to the traces of others without orchestration. The nexus docs describe the same model as "storage as shared memory, nex as the execution loop that reads from it and writes back to it", where every backend including another nex is indistinguishable from the core's perspective.

The telos layer, by contrast, is the physical file-IO substrate below this net. It handles the file granularity; actus and the other agents operate above it, in the coordinate space, without knowing files exist.

## Judgment

Analytic conclusion: the removal of cypher and petgraph completes the thin nexus principle on the query-language boundary. Nexus infrastructure now holds contracts (`QueryCapable`, `ColdQuery`) and the FIH/Tagma semantics, and no query or storage implementation. The `nex-duckdb` direction is consistent with the documented apps ecosystem policy and with the code that remains.

Value judgment, hedged: keeping `interface/query` in the nexus workspace is reasonable because it is a small, stable contract that multiple future backends will implement, and the reference emitter lives in git history for recovery. The alternative of removing it entirely would force a contract resurrection later. The judgment is scoped to the current stage; if no backend materializes, the contract could be moved to the future `nex-duckdb` repository.

## Open follow-ups

- Create the `nex-duckdb` repository implementing `QueryCapable` over DuckDB (parquet-backed FIH views), attaching to the nexus network via the nex protocol.
- Decide whether `interface/query` stays in the nexus workspace or moves into `nex-duckdb` once the backend exists.
- Record the issue 179 decision comment and open the combined PR for issues 179 and 181 from the `179-multidim-search-bench` branch.

## References

- Devlog 2026-08-20: thin nexus principle (storage down, semantics up)
- Devlog 2026-08-27: issue 179 multi-dimensional structural search benchmark
- Nexus issue 181: OS-less storage path, remove petgraph traces and std-only dependencies
- Nexus issue 179: real-scenario multi-dimensional search over the structural filter index
- Nexus apps ecosystem document: docs.ssccs.org/projects/nexus/apps/index.llms.md
- Rem devlog 2026-08-27: OS-less storage analysis (companion record in the rem repository)
- Commit `28c48fae`: DuckDB cold storage backend removal
- Commit `ecbc095a`: cypher interface and petgraph trace removal
