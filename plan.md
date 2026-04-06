# membrane — LLM Memory Management for Edge AI

## 1. Problem Statement

### Core Challenge

Small LLMs (Gemma 4 2B/4B) cannot be made performant by scaling up the model — resource constraints on edge devices make that path closed. membrane's role is to **compensate on the outside of the model**, raising cost-performance by improving what goes *in*, not what the model *is*.

Three strategies drive the design:

**Information Diet** — Rather than stuffing all available context into the prompt, membrane selects only what is needed right now. Small models degrade sharply as context grows (Lost-in-the-Middle effect). Less is more.

**Externalized Decision-Making** — Entity resolution, fact consistency checks, deduplication, and structured retrieval are handled by membrane, not the LLM. The model's job is reasoning; everything else is delegated out.

**Consistency Enforcement** — Contradictory memories, stale facts, and duplicate episodes are resolved *before* being handed to the model, so the model never has to choose between conflicting information.

### Library Scope

membrane is a **general-purpose library** — it does not target a fixed use case. Because of this, the boundary between what the library automates and what the user designs must be explicit. See Section 16 for the full guideline.

### Existing Rust Ecosystem Survey

Before committing to building membrane, we surveyed the Rust ecosystem for similar libraries. **No existing Rust library covers the full scope membrane targets.** Specific gaps found:

| Library | What it does | Gap vs. membrane |
|---------|-------------|-----------------|
| **Motorhead** (YC-backed, Rust) | Session memory server with sliding window + incremental summarization | Requires external Redis + OpenAI API; server model, not embedded; no offline/edge use |
| **Cortex Memory** | 3-tier progressive memory (L0/L1/L2), Qdrant backend, MCP/CLI interface | No vector+SQL+graph unification; no token budget enforcement; no local embedding; no DuckDB |
| **Kalosm** | Local LLM runner (Candle-based) with basic conversation memory | No multi-tier memory types; no DuckDB/graph; not a memory management system |
| **Rig** (0xPlaygrounds) | Modular LLM app framework with pluggable VectorStoreIndex | No memory system; no token budget; requires external LLM providers |
| **llm-memory-graph** | Graph-only persistent store for LLM interactions | No vector/SQL layer; no episodic/semantic separation; no token budget |
| **GraphRAG-rs** | Knowledge graph from documents, entity extraction | Document-focused; no conversational memory tiers; no token budget |
| **AutoAgents** | Multi-agent framework with sliding window memory | No vector backend; no token budget; no local embeddings |
| **mem7** | Rust mem0-like with Ebbinghaus forgetting curve + fact extraction | No multi-tier types; no token budget enforcement; production/cloud focus |
| **memory-mcp-rs** | MCP memory server on SQLite | No vector/graph; no token budget; MCP protocol only |

**Confirmed gaps in the Rust ecosystem:**
- No unified 4-tier memory system (working / episodic / semantic / procedural)
- No library combines vector (LanceDB) + structured (DuckDB) + graph (LanceGraph) in one embedded crate
- No token budget enforcement *during* context assembly (existing tools optimize after-the-fact)
- No edge-first design targeting sub-4B models with small context windows
- No externalized decision-making layer for compensating small model limitations

membrane fills this gap.

### Reference Implementations

Ideas borrowed from existing systems:

| System | Borrowed Idea |
|--------|--------------|
| **MemGPT / Letta** | 3-tier memory hierarchy (in-context / recall / archival); overflow → summarize pattern |
| **Zep / Graphiti** | Bi-temporal knowledge graph (`valid_from` / `valid_until` on facts) |
| **mem0** | Automatic memory extraction pipeline triggered after each episode |
| **Vertex AI Agent Engine** | Async background processing for memory consolidation |
| **Neo4j Memory MCP** | POLE+O entity model (Person, Organization, Location, Event, Object); fulltext + graph traversal |
| **LlamaIndex ChatMemoryBuffer** | Token budget enforcement + conversation compression |
| **Cortex Memory** | Progressive disclosure (L0 abstract → L1 overview → L2 detail) |

Key distinction from MemGPT: MemGPT asks the *model* to decide when to read/write memory. membrane makes those decisions *outside* the model — critical for small models that cannot reliably self-manage.

### Technology Stack

