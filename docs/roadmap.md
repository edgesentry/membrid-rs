# Roadmap

## Architecture Basis

membrane uses a **Vector-First storage hierarchy** (see `storage-architecture.md`):

- **Phase 1** — Tier 1 only: LanceDB core (vector store, TTL, embedding cache)
- **Phase 2** — Tier 2: LanceGraph (entity graph, relationship traversal)
- **Phase 3** — Tier 3: DuckDB (SQL federation, Iceberg, cross-session analytics)

DuckDB is an **optional analytics layer** introduced only in Phase 3. It does not participate in the `store_episode()` write path in any phase.

---

## Current State

Design complete. No production code exists yet. Deliverables so far:

- [x] Ecosystem survey — confirmed no equivalent Rust library exists
- [x] Architecture decision: Arrow as IO common language
- [x] Vector-First hierarchy adopted: LanceDB primary → LanceGraph additive → DuckDB analytics
- [x] Storage schema: `facts.lance` (with `ttl_expires_ms`), `embedding_cache.lance`, `memory_graph/`
- [x] Core trait design: `FactStore`, `RelationshipStore`, `LifecycleStore`, `EmbeddingEngine`
- [x] Crate layout defined (`src/arrow/` as schema source of truth)
- [x] Automation guidelines (A/B/C/D levels)
- [x] Docs split into focused files + `AGENTS.md`

---

## Phase 1 — Fact Memory MVP (Tier 1: LanceDB)

**Goal:** store and retrieve episodes with TTL; assemble context within a token budget. No DuckDB or LanceGraph required.

### Milestones

| Milestone | Deliverable | Done |
|-----------|-------------|------|
| 1.1 | `src/error.rs` — `MembraneError` with `thiserror` | [ ] |
| 1.2 | `src/types.rs` — `MemoryId`, `Episode`, `Role`, `RetrievedMemory`, `AssembledContext` | [ ] |
| 1.3 | `src/arrow/mod.rs` — Arrow schema for `facts.lance` (including `ttl_expires_ms` column); `src/arrow/convert.rs` — `Episode → RecordBatch` | [ ] |
| 1.3a | `src/arrow/mod.rs` — Arrow schema for `embedding_cache.lance` (key, model_id, vector) | [ ] |
| 1.4 | `src/memory/working.rs` — `WorkingMemory` ring buffer (no deps) | [ ] |
| 1.5 | `src/storage/mod.rs` — `FactStore`, `EmbeddingEngine` traits (RPITIT) | [ ] |
| 1.6 | `src/storage/lance.rs` — `LanceFactStore` (feature = `store-lance`) | [ ] |
| 1.7 | `src/embedding/mod.rs` — `NoopEmbeddingEngine`; `src/embedding/cache.rs` — in-memory HashMap cache (Phase 1), `embedding_cache.lance` (Phase 1 opt-in) | [ ] |
| 1.8 | `src/embedding/mistral.rs` — `MistralEmbeddingEngine` (feature = `embedding-local`) | [ ] |
| 1.9 | `src/ops/store.rs` — `store_episode()`: embed → RecordBatch → LanceDB (single write, idempotent) | [ ] |
| 1.10 | `src/ops/retrieve.rs` — `retrieve()`: ANN search on `facts.lance` with `ttl_expires_ms` filter | [ ] |
| 1.11 | `src/ops/forget.rs` — `forget()`: Lance metadata filter sweep + versioned batch delete | [ ] |
| 1.12 | `src/context/token_budget.rs` — character heuristic; `src/context/assembler.rs` — `assemble_context()` | [ ] |
| 1.13 | `src/lib.rs` — `MembraneEngine::open()`, `store_episode()`, `assemble_context()`, `forget()` public API | [ ] |
| 1.14 | `examples/basic_store_retrieve.rs`, `examples/context_assembly.rs` | [ ] |
| 1.15 | Unit tests: `InMemoryFactStore` (no LanceDB); integration tests: `tempfile::TempDir` + LanceDB | [ ] |

### Key design constraints (Phase 1)

