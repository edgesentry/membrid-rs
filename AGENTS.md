# AGENTS.md — membrane

AI agent entry point. For humans, see [README.md](README.md).

## What this crate is

`membrane` is an embedded Rust library for LLM memory management targeting edge AI (Gemma 4 2B/4B). It compensates for small model limitations from outside the model: selecting context, resolving entities, enforcing token budgets, and keeping memory consistent. It is general-purpose — no fixed domain.

## Key design decisions (read before touching anything)

| Decision | Rule |
|----------|------|
| **Arrow is the IO boundary** | All API boundaries pass `arrow_array::RecordBatch`. Never add a function that crosses backends with serialized bytes or custom structs. |
| **DuckDB holds no primary facts** | DuckDB is the lifecycle brain (TTL, cache, consolidation state). It reads Lance files via `read_parquet()`. Do not store fact content in DuckDB tables. |
| **LanceDB is fact memory** | `facts.lance` — single table for episodes + extracted SPO facts (`fact_kind` field distinguishes). ANN + Arrow predicate filter in one query. |
| **LanceGraph is relationship memory** | Entity graph as Lance datasets. Traversal via Arrow joins — no graph engine process. |
| **WorkingMemory is in-process only** | Ring buffer. No storage dependency. Lives in the context window. |
| **Edge ↔ cloud sync is out of scope** | membrane operates locally. Sync is an application-level concern. |
| **RPITIT for async traits** | `impl Future<Output = ...> + Send` in trait methods (stable Rust 1.75+). Do not use `async_trait` macro. |

## Module map

```
src/
  lib.rs           — public API, feature gating
  error.rs         — MembraneError
  types.rs         — MemoryId, Episode, Entity, Fact, RetrievedMemory, AssembledContext
  memory/
    working.rs     — WorkingMemory ring buffer
  storage/
    mod.rs         — FactStore, RelationshipStore, LifecycleStore, EmbeddingEngine traits
    lance.rs       — FactStore impl (LanceDB)
    graph.rs       — RelationshipStore impl (LanceGraph)
    duck.rs        — LifecycleStore impl (DuckDB)
  arrow/
    mod.rs         — Arrow schemas for facts.lance, entities.lance  ← single source of truth
    convert.rs     — Episode/Fact/Entity ↔ RecordBatch
  embedding/
    mod.rs         — EmbeddingEngine trait, NoopEmbeddingEngine
    mistral.rs     — mistral.rs impl (feature = embedding-local)
    cache.rs       — EmbeddingCache
  ops/
    store.rs       — store_episode()
    retrieve.rs    — retrieve()
    consolidate.rs — consolidate() Phase 3
    forget.rs      — forget()
    summarize.rs   — summarize() Phase 3
  context/
    assembler.rs   — assemble_context()
    token_budget.rs— TokenCounter
  entity/
    rule_based.rs  — NER Phase 2
    resolution.rs  — entity dedup Phase 2
  audit/
    ledger.rs      — AuditBridge (feature = audit-bridge)
```

## Core traits (storage/mod.rs)

- `FactStore` — `insert(RecordBatch)`, `search(&[f32], limit, filter)`, `delete(&[MemoryId])`
- `RelationshipStore` — `upsert_entities(RecordBatch)`, `upsert_edges(relation, RecordBatch)`, `neighbors(node_id, depth)`
- `LifecycleStore` — `query(sql, params) -> RecordBatch`, `execute(sql)`
- `EmbeddingEngine` — `embed(text) -> Vec<f32>`, `embed_batch(texts) -> FixedSizeListArray`, `dimensions()`

Test stubs: `InMemoryFactStore`, `NoopEmbeddingEngine` — no external deps required.

See [docs/integration.md](docs/integration.md) for feature flags and Cargo.toml.
See [docs/roadmap.md](docs/roadmap.md) for implementation status, build commands, and milestone checklists.

## Docs index

| File | Contents |
|------|----------|
| [docs/problem-statement.md](docs/problem-statement.md) | Why this library, ecosystem survey, reference implementations, tech stack |
| [docs/storage-architecture.md](docs/storage-architecture.md) | Arrow IO diagram, backend roles, lakehouse model, sync scope decision |
| [docs/memory-taxonomy.md](docs/memory-taxonomy.md) | Memory tiers, LanceDB/LanceGraph/DuckDB schemas |
| [docs/api-design.md](docs/api-design.md) | Core traits, data structures, operations table |
| [docs/embedding-context.md](docs/embedding-context.md) | Mistral.rs embedding pipeline, context assembly algorithm |
| [docs/integration.md](docs/integration.md) | Feature flags, Cargo.toml, edgesentry-audit / arktrace / edgesentry-inspect integration |
| [docs/implementation.md](docs/implementation.md) | Crate layout, implementation phases, reference files, verification commands |
| [docs/automation-guidelines.md](docs/automation-guidelines.md) | A/B/C/D automation levels, design checklist, recommended configs per use case |
| [docs/roadmap.md](docs/roadmap.md) | Milestone checklist for Phase 1/2/3, acceptance criteria, future considerations |