- **Target models**: Gemma 4 2B / 4B (quantized INT8/FP16, 2–8 GB RAM)
- **Deployment**: edge-first; cloud deployment uses the same codebase — sync strategy is out of scope
- **Integration**: edgesentry-rs (Rust), arktrace (Python)
- **IO common language**: **Apache Arrow** — all data crossing backend boundaries is `RecordBatch`; zero-copy between LanceDB, DuckDB, and the embedding pipeline
- **Fact memory**: **LanceDB** — stores facts and episodes as Arrow/Lance columnar data with vector embeddings; lakehouse model (local path or object storage URI, same API)
- **Relationship memory**: **LanceGraph** — entity graph stored as Arrow/Lance datasets; lakehouse model
- **Lifecycle brain**: **DuckDB** — orchestrates TTL, consolidation state, embedding cache, session index; federates across lakehouses via Arrow, Parquet, and Iceberg
- **Embedding / inference**: **Mistral.rs** (local quantized models; outputs Arrow arrays)

---

## 2. Storage Architecture

### Arrow as IO Common Language

Apache Arrow is the lingua franca that connects all backends with zero-copy data transfer:

```
                    ┌─────────────────────────────────────────────┐
                    │             Apache Arrow boundary            │
                    │                                             │
  Mistral.rs  ──── RecordBatch ────►  LanceDB (FactMemory)       │
  embedding                          Lance columnar format        │
  pipeline                           (Arrow-native on disk)       │
                                              │                   │
                    RecordBatch (Arrow IPC)   │                   │
                    ◄─────────────────────────┘                   │
                    │                                             │
                    ▼                                             │
             LanceGraph (RelationshipMemory)                      │
             Lance datasets = Arrow tables                        │
                    │                                             │
                    │  Arrow C Data Interface / read_parquet()    │
                    ▼                                             │
             DuckDB (LifecycleBrain)                              │
             queries Lance files directly as Parquet              │
             federates across Iceberg catalogs                    │
                    └─────────────────────────────────────────────┘
```

Every API boundary in membrane passes `arrow_array::RecordBatch`, not serialized bytes. This means:
- No copy/deserialize overhead between LanceDB → DuckDB
- Embedding output from mistral.rs is `FixedSizeListArray` — directly inserted into Lance tables
- DuckDB reads LanceDB's `.lance` files as Parquet without a separate export step

### Backend Roles

| Backend | Role | Why |
|---------|------|-----|
| **LanceDB** | **Fact memory** — stores episodes and extracted facts with their vector embeddings | Lance format is Arrow/Parquet columnar; ANN search + payload filter in one query; same API for local FS and object storage |
| **LanceGraph** | **Relationship memory** — entity graph stored as Arrow/Lance datasets | Graph edges as Lance tables; same lakehouse model as LanceDB; traversal via Arrow joins |
| **DuckDB** | **Lifecycle brain** — orchestrates TTL, consolidation state, embedding cache, session index; federates across lakehouses | Speaks Arrow natively; can `read_parquet()` Lance files directly; Iceberg catalog support for cross-store queries |
| **In-process** | **Working memory** — current session ring buffer | Zero latency; lives entirely in the context window; no Arrow overhead needed |

### Lakehouse Deployment Model

LanceDB and LanceGraph use the same Lance storage format regardless of where data lives:

```
Local (edge):                          Object storage (cloud):
  ./data/memory/facts.lance     ←→       s3://bucket/memory/facts.lance
  ./data/memory/graph/               s3://bucket/memory/graph/
  ./data/memory/relations.lance       gcs://bucket/memory/relations.lance

Same Rust API. Same schema. Deployment is configuration, not code.
```

DuckDB federates across these locations via:
- `read_parquet('s3://...')` / `read_parquet('./data/...')` — Lance files are valid Parquet
- Iceberg catalog queries for versioned, ACID-compliant lakehouse access
- Arrow Flight for in-process zero-copy exchange with LanceDB

**Edge ↔ cloud sync is intentionally out of scope.** This is a deliberate scope decision, not an omission.

membrane's role is to provide knowledge that assists judgment: similar past cases, trend/frequency signals, and entity relationships. In edge AI deployments, anything on the critical decision path must be fully local — a cloud round-trip is not acceptable for real-time inference. membrane therefore operates entirely within the edge process.

Learning and improvement — aggregating episodes and extracted facts into object storage for offline analysis or model fine-tuning — is post-processing work that happens outside the judgment loop. That belongs in a separate layer, not in a memory management library.

