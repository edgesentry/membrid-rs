# Storage Architecture

## Vector-First Storage Hierarchy

LLM memory is fundamentally multi-dimensional vectors — semantic retrieval via nearest-neighbor search is the primary operation in every use case. The storage stack reflects this:

```
┌─────────────────────────────────────────────────────────┐
│  Tier 1 — PRIMARY (always active)                       │
│  LanceDB  facts.lance + embedding_cache.lance           │
│  Vector ANN · TTL filter · embedding cache              │
├─────────────────────────────────────────────────────────┤
│  Tier 2 — ADDITIVE (when entity relationships matter)   │
│  LanceGraph  entities.lance + edge datasets             │
│  Multi-hop traversal · knowledge graph                  │
├─────────────────────────────────────────────────────────┤
│  Tier 3 — ANALYTICS (when SQL federation needed)        │
│  DuckDB                                                 │
│  Cross-session aggregation · Iceberg · BI integration   │
└─────────────────────────────────────────────────────────┘
```

**Design rule:** each tier is independently deployable. A system with only Tier 1 is fully functional. Tier 2 and Tier 3 are opt-in extensions that add capability without changing the Tier 1 API.

This hierarchy maps cleanly onto the implementation phases: Phase 1 = Tier 1, Phase 2 = Tier 2, Phase 3 = Tier 3.

---

## Arrow as IO Common Language

Apache Arrow is the lingua franca that connects all backends with zero-copy data transfer:

```
                    ┌─────────────────────────────────────────────┐
                    │             Apache Arrow boundary            │
                    │                                             │
  Mistral.rs  ──── RecordBatch ────►  LanceDB (Tier 1)           │
  embedding                          facts.lance                  │
  pipeline                           embedding_cache.lance        │
                                     (Arrow-native on disk)       │
                                              │                   │
                    RecordBatch (Arrow IPC)   │                   │
                    ◄─────────────────────────┘                   │
                    │                                             │
                    ▼                                             │
             LanceGraph (Tier 2)                                  │
             entities.lance + edge datasets                       │
                    │                                             │
                    │  read_parquet() — Phase 3 only              │
                    ▼                                             │
             DuckDB (Tier 3)                                      │
             SQL federation · Iceberg · cross-session analytics   │
                    └─────────────────────────────────────────────┘
```

Every API boundary in membrane passes `arrow_array::RecordBatch`, not serialized bytes. This means:
- No copy/deserialize overhead between LanceDB → DuckDB
- Embedding output from mistral.rs is `FixedSizeListArray` — directly inserted into Lance tables
- DuckDB reads LanceDB's `.lance` files as Parquet without a separate export step (Phase 3)

---

## Backend Roles

| Tier | Backend | Role | Phase |
|------|---------|------|-------|
| **1** | **LanceDB** | **Primary** — facts, episodes, embeddings, TTL, embedding cache | Phase 1+ |
| **2** | **LanceGraph** | **Additive** — entity graph, multi-hop traversal, knowledge graph | Phase 2+ |
| **3** | **DuckDB** | **Analytics** — SQL federation, Iceberg, cross-session consolidation | Phase 3 only |
| — | **In-process** | **Working memory** — current session ring buffer | Phase 1+ |

### Tier 1: LanceDB (Primary)

LanceDB serves as the single source of truth for all memory state through Phase 2:

- `facts.lance` — episodes and extracted facts with vector embeddings
- `embedding_cache.lance` — blake3(text) → vector cache; Arrow-native, no DuckDB dependency

**TTL enforcement** is Lance-native: `ttl_expires_ms` is a reserved column in `facts.lance`. Expiry is applied as a metadata filter (`where ttl_expires_ms IS NULL OR ttl_expires_ms > now_ms()`) at query time and cleaned up in batch via Lance's versioned delete. No secondary lifecycle store required.

This keeps write atomicity simple: `store_episode()` is a single write to one Lance dataset.

### Tier 2: LanceGraph (Additive)

LanceGraph is a logical graph built on top of Lance datasets — no sync with LanceDB required. It adds value when the retrieval question is relational ("what entities are connected to X?") rather than semantic ("what is similar to X?").

Write ordering rule: always write entity node before any edge referencing it (entity-first). This eliminates dangling edge windows without requiring cross-table transactions.

