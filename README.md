# membrane

**LLM memory management for edge AI with small models.**

membrane gives small language models (Gemma 4 2B/4B) better recall without making them bigger. It manages what goes *into* the model — not what the model *is*.

---

## The problem

Running a 2B–4B parameter model on an edge device means living inside hard resource constraints. You cannot scale your way out. The model's context window is limited, its reasoning degrades as context grows (Lost-in-the-Middle), and re-computing or cloud-fetching information at inference time is not an option.

membrane addresses this from the outside:

- **Information diet** — select only what is relevant right now, not everything that has ever been said
- **Externalized decisions** — entity resolution, deduplication, fact consistency checks, and structured retrieval happen in Rust, not in the model
- **Consistency enforcement** — contradictory or stale facts are filtered before reaching the prompt

The model's job is reasoning. Everything else is membrane's job.

---

## What membrane provides

| Capability | How |
|-----------|-----|
| **Fact memory** | Episodes and extracted facts stored as Arrow/Lance columnar data in LanceDB, with ANN vector search |
| **Relationship memory** | Entity graph (who knows what, what causes what) stored in LanceGraph as Arrow datasets |
| **Lifecycle management** | TTL expiry, consolidation state, embedding cache — orchestrated by DuckDB as the lifecycle brain |
| **Working memory** | In-process ring buffer for the current session, always within the context window |
| **Context assembly** | Multi-tier retrieval ranked by Relevance × Recency × Diversity, hard-capped at your token budget |
| **Local embeddings** | mistral.rs runs quantized embedding models (nomic-embed-text, Gemma 4 variants) on-device |

---

## Architecture

### Apache Arrow as the IO boundary

All data crossing backend boundaries is `arrow_array::RecordBatch`. There is no intermediate serialization. Embedding output from mistral.rs is a `FixedSizeListArray` inserted directly into LanceDB. DuckDB reads Lance files as Parquet without an export step.

```
mistral.rs         FixedSizeListArray
embedding    ──────────────────────────►  LanceDB  (facts.lance)
pipeline                                  Arrow/Lance columnar
                                               │
                               Arrow IPC / read_parquet()
                                               │
                                               ▼
                                          DuckDB  (lifecycle.db)
                                          TTL · cache · jobs
                                               │
                                   Arrow joins over Lance datasets
                                               │
                                               ▼
                                         LanceGraph  (memory_graph/)
                                         entity relationships
```

### Backend roles

```
LanceDB      — Fact memory
               episodes + extracted SPO facts, each with an embedding vector
               ANN search + Arrow predicate filter in one query

LanceGraph   — Relationship memory
               entity graph stored as Arrow/Lance datasets
               traversal via Arrow joins, no separate graph process

DuckDB       — Lifecycle brain
               owns TTL state, embedding cache, consolidation jobs
               holds no primary facts — reads Lance files as Parquet
               federates across Iceberg catalogs for cloud deployments

In-process   — Working memory
               current session ring buffer, no storage dependency
```

### Lakehouse model

LanceDB and LanceGraph use the Lance storage format. The same data reads identically from local filesystem or object storage — deployment is configuration, not code:

```
edge:   ./data/memory/facts.lance
cloud:  s3://bucket/memory/facts.lance   ← same API, same schema
```

DuckDB federates across both via `read_parquet()` and `iceberg_scan()`.

**Edge ↔ cloud sync is intentionally out of scope.** membrane's job is judgment support at inference time — providing similar past cases, trend and frequency signals, and entity relationships for local decision-making. Knowledge aggregation for learning (collecting episodes from many edge nodes, improving embeddings, updating the graph at scale) is post-processing work that belongs in a separate pipeline. membrane produces Arrow/Lance data that any sync layer can consume.

---

## Memory tiers

| Tier | Role | Backend |
|------|------|---------|
| `WorkingMemory` | Current session, lives in the context window | In-process ring buffer |
| `FactMemory` | Past episodes and extracted facts, vector-searchable | LanceDB |
| `RelationshipMemory` | Entity graph — who is connected to what | LanceGraph |
| `LifecycleBrain` | TTL, cache, consolidation jobs | DuckDB |

---

## Usage sketch

```rust
use membrane::{MembraneEngine, MembraneConfig, Episode, Role};

let engine = MembraneEngine::open("./data/memory", MembraneConfig::default()).await?;

// Store a conversation turn
engine.store_episode(Episode {
    session_id: "session-42".into(),
    role: Role::User,
    content: "What were the anomalies detected last Tuesday?".into(),
    ..Default::default()
}).await?;

// Assemble context for the next model call (token budget: 4096)
let ctx = engine.assemble_context("anomalies last week", 4096).await?;
println!("{}", ctx.prompt_prefix);  // ready to prepend to your prompt
```