Possible sync strategies (shared object storage, log-based replication, Iceberg table versioning) are application-level decisions. membrane imposes no constraint and is compatible with any of them via the lakehouse model: the same Lance/Parquet files readable locally are readable from object storage without code changes.

---

## 3. Memory Taxonomy

| Tier | Role | Backend | Retrieval |
|------|------|---------|-----------|
| **WorkingMemory** | Current session conversation (lives in context window) | In-process ring buffer | Direct access |
| **FactMemory** | Episodes and extracted facts with vector embeddings | LanceDB | ANN vector search + Arrow payload filter |
| **RelationshipMemory** | Entity graph — who/what is connected to whom/what | LanceGraph | Graph traversal over Arrow/Lance datasets |
| **LifecycleBrain** | TTL state, consolidation progress, embedding cache, session index | DuckDB | SQL; federates across LanceDB/Iceberg |

**Why this maps to the OSS reference designs:**

- MemGPT's in-context / recall / archival → WorkingMemory / FactMemory / LifecycleBrain
- Zep's bi-temporal KG → FactMemory holds `valid_from` / `valid_until` on fact payloads
- Neo4j Memory MCP's graph → RelationshipMemory via LanceGraph
- Vertex AI Memory Bank's async background processing → LifecycleBrain (DuckDB) drives consolidation triggers

---

## 4. Storage Schema

### 4.1 LanceDB — Fact Memory

All Lance tables are Arrow/Parquet columnar. Payload fields are Arrow structs stored alongside the vector column.

```
facts.lance
  ┌─────────────────────────────────────────────────────────────────┐
  │  id           : binary(16)     -- blake3(content + timestamp)  │
  │  vector       : list<f32>[D]   -- embedding (e.g. 768-dim)     │
  │  fact_kind    : utf8           -- "episode" | "extracted_fact"  │
  │  session_id   : utf8                                           │
  │  role         : utf8           -- "user" | "assistant" | "sys"  │
  │  content      : utf8           -- full text                    │
  │  preview      : utf8           -- first 256 chars              │
  │  subject_id   : utf8           -- entity id (facts only)       │
  │  predicate    : utf8           -- "works_at" etc. (facts only)  │
  │  object_id    : utf8           -- entity id or null            │
  │  object_value : utf8           -- literal value or null        │
  │  confidence   : float32                                        │
  │  valid_from   : int64          -- ms; bi-temporal              │
  │  valid_until  : int64          -- ms; null = still valid       │
  │  timestamp_ms : int64                                          │
  │  topic_tags   : list<utf8>                                     │
  │  entity_ids   : list<utf8>                                     │
  └─────────────────────────────────────────────────────────────────┘
```

Single table for both raw episodes and extracted SPO facts — `fact_kind` distinguishes them. ANN search + Arrow predicate filtering runs in one query. DuckDB reads this table directly as Parquet for lifecycle queries.

### 4.2 LanceGraph — Relationship Memory

Rust re-implementation of `arktrace/src/graph/store.py`. Each edge type is a separate Lance dataset (Arrow table).

```
memory_graph/
  entities.lance
    id         : utf8           -- "person:alice"
    label      : utf8           -- POLE+O kind
    name       : utf8
    aliases    : list<utf8>
    attributes : utf8           -- JSON string
    vector     : list<f32>[D]   -- entity description embedding

  edges/
    RELATED_TO.lance    ┐
    CAUSES.lance        │  src_id : utf8, dst_id : utf8,
    CONTRADICTS.lance   │  weight : float32, properties : utf8 (JSON)
    MENTIONED_WITH.lance┘
```

Graph traversal is implemented as Arrow joins over Lance datasets — no separate graph engine process. Works identically on local FS or object storage.

### 4.3 DuckDB — Lifecycle Brain

DuckDB holds **no primary facts**. Its role is orchestration: tracking what exists, what has expired, what needs consolidation, and caching embeddings. It reads LanceDB/LanceGraph data directly as Parquet when it needs to query content.