### Tier 3: DuckDB (Optional Analytics Layer)

DuckDB is **not active until Phase 3**. It is introduced only when use cases genuinely require SQL:

- Cross-session aggregation (GROUP BY, window functions over lifecycle data)
- Iceberg catalog federation (`iceberg_scan()`)
- BI tool integration
- Large-scale consolidation job scheduling across many sessions

DuckDB reads Lance files as Parquet — it is a query layer over Tier 1/2 data, not a lifecycle manager. It never participates in the `store_episode()` write path.

---

## facts.lance Schema (Reserved Columns)

```
id               FixedSizeBinary(16)   blake3(content + timestamp_ms)[:16]
session_id       Utf8
timestamp_ms     UInt64
role             Utf8                  "user" | "assistant" | "system"
content          Utf8
embedding        FixedSizeList<f32>    model-dimension (e.g. 768 for nomic-embed-text)
fact_kind        Utf8                  "episode" | "fact" | "summary"
ttl_expires_ms   UInt64 (nullable)     null = never expires
source_ids       List<FixedSizeBinary(16)>   for facts derived from episodes
```

`ttl_expires_ms` is the sole mechanism for lifecycle management in Tier 1. No external store required.

---

## embedding_cache.lance Schema

```
key        FixedSizeBinary(32)    blake3(text)
model_id   Utf8                   invalidated on model change
vector     FixedSizeList<f32>     cached embedding
```

Replaces the Phase 2 DuckDB `embedding_cache` table from the original design. Cache hits avoid re-embedding on edge hardware where inference is expensive.

---

## Lakehouse Deployment Model

LanceDB and LanceGraph use the same Lance storage format regardless of where data lives:

```
Local (edge):                          Object storage (cloud):
  ./data/memory/facts.lance     ←→       s3://bucket/memory/facts.lance
  ./data/memory/embedding_cache.lance    s3://bucket/memory/embedding_cache.lance
  ./data/memory/graph/                   s3://bucket/memory/graph/

Same Rust API. Same schema. Deployment is configuration, not code.
```

DuckDB (Phase 3) federates across these locations via:
- `read_parquet('s3://...')` / `read_parquet('./data/...')` — Lance files are valid Parquet
- Iceberg catalog queries for versioned, ACID-compliant lakehouse access

---

## Edge ↔ Cloud Sync — Intentionally Out of Scope

This is a deliberate scope decision, not an omission.

membrane's role is to provide knowledge that assists judgment: similar past cases, trend/frequency signals, and entity relationships. In edge AI deployments, anything on the critical decision path must be fully local — a cloud round-trip is not acceptable for real-time inference. membrane therefore operates entirely within the edge process.

Learning and improvement — aggregating episodes and extracted facts into object storage for offline analysis or model fine-tuning — is post-processing work that happens outside the judgment loop. That belongs in a separate layer, not in a memory management library.

Possible sync strategies (shared object storage, log-based replication, Iceberg table versioning) are application-level decisions. membrane imposes no constraint and is compatible with any of them via the lakehouse model.

---

## Relationship to Issue #22 (Data Consistency)

Adopting the Vector-First hierarchy eliminates the majority of cross-store inconsistency scenarios identified in #22:

| Original scenario | Status under Vector-First |
|---|---|
| 1. store_episode() LanceDB↔DuckDB desync | **Eliminated** — single Lance write, no DuckDB in write path |
| 2. forget() partial execution | **Simplified** — TTL filter is a Lance column predicate; batch delete is one Lance operation |
| 3. consolidate() checkpoint gap | **Deferred to Phase 3** — only relevant when DuckDB consolidation jobs exist |
| 4. LanceGraph entity/edge atomicity | **Remains** — mitigated by M4 (entity-first ordering) |
| 5. Entity dedup merge | **Remains** — mitigated by M4 (tombstone soft-delete) |
| 6. embedding_cache desync | **Eliminated** — cache is embedding_cache.lance (same engine as facts.lance) |

Mitigations M1 (Write-intent WAL) and M3 (startup consistency scan across Lance↔DuckDB) are deferred to Phase 3. M2 (idempotent ops) and M4 (entity-first ordering) remain P0 for Phase 1 and Phase 2 respectively.