### Configuration

membrane ships opinionated defaults that work for general conversational agents. Override what you need:

```rust
use membrane::{MembraneConfig, RetrievalConfig, ScoringWeights, WorkingMemoryConfig};

// Constrained edge device (≤2 GB RAM)
let config = MembraneConfig {
    working_memory: WorkingMemoryConfig {
        max_turns: 5,
        ..Default::default()
    },
    retrieval: RetrievalConfig {
        limit: 3,
        weights: ScoringWeights { relevance: 0.7, recency: 0.2, diversity: 0.1 },
        ..Default::default()
    },
    ..Default::default()
};
```

### Hooks for application-specific behavior

```rust
// Control what gets stored
engine.set_worthy_filter(Box::new(MyWorthyFilter));

// Control which tier an episode goes to
engine.set_tier_router(Box::new(MyTierRouter));

// Control how retrieved memories are formatted into the prompt
engine.set_context_formatter(Box::new(MyContextFormatter));
```

---

## What membrane automates vs. what you design

**membrane always handles:**
- Embedding generation and caching (blake3-keyed, model-id-aware)
- Token budget enforcement (the assembled context never exceeds your limit)
- Content-hash deduplication
- TTL expiry
- Retrieval scoring and diversity

**You configure:**
- Scoring weights (Relevance / Recency / Diversity)
- Result limits and working memory window size
- Entity taxonomy (`EntityKind::Custom("vessel")`)

**You implement via hooks:**
- What is worth storing (`WorthyFilter`)
- Which tier to route to (`TierRouter`)
- How to build retrieval queries (`QueryBuilder`)
- How to format context into a prompt (`ContextFormatter`)

**Out of scope (you design at the application layer):**
- Edge ↔ cloud sync strategy
- Contradiction detection rules
- Consolidation business logic

---

## Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `async` | on | tokio async API |
| `store-lance` | on | LanceDB + LanceGraph backends |
| `store-duck` | on | DuckDB lifecycle brain |
| `embedding-local` | off | mistral.rs local inference |
| `audit-bridge` | off | tamper-evident writes via edgesentry-audit |
| `pyo3-bindings` | off | Python bindings |

Minimal footprint (no storage deps, in-memory only):

```toml
membrane = { version = "0.1", default-features = false }
```

---

## Stack

| Component | Technology |
|-----------|-----------|
| IO boundary | Apache Arrow (`arrow-array`, `arrow-schema`) |
| Fact storage + ANN search | LanceDB (Lance columnar format) |
| Entity graph | LanceGraph |
| Lifecycle orchestration | DuckDB |
| Local embeddings + inference | mistral.rs |
| Serialization | serde + postcard |
| Content addressing | blake3 |
| Async runtime | tokio |

---

## Relationship to edgesentry-rs

membrane is a standalone crate. It integrates optionally with the edgesentry ecosystem:

- **edgesentry-audit** (`audit-bridge` feature): every memory write emits a blake3-signed `AuditRecord`, making memory history tamper-evident
- **edgesentry-inspect**: future integration — `ContextAssembler` prepends relevant past scan findings before a sensor frame is processed
- **arktrace** (`pyo3-bindings` feature): Python bindings expose `MembraneSession` for use in arktrace's LLM chat routes

---

## OSS reference designs

| System | What membrane borrows |
|--------|-----------------------|
| MemGPT / Letta | 3-tier hierarchy; overflow → summarize |
| Zep / Graphiti | Bi-temporal facts (`valid_from` / `valid_until`) |
| mem0 | Automatic fact extraction pipeline |
| Neo4j Memory MCP | POLE+O entity model |
| LlamaIndex ChatMemoryBuffer | Token budget enforcement |
| Vertex AI Memory Bank | Async background consolidation |

Key difference from MemGPT: MemGPT asks the *model* to decide when to read/write memory. membrane makes those decisions *outside* the model — essential for small models that cannot reliably self-manage.

---

## Status

Early development. See [plan.md](plan.md) for detailed design decisions.

| Phase | Scope | Status |
|-------|-------|--------|
| Phase 1 | WorkingMemory + FactStore (LanceDB) + EmbeddingEngine + context assembly | In design |
| Phase 2 | LifecycleStore (DuckDB) + RelationshipStore (LanceGraph) + entity extraction + TTL | Planned |
| Phase 3 | Consolidation · summarization · AuditBridge · PyO3 bindings | Planned |

---

## License

MIT OR Apache-2.0