```sql
-- Lifecycle index: tracks every fact entry without duplicating content
CREATE TABLE fact_lifecycle (
    id              BLOB PRIMARY KEY,   -- matches facts.lance id
    fact_kind       VARCHAR NOT NULL,   -- "episode" | "extracted_fact"
    session_id      VARCHAR NOT NULL,
    timestamp_ms    BIGINT NOT NULL,
    ttl_expires_ms  BIGINT,             -- NULL = no expiry
    consolidated    BOOLEAN NOT NULL DEFAULT false,
    summary_id      BLOB,               -- id of summary fact if consolidated
    created_at_ms   BIGINT NOT NULL DEFAULT epoch_ms(now())
);
CREATE INDEX idx_lifecycle_session ON fact_lifecycle (session_id, timestamp_ms);
CREATE INDEX idx_lifecycle_ttl     ON fact_lifecycle (ttl_expires_ms) WHERE ttl_expires_ms IS NOT NULL;
CREATE INDEX idx_lifecycle_pending ON fact_lifecycle (consolidated) WHERE consolidated = false;

-- Embedding cache (avoids re-computing on edge hardware)
CREATE TABLE embedding_cache (
    text_hash    BLOB    PRIMARY KEY,   -- blake3([u8; 32])
    embedding    BLOB    NOT NULL,      -- postcard-encoded Vec<f32>
    model_id     VARCHAR NOT NULL,
    cached_at_ms BIGINT  NOT NULL
);

-- Procedural memory: action patterns (structured, not vector-searchable)
CREATE TABLE action_patterns (
    id              VARCHAR PRIMARY KEY,
    trigger_pattern TEXT    NOT NULL,
    action_sequence TEXT    NOT NULL,   -- JSON array
    success_count   INTEGER NOT NULL DEFAULT 0,
    failure_count   INTEGER NOT NULL DEFAULT 0,
    last_used_ms    BIGINT  NOT NULL
);

-- Consolidation jobs: tracks background merge/summarize work
CREATE TABLE consolidation_jobs (
    id           VARCHAR PRIMARY KEY,
    status       VARCHAR NOT NULL,      -- "pending" | "running" | "done" | "failed"
    target_ids   BLOB[],               -- fact ids to consolidate
    result_id    BLOB,                 -- output summary fact id
    started_ms   BIGINT,
    finished_ms  BIGINT,
    error        TEXT
);
```

**DuckDB federating across lakehouses:**

```sql
-- Read LanceDB facts directly (Lance files are valid Parquet)
SELECT id, content, confidence
FROM read_parquet('./data/memory/facts.lance/**/*.parquet')
WHERE valid_until IS NULL AND fact_kind = 'extracted_fact';

-- Query across Iceberg catalog (cloud deployment)
SELECT * FROM iceberg_scan('s3://bucket/memory/facts');

-- Join lifecycle metadata with LanceDB content
SELECT f.content, l.ttl_expires_ms
FROM fact_lifecycle l
JOIN read_parquet('./data/memory/facts.lance/**/*.parquet') f ON l.id = f.id
WHERE l.ttl_expires_ms < epoch_ms(now());
```

---

## 5. Core Trait Design

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

## 6. Core Data Structures

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

## 7. Operations

| Function | Description | Phase |
|----------|-------------|-------|
| `store_episode()` | Generate embedding (Arrow) → write RecordBatch to LanceDB; register lifecycle row in DuckDB | Phase 1 |
| `retrieve()` | Multi-stage: ANN on LanceDB + graph traversal on LanceGraph + lifecycle filter from DuckDB | Phase 1 (ANN only) / Phase 2 (all stages) |
| `assemble_context()` | Rank retrieved RecordBatches by score, enforce token budget, format prompt prefix | Phase 1 |
| `forget()` | TTL expiry sweep + explicit deletion | Phase 2 |
| `consolidate()` | Background task: merge similar memories, deduplicate | Phase 3 |
| `summarize()` | Compress old episodes into Facts (via mistral.rs inference) | Phase 3 |

---

## 8. Embedding Pipeline (Mistral.rs)

```
mistralrs-core Pipeline
  └── quantized embedding model (INT8 / FP16)
       e.g. nomic-embed-text-1.5-q8  (137M params, 768-dim)
            Gemma 4 embedding variant

Config:
  batch_size: 4           # tuned for edge hardware
  max_seq_len: 512        # trim to episode preview length

Output: arrow_array::FixedSizeListArray
  → inserted directly into facts.lance vector column (zero-copy)

Cache strategy:
  Phase 1: HashMap<[u8;32], Vec<f32>>  in-memory
  Phase 2: persisted to DuckDB embedding_cache table
  Key: blake3(text) → [u8; 32]
  Invalidation: cache entries are keyed by model_id; changing models auto-invalidates
```

