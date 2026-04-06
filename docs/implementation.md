# Implementation — Crate Layout, Phases, and Verification

## Crate Layout

```
membrane/
├── Cargo.toml
├── README.md                  # human quick-start
├── AGENTS.md                  # AI agent entry point
├── docs/                      # full design docs (this directory)
├── src/
│   ├── lib.rs                 # public API, feature gating
│   ├── error.rs               # MembraneError (thiserror)
│   ├── types.rs               # MemoryId, Episode, Entity, Fact, RetrievedMemory, ...
│   │
│   ├── memory/
│   │   ├── mod.rs             # MemoryTier enum, WorkingMemory
│   │   └── working.rs         # in-process ring buffer
│   │
│   ├── storage/
│   │   ├── mod.rs             # FactStore, RelationshipStore, LifecycleStore, EmbeddingEngine traits
│   │   ├── lance.rs           # FactStore impl via LanceDB (feature = store-lance)
│   │   ├── graph.rs           # RelationshipStore impl via LanceGraph (feature = store-lance)
│   │   └── duck.rs            # LifecycleStore impl via DuckDB (feature = store-duck)
│   │
│   ├── arrow/
│   │   ├── mod.rs             # Arrow schema definitions for facts.lance, entities.lance
│   │   └── convert.rs         # Episode/Fact/Entity → RecordBatch, RecordBatch → RetrievedMemory
│   │
│   ├── embedding/
│   │   ├── mod.rs             # EmbeddingEngine trait, NoopEmbeddingEngine
│   │   ├── mistral.rs         # mistral.rs impl → FixedSizeListArray (feature = embedding-local)
│   │   └── cache.rs           # EmbeddingCache (Phase 1: HashMap; Phase 2: DuckDB)
│   │
│   ├── ops/
│   │   ├── mod.rs
│   │   ├── store.rs           # store_episode(): embed → RecordBatch → LanceDB + DuckDB lifecycle
│   │   ├── retrieve.rs        # retrieve(): ANN → lifecycle filter → graph expand → score
│   │   ├── consolidate.rs     # consolidate() background task (Phase 3)
│   │   ├── forget.rs          # forget(): DuckDB TTL sweep → LanceDB delete
│   │   └── summarize.rs       # summarize(): old facts → compressed fact via mistral.rs (Phase 3)
│   │
│   ├── context/
│   │   ├── mod.rs
│   │   ├── assembler.rs       # ContextAssembler, assemble_context()
│   │   └── token_budget.rs    # TokenCounter (Phase 1: heuristic; Phase 2: tokenizers)
│   │
│   ├── entity/
│   │   ├── mod.rs             # EntityExtractor trait
│   │   ├── rule_based.rs      # regex-based NER (Phase 2)
│   │   └── resolution.rs      # entity deduplication and merging (Phase 2)
│   │
│   └── audit/
│       └── ledger.rs          # AuditBridge (feature = audit-bridge)
│
└── examples/
    ├── basic_store_retrieve.rs
    └── context_assembly.rs
```

Key: `src/arrow/` module owns all Arrow schema definitions and `RecordBatch` ↔ domain type conversions. This is the single place that defines the Lance table schemas — all other modules import from here.

See [roadmap.md](roadmap.md) for implementation phases, milestone checklists, and acceptance criteria.

---

## Key Reference Files

| File | Purpose |
|------|---------|
| `edgesentry-rs/crates/edgesentry-audit/src/ingest/storage.rs` | Template for trait pattern (sync + async feature flag, RPITIT) |
| `edgesentry-rs/crates/edgesentry-audit/src/buffer/mod.rs` | Generic storage layer pattern (`OfflineBuffer<S>`) |
| `arktrace/src/graph/store.py` | LanceGraph schema (node/edge Lance dataset layout) |
| `arktrace/src/ingest/schema.py` | DuckDB DDL reference (column types, index strategy) |
| `edgesentry-rs/Cargo.toml` | Workspace dependency versions (blake3 1.5, serde 1, thiserror 2, postcard 1.1) |

---

## Verification

```bash
# Phase 1: unit tests (no external dependencies)
cd membrane
cargo test

# Phase 1: LanceDB integration tests (uses tempfile::TempDir)
cargo test --features store-lance

# Phase 2: DuckDB integration tests
cargo test --features store-duck

# Full stack examples
cargo run --example basic_store_retrieve --features "store-lance store-duck embedding-local"
cargo run --example context_assembly     --features "store-lance store-duck embedding-local"
```

**End-to-end check:**
1. arktrace chat route stores episodes via membrane
2. `assemble_context()` retrieves relevant context
3. Assembled context + query sent to Gemma 4 via mistral.rs
4. Verify `retrieve()` returns correct results across sessions
5. Verify token budget is never exceeded
