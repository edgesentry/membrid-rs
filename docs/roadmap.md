# Roadmap

## Current State

Design complete. No production code exists yet. Deliverables so far:

- [x] Ecosystem survey — confirmed no equivalent Rust library exists
- [x] Architecture decision: Arrow as IO common language
- [x] Storage schema: `facts.lance`, `memory_graph/`, DuckDB lifecycle tables
- [x] Core trait design: `FactStore`, `RelationshipStore`, `LifecycleStore`, `EmbeddingEngine`
- [x] Crate layout defined (`src/arrow/` as schema source of truth)
- [x] Automation guidelines (A/B/C/D levels)
- [x] Docs split into focused files + `AGENTS.md`

---

## Phase 1 — Fact Memory MVP

**Goal:** store and retrieve episodes; assemble context within a token budget. No DuckDB or LanceGraph required.

### Milestones

| Milestone | Deliverable | Done |
|-----------|-------------|------|
| 1.1 | `src/error.rs` — `MembraneError` with `thiserror` | [ ] |
| 1.2 | `src/types.rs` — `MemoryId`, `Episode`, `Role`, `RetrievedMemory`, `AssembledContext` | [ ] |
| 1.3 | `src/arrow/mod.rs` — Arrow schema for `facts.lance`; `src/arrow/convert.rs` — `Episode → RecordBatch` | [ ] |
| 1.4 | `src/memory/working.rs` — `WorkingMemory` ring buffer (no deps) | [ ] |
| 1.5 | `src/storage/mod.rs` — `FactStore`, `EmbeddingEngine` traits (RPITIT) | [ ] |
| 1.6 | `src/storage/lance.rs` — `LanceFactStore` (feature = `store-lance`) | [ ] |
| 1.7 | `src/embedding/mod.rs` — `NoopEmbeddingEngine`; `src/embedding/cache.rs` — in-memory HashMap cache | [ ] |
| 1.8 | `src/embedding/mistral.rs` — `MistralEmbeddingEngine` (feature = `embedding-local`) | [ ] |
| 1.9 | `src/ops/store.rs` — `store_episode()`: embed → RecordBatch → LanceDB | [ ] |
| 1.10 | `src/ops/retrieve.rs` — `retrieve()`: ANN search on `facts.lance` only | [ ] |
| 1.11 | `src/context/token_budget.rs` — character heuristic; `src/context/assembler.rs` — `assemble_context()` | [ ] |
| 1.12 | `src/lib.rs` — `MembraneEngine::open()`, `store_episode()`, `assemble_context()` public API | [ ] |
| 1.13 | `examples/basic_store_retrieve.rs`, `examples/context_assembly.rs` | [ ] |
| 1.14 | Unit tests: `InMemoryFactStore` (no LanceDB); integration tests: `tempfile::TempDir` + LanceDB | [ ] |

### Acceptance criteria

```bash
cargo test                           # all unit tests pass, no external deps
cargo test --features store-lance    # LanceDB integration tests pass
cargo run --example basic_store_retrieve --features "store-lance"
# assemble_context() never exceeds token_budget
# retrieve() returns most recent + most relevant episodes
```

---

## Phase 2 — Lifecycle and Relationships

**Goal:** add DuckDB lifecycle management, LanceGraph entity graph, TTL-based forgetting, and accurate token counting.

### Milestones

| Milestone | Deliverable | Done |
|-----------|-------------|------|
| 2.1 | `src/storage/duck.rs` — `DuckLifecycleStore`: DDL for `fact_lifecycle`, `embedding_cache`, `action_patterns`, `consolidation_jobs` | [ ] |
| 2.2 | `src/storage/graph.rs` — `LanceRelationshipStore`: `entities.lance` + edge datasets | [ ] |
| 2.3 | `src/arrow/` — extend schemas for entity and edge tables | [ ] |
| 2.4 | `src/ops/store.rs` — extend `store_episode()` to register lifecycle row in DuckDB | [ ] |
| 2.5 | `src/ops/retrieve.rs` — extend `retrieve()` with DuckDB lifecycle filter + LanceGraph neighbor expand | [ ] |
| 2.6 | `src/ops/forget.rs` — `forget()`: DuckDB TTL sweep → batch delete from LanceDB | [ ] |
| 2.7 | `src/embedding/cache.rs` — persist cache to DuckDB `embedding_cache` table | [ ] |
| 2.8 | `src/entity/rule_based.rs` — regex NER; `src/entity/resolution.rs` — entity dedup | [ ] |
| 2.9 | `src/context/token_budget.rs` — Gemma 4 `tokenizer.json` via `tokenizers` crate | [ ] |
| 2.10 | DuckDB federation: `read_parquet('./data/memory/facts.lance/**/*.parquet')` integration test | [ ] |

### Acceptance criteria

```bash
cargo test --features "store-lance store-duck"
# TTL expiry: episodes with ttl_secs set are deleted after expiry
# Lifecycle filter: expired ids are excluded from retrieve() results
# Entity graph: neighbors() traversal returns related entities
# Token counting: AssembledContext.tokens_used matches Gemma 4 tokenizer
```

---

## Phase 3 — Automation and Integration

**Goal:** background consolidation, summarization, tamper-evident audit trail, Python bindings.

### Milestones

| Milestone | Deliverable | Done |
|-----------|-------------|------|
| 3.1 | `src/ops/consolidate.rs` — `consolidate()` tokio background task; DuckDB drives job scheduling via `consolidation_jobs` | [ ] |
| 3.2 | `src/ops/summarize.rs` — `summarize()`: compress old episodes into extracted facts via mistral.rs | [ ] |
| 3.3 | `src/audit/ledger.rs` — `AuditBridge<L: AuditLedger>`: emit blake3-signed `AuditRecord` on every `store_episode()` | [ ] |
| 3.4 | PyO3 bindings: `MembraneSession` Python class (`store_episode`, `assemble_context`, `forget`) | [ ] |
| 3.5 | arktrace integration test: chat route stores episodes via membrane; `assemble_context()` feeds Gemma 4 | [ ] |

### Acceptance criteria

```bash
cargo test --features "store-lance store-duck embedding-local audit-bridge"
cargo run --example context_assembly --features "store-lance store-duck embedding-local"
# consolidate(): duplicate episodes merged; summary fact written to facts.lance
# AuditBridge: every store_episode() emits a signed AuditRecord
# Python: import membrane; session.store_episode(...); session.assemble_context(...)
```

---

## Future Considerations (post-Phase 3)

These are not planned but are compatible with the current architecture:

| Item | Notes |
|------|-------|
| Iceberg catalog support | DuckDB already supports `iceberg_scan()` — enable cross-region lakehouse queries |
| Contradiction detection | `ConsistencyChecker` trait hook (Level D automation) — domain-defined rules |
| Streaming token budget | Real-time budget enforcement during generation (requires LLM stream API) |
| edgesentry-inspect grounding | `ContextAssembler` prepends past vessel findings to `SensorFrame` processing |
| Multi-session consolidation | Aggregate episodes across patrol vessel sessions into shared object storage |