Fallback: `NoopEmbeddingEngine` returns zero vectors — enables CI and tests without model files.

---

## 9. Context Assembly

Given query `q` and token budget `budget`:

```
1. WorkingMemory scan            → most recent N turns (always included; anchors recency)
2. FactStore ANN search          → embed(q) → LanceDB ANN on facts.lance
3. LifecycleStore filter         → DuckDB removes expired / consolidated ids
4. RelationshipStore traversal   → LanceGraph neighbors of entities found in step 2 (Phase 2)
5. Merge results as RecordBatch
6. Score: Relevance × Recency × Diversity
7. Deduplicate by blake3(content)
8. Trim to fit within token_budget
9. Format as AssembledContext.prompt_prefix
```

All intermediate results are Arrow RecordBatches — no intermediate serialization.

**Token counting:**
- Phase 1: `content.len() / 4` character heuristic
- Phase 2: `tokenizers` crate loads Gemma 4 `tokenizer.json` (can be `include_bytes!` for embedded targets)

---

## 10. Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `async` | ON | tokio async API (RPITIT trait methods) |
| `store-lance` | ON | LanceDB (FactStore) + LanceGraph (RelationshipStore) |
| `store-duck` | ON | DuckDB (LifecycleStore) |
| `embedding-local` | OFF | mistral.rs local inference (heavy dependency) |
| `pyo3-bindings` | OFF | Python bindings for arktrace (Phase 3) |
| `audit-bridge` | OFF | edgesentry-audit integration (Phase 3) |

---

## 11. Integration with Existing Crates

### edgesentry-audit

`AuditBridge` (`audit-bridge` feature): each `store_episode()` call emits a blake3-signed `AuditRecord` via the existing `AuditLedger` trait. Memory writes become tamper-evident.

```rust
// audit/ledger.rs
pub struct AuditBridge<L: AuditLedger> { ledger: L }
impl<L: AuditLedger> AuditBridge<L> {
    pub fn record_store(&mut self, id: &MemoryId, content_hash: &Hash32) -> Result<()>;
}
```

### arktrace (Python)

Phase 3: PyO3 bindings expose `MembraneSession` as a Python class:

```python
import membrane
session = membrane.MembraneSession(lance_uri="./data/memory/facts.lance",
                                   duck_path="./data/memory/lifecycle.db")
await session.store_episode(role="user", content="...")
ctx = await session.assemble_context(query="...", token_budget=4096)
```

arktrace's existing LanceDB files at `data/processed/` share the same Lance format — schema alignment is a configuration concern, not a code change.

### edgesentry-inspect

Future: `ContextAssembler` prepends relevant past findings before processing a `SensorFrame`, enabling inspection reports that "remember" prior observations about a vessel or location.

---

## 12. Cargo.toml

```toml
[package]
name = "membrane"
version = "0.1.0"
edition = "2021"

[features]
default = ["async", "store-lance", "store-duck"]
async = ["dep:tokio"]
embedding-local = ["dep:mistralrs", "dep:mistralrs-core"]
store-lance = ["dep:lancedb", "dep:arrow-array", "dep:arrow-schema", "dep:arrow-cast"]
store-duck = ["dep:duckdb"]
pyo3-bindings = ["dep:pyo3"]
audit-bridge = ["dep:edgesentry-audit"]

[dependencies]
# Align with edgesentry-rs workspace versions
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
blake3 = "1.5"
postcard = { version = "1.1", default-features = false, features = ["alloc"] }
tracing = "0.1"
hex = "0.4"
uuid = { version = "1", features = ["v4"] }

# Arrow is always present — it is the IO common language
arrow-array  = { version = "53" }
arrow-schema = { version = "53" }

tokio = { version = "1", features = ["rt", "sync", "macros", "time"], optional = true }

lancedb    = { version = "0.9",  optional = true }
arrow-cast = { version = "53",   optional = true }   # for Lance ↔ DuckDB Arrow IPC

duckdb = { version = "1.1", features = ["bundled"], optional = true }

mistralrs      = { version = "0.3", optional = true }
mistralrs-core = { version = "0.3", optional = true }

pyo3 = { version = "0.22", features = ["extension-module"], optional = true }

edgesentry-audit = { path = "../edgesentry-rs/crates/edgesentry-audit", optional = true }

[dev-dependencies]
tokio    = { version = "1", features = ["rt-multi-thread", "macros"] }
tempfile = "3"
```

