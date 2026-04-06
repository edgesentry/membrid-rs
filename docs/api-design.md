# API Design — Traits, Data Structures, and Operations

## Core Traits

Follows the `RawDataStore` / `AuditLedger` pattern from `edgesentry-rs/crates/edgesentry-audit/src/ingest/storage.rs`. Uses RPITIT (`impl Future` in trait methods, stable since Rust 1.75).

**All API boundaries pass `arrow_array::RecordBatch`** — not serialized bytes, not custom structs. This enables zero-copy between backends and composable pipelines.

```rust
// storage/mod.rs

/// Fact memory backend (LanceDB). Insert and ANN-search Arrow RecordBatches.
pub trait FactStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Insert a RecordBatch of facts (episodes or extracted facts).
    /// Schema must match facts.lance (id, vector, fact_kind, content, ...).
    fn insert(&self, batch: RecordBatch)
        -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// ANN search. Returns matching rows as RecordBatch.
    /// `filter` is an Arrow compute expression string (e.g. "fact_kind = 'episode'").
    fn search(&self, query_vector: &[f32], limit: usize, filter: Option<&str>)
        -> impl Future<Output = Result<RecordBatch, Self::Error>> + Send;

    /// Delete by id column values.
    fn delete(&self, ids: &[MemoryId])
        -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Relationship memory backend (LanceGraph).
pub trait RelationshipStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn upsert_entities(&self, batch: RecordBatch)
        -> impl Future<Output = Result<(), Self::Error>> + Send;

    fn upsert_edges(&self, relation: &str, batch: RecordBatch)
        -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Returns neighbor entity rows as RecordBatch.
    fn neighbors(&self, node_id: &str, depth: u8)
        -> impl Future<Output = Result<RecordBatch, Self::Error>> + Send;
}

/// Lifecycle brain (DuckDB). Speaks Arrow natively via C Data Interface.
pub trait LifecycleStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Execute SQL and return results as RecordBatch.
    /// DuckDB is !Send — implementation wraps with tokio::task::spawn_blocking.
    fn query(&self, sql: &str, params: &[SqlParam])
        -> impl Future<Output = Result<RecordBatch, Self::Error>> + Send;

    fn execute(&self, sql: &str)
        -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Embedding engine. Output is Arrow FixedSizeListArray — directly insertable into Lance.
pub trait EmbeddingEngine: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Embed a single text. Returns a Vec<f32> for convenience.
    fn embed(&self, text: &str)
        -> impl Future<Output = Result<Vec<f32>, Self::Error>> + Send;

    /// Batch embed. Returns FixedSizeListArray for zero-copy insertion into LanceDB.
    fn embed_batch(&self, texts: &[&str])
        -> impl Future<Output = Result<arrow_array::FixedSizeListArray, Self::Error>> + Send;

    fn dimensions(&self) -> usize;
}
```

**In-memory test implementations (provided from Phase 1):**

```rust
pub struct InMemoryFactStore { ... }    // HashMap<MemoryId, RecordBatch>, no external deps
pub struct NoopEmbeddingEngine;         // returns zero vectors (no model required)
```

---

## Core Data Structures

```rust
pub type MemoryId = [u8; 16];   // first 16 bytes of blake3(content + timestamp)
pub type Score = f32;           // relevance score [0.0, 1.0]
pub type TokenCount = usize;

pub struct Episode {
    pub id: MemoryId,
    pub session_id: String,
    pub timestamp_ms: u64,
    pub role: Role,               // User | Assistant | System
    pub content: String,
    pub embedding: Option<Vec<f32>>,
    pub ttl_secs: Option<u64>,
    pub metadata: EpisodeMetadata,
}

pub struct Entity {
    pub id: String,               // "person:alice"
    pub kind: EntityKind,         // Person | Organization | Location | Concept | Event | Custom(String)
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub attributes: serde_json::Value,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub mention_count: u32,
}

pub struct Fact {
    pub id: String,
    pub subject_entity_id: String,
    pub predicate: String,
    pub object_entity_id: Option<String>,
    pub object_literal: Option<String>,
    pub confidence: f32,
    pub source_episode_ids: Vec<MemoryId>,
    pub valid_from_ms: Option<u64>,   // bi-temporal
    pub valid_until_ms: Option<u64>,  // bi-temporal
}

pub struct RetrievedMemory {
    pub id: MemoryId,
    pub content: String,
    pub score: Score,
    pub tier: MemoryTier,   // Working | Fact | Relationship | Lifecycle
    pub timestamp_ms: u64,
    pub metadata: serde_json::Value,
}

pub struct AssembledContext {
    pub prompt_prefix: String,
    pub tokens_used: TokenCount,
    pub sources: Vec<RetrievedMemory>,   // for attribution and debugging
    pub truncated: bool,
}
```

---

## Operations

| Function | Description | Phase |
|----------|-------------|-------|
| `store_episode()` | Generate embedding (Arrow) → write RecordBatch to LanceDB; register lifecycle row in DuckDB | Phase 1 |
| `retrieve()` | Multi-stage: ANN on LanceDB + graph traversal on LanceGraph + lifecycle filter from DuckDB | Phase 1 (ANN only) / Phase 2 (all stages) |
| `assemble_context()` | Rank retrieved RecordBatches by score, enforce token budget, format prompt prefix | Phase 1 |
| `forget()` | TTL expiry sweep + explicit deletion | Phase 2 |
| `consolidate()` | Background task: merge similar memories, deduplicate | Phase 3 |
| `summarize()` | Compress old episodes into Facts (via mistral.rs inference) | Phase 3 |