- `store_episode()` is a **single write to one Lance dataset** — no secondary store, no two-phase commit
- TTL is enforced via `ttl_expires_ms` column predicate; `forget()` is a Lance versioned delete
- All write operations must be **idempotent** (upsert semantics) — re-execution is always safe (M2 from #22)

### Acceptance criteria

```bash
cargo test                           # all unit tests pass, no external deps
cargo test --features store-lance    # LanceDB integration tests pass
cargo run --example basic_store_retrieve --features "store-lance"
# assemble_context() never exceeds token_budget
# retrieve() excludes episodes where ttl_expires_ms < now
# forget() removes target episodes; re-run is a no-op
```

---

## Phase 2 — Entity Graph (Tier 2: LanceGraph)

**Goal:** add entity extraction, entity graph storage, and graph-expansion during retrieval. DuckDB still not required.

### Milestones

| Milestone | Deliverable | Done |
|-----------|-------------|------|
| 2.1 | `src/storage/graph.rs` — `LanceRelationshipStore`: `entities.lance` + edge datasets | [ ] |
| 2.2 | `src/arrow/` — extend schemas for entity and edge tables | [ ] |
| 2.3 | `src/ops/store.rs` — extend `store_episode()` to extract entities and write to LanceGraph (entity-first ordering) | [ ] |
| 2.4 | `src/ops/retrieve.rs` — extend `retrieve()` with LanceGraph neighbor expansion | [ ] |
| 2.5 | `src/entity/rule_based.rs` — regex NER; `src/entity/resolution.rs` — entity dedup with tombstone soft-delete | [ ] |
| 2.6 | `src/context/token_budget.rs` — Gemma 4 `tokenizer.json` via `tokenizers` crate | [ ] |

### Key design constraints (Phase 2)

- **Entity-first write ordering** (M4 from #22): always write `entities.lance` before any edge dataset. A crash after entity write but before edge write leaves an orphan node — benign; the entity is still findable via ANN. A dangling edge (edge without entity) cannot occur under this ordering.
- **Tombstone soft-delete** for entity merges: mark entity as tombstoned → redirect edges → hard-delete. No dangling edge window.
- LanceGraph and LanceDB share no write path — they are independent Lance datasets. Cross-store consistency is not required.

### Acceptance criteria

```bash
cargo test --features "store-lance"
# Entity graph: neighbors() traversal returns related entities
# Entity-first: no dangling edges in integration tests
# Token counting: AssembledContext.tokens_used matches Gemma 4 tokenizer
# retrieve() graph-expansion returns additional context beyond ANN alone
```

---

## Phase 3 — Analytics and Federation (Tier 3: DuckDB)

**Goal:** introduce DuckDB as an optional analytics layer for cross-session aggregation, Iceberg federation, and large-scale consolidation. Background automation and Python bindings.

### Milestones

| Milestone | Deliverable | Done |
|-----------|-------------|------|
| 3.1 | `src/storage/duck.rs` — `DuckLifecycleStore`: DDL for `consolidation_jobs`, `session_index`; reads Lance files via `read_parquet()` | [ ] |
| 3.2 | `src/ops/consolidate.rs` — `consolidate()` tokio background task; DuckDB drives job scheduling via `consolidation_jobs` state machine (`PENDING → WRITING_SUMMARY → SUMMARY_WRITTEN → DELETING_ORIGINALS → COMPLETE`) | [ ] |
| 3.3 | `src/ops/summarize.rs` — `summarize()`: compress old episodes into extracted facts via mistral.rs | [ ] |
| 3.4 | DuckDB federation: `read_parquet('./data/memory/facts.lance/**/*.parquet')` integration test | [ ] |
| 3.5 | `src/audit/ledger.rs` — `AuditBridge<L: AuditLedger>`: emit blake3-signed `AuditRecord` on every `store_episode()` | [ ] |
| 3.6 | PyO3 bindings: `MembraneSession` Python class (`store_episode`, `assemble_context`, `forget`) | [ ] |
| 3.7 | arktrace integration test: chat route stores episodes via membrane; `assemble_context()` feeds Gemma 4 | [ ] |

### DuckDB scope in Phase 3

DuckDB is a **query layer over Lance data**, not a lifecycle manager:
- It reads `facts.lance` and `entities.lance` as Parquet for aggregation — it does not write to them
- The only DuckDB-owned tables are `consolidation_jobs` and `session_index`
- `store_episode()` remains a single Lance write — DuckDB is never in the write path

Write-intent WAL (M1 from #22) applies only to consolidation jobs, not to normal episode writes.

### Acceptance criteria

```bash
cargo test --features "store-lance store-duck embedding-local audit-bridge"
cargo run --example context_assembly --features "store-lance store-duck embedding-local"
# consolidate(): duplicate episodes merged; summary fact written to facts.lance
# AuditBridge: every store_episode() emits a signed AuditRecord
# Python: import membrane; session.store_episode(...); session.assemble_context(...)
# DuckDB federation: SQL aggregation over multi-session facts.lance works
```

---

## Future Considerations (post-Phase 3)

| Item | Notes |
|------|-------|
| Iceberg catalog support | DuckDB already supports `iceberg_scan()` — enable cross-region lakehouse queries |
| Contradiction detection | `ConsistencyChecker` trait hook (Level D automation) — domain-defined rules |
| Streaming token budget | Real-time budget enforcement during generation (requires LLM stream API) |
| edgesentry-inspect grounding | `ContextAssembler` prepends past vessel findings to `SensorFrame` processing |
| Multi-session consolidation | Aggregate episodes across patrol vessel sessions into shared object storage |