Note: `arrow-array` and `arrow-schema` are **non-optional** — Arrow is the IO boundary for all backends.

---

## 13. Crate Layout

```
membrane/
├── Cargo.toml
├── plan.md
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

Key addition: `src/arrow/` module owns all Arrow schema definitions and `RecordBatch` ↔ domain type conversions. This is the single place that defines the Lance table schemas — all other modules import from here.

---

## 14. Implementation Phases

### Phase 1 — Fact Memory Working

**In scope:**
- `WorkingMemory` (in-process ring buffer, no external deps)
- `FactStore` trait + LanceDB implementation (`facts.lance`)
- `EmbeddingEngine` trait + `NoopEmbeddingEngine` (tests) + mistral.rs impl
- `EmbeddingCache` (in-memory HashMap)
- `src/arrow/` module: Arrow schema for `facts.lance`, `RecordBatch` ↔ `Episode` conversions
- `store_episode()`, `retrieve()` (ANN only), `assemble_context()`
- `InMemoryFactStore` for unit tests (no LanceDB process needed)

**Out of scope:**
- DuckDB `LifecycleStore`
- LanceGraph `RelationshipStore`
- Entity extraction / resolution
- `consolidate()`, `summarize()`
- PyO3, AuditBridge
- Exact token counting (character heuristic only)

### Phase 2 — Lifecycle and Relationships

- `LifecycleStore` (DuckDB): `fact_lifecycle`, `embedding_cache`, `action_patterns`, `consolidation_jobs` tables
- TTL-based `forget()`: DuckDB TTL sweep → delete from LanceDB by id batch
- `RelationshipStore` (LanceGraph): `entities.lance` + edge datasets
- Lifecycle filter in `retrieve()`: DuckDB removes expired ids before returning results
- Rule-based NER and entity resolution (no model required)
- DuckDB-persisted `EmbeddingCache`
- Gemma 4 `tokenizer.json` for accurate token counting
- DuckDB federation: `read_parquet()` on `facts.lance` for cross-store queries

### Phase 3 — Automation and Integration

- `consolidate()` background tokio task (DuckDB drives job scheduling)
- `summarize()` (mistral.rs compresses old episodes into extracted facts)
- edgesentry-audit `AuditBridge`
- PyO3 bindings (arktrace Python integration)

---

## 15. Key Reference Files

| File | Purpose |
|------|---------|
| `edgesentry-rs/crates/edgesentry-audit/src/ingest/storage.rs` | Template for trait pattern (sync + async feature flag, RPITIT) |
| `edgesentry-rs/crates/edgesentry-audit/src/buffer/mod.rs` | Generic storage layer pattern (`OfflineBuffer<S>`) |
| `arktrace/src/graph/store.py` | LanceGraph schema (node/edge Lance dataset layout) |
| `arktrace/src/ingest/schema.py` | DuckDB DDL reference (column types, index strategy) |
| `edgesentry-rs/Cargo.toml` | Workspace dependency versions (blake3 1.5, serde 1, thiserror 2, postcard 1.1) |

---

## 16. Verification

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

---

## 17. Automation Guidelines — Library Layer vs. User Layer

membrane is a general-purpose library with no fixed use case. This boundary is the most important design decision.

### What membrane guarantees

- Storage integrity (no duplicates, TTL enforcement, content-hash uniqueness)
- Context window is never exceeded (token budget strictly enforced)
- Retrieval diversity (Recency × Relevance × Diversity composite score)
- Entity consistency (same entity not stored under multiple IDs)
- Backend error isolation (one storage backend failing does not crash the engine)

### What membrane does not guarantee (user's responsibility)

- What qualifies as a memory-worthy event
- Which tier to route an episode to
- When to trigger retrieval
- How to construct retrieval queries

---

### Automation Level A — Always done by membrane (non-configurable)

| Function | Rationale |
|----------|-----------|
| Embedding cache (blake3 key) | Re-computing on edge hardware is expensive; savings are unconditional |
| Token budget enforcement | Exceeding the limit breaks the model; no exceptions |
| Content-hash deduplication | Storing identical text twice has no benefit |
| TTL expiry cleanup | Serving expired memories to the model is harmful |

### Automation Level B — Opinionated defaults, user-configurable

| Function | Default | How to change |
|----------|---------|---------------|
| Scoring weights | Relevance 0.6 / Recency 0.3 / Diversity 0.1 | `RetrievalConfig::weights` |
| Search result limit | 10 | `RetrievalConfig::limit` |
| WorkingMemory max turns | 20 | `WorkingMemoryConfig::max_turns` |
| Summarization threshold | 100 episodes | `ConsolidateConfig::summarize_threshold` |
| Entity confidence threshold | 0.7 | `EntityConfig::confidence_threshold` |

### Automation Level C — User-controlled via hooks

```rust
/// Controls which episodes are worth storing
pub trait WorthyFilter: Send + Sync {
    fn is_worthy(&self, episode: &Episode) -> bool;
}

/// Controls which memory tier an episode is routed to
pub trait TierRouter: Send + Sync {
    fn route(&self, episode: &Episode) -> MemoryTier;
}

/// Builds the retrieval query from current context
pub trait QueryBuilder: Send + Sync {
    fn build(&self, context: &QueryContext) -> RetrievalQuery;
}

/// Post-processes the assembled context string
pub trait ContextFormatter: Send + Sync {
    fn format(&self, memories: &[RetrievedMemory]) -> String;
}
```

All hooks have default implementations (`DefaultWorthyFilter`, `DefaultTierRouter`, etc.). Users replace only what they need.

### Automation Level D — User designs entirely (membrane provides primitives only)

| Concern | What membrane provides |
|---------|----------------------|
| Entity taxonomy | `EntityKind::Custom(String)` for arbitrary extension |
| Fact predicate vocabulary | Free string ("works_at", "contradicts" — domain-defined) |
| Consolidation business logic | `ConsolidationPolicy` trait to implement |
| Contradiction detection rules | `ConsistencyChecker` trait to implement |

---

### Design Checklist for Small LLMs

Keep these constraints in mind during implementation and use:

**Information diet:**
- [ ] `AssembledContext` never exceeds `token_budget`
- [ ] Retrieved memories are diverse (no repeated topic clusters)
- [ ] WorkingMemory's most recent turns are always included
- [ ] Long episodes are represented by preview (first N chars); full content fetched only on demand

**Externalized decisions:**
- [ ] Entity resolution (Alice == alice) is done by membrane, not the model
- [ ] Facts with `valid_until_ms < now()` are filtered before retrieval
- [ ] Duplicate episodes are merged by `consolidate()`, not by the model

**Consistency:**
- [ ] Facts for the same `entity_id` are managed in chronological order (bi-temporal)
- [ ] Episodes with `ttl_expires_ms` are reliably deleted by `forget()`
- [ ] Embedding cache is invalidated when `model_id` changes

---

### Recommended Configurations by Use Case

#### Use Case A: Conversational Agent

```rust
MembraneConfig {
    working_memory: WorkingMemoryConfig {
        max_turns: 20,
        overflow_strategy: OverflowStrategy::SummarizeOldest,
    },
    retrieval: RetrievalConfig {
        weights: ScoringWeights { relevance: 0.5, recency: 0.4, diversity: 0.1 },
        limit: 5,  // small models benefit from fewer, more precise results
    },
    tier_router: Box::new(DefaultTierRouter),
}
```

#### Use Case B: Document Q&A (RAG)

```rust
MembraneConfig {
    working_memory: WorkingMemoryConfig {
        max_turns: 5,  // minimal conversation history
        ..Default::default()
    },
    retrieval: RetrievalConfig {
        weights: ScoringWeights { relevance: 0.8, recency: 0.1, diversity: 0.1 },
        limit: 10,
    },
    tier_router: Box::new(DocumentChunkRouter),  // custom: routes doc chunks to Episodic
}
```

#### Use Case C: Constrained Edge Device (≤2 GB RAM)

```rust
MembraneConfig {
    working_memory: WorkingMemoryConfig {
        max_turns: 5,
        overflow_strategy: OverflowStrategy::DropOldest,  // no summarization inference cost
    },
    retrieval: RetrievalConfig {
        limit: 3,  // absolute minimum context
        ..Default::default()
    },
    embedding: EmbeddingConfig {
        model_id: "nomic-embed-text-1.5-q8".into(),  // 137M params, INT8
        batch_size: 1,
    },
}
```
