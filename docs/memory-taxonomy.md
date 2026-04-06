# Memory Taxonomy and Storage Schema

## Memory Tiers

| Tier | Role | Backend | Retrieval |
|------|------|---------|-----------|
| **WorkingMemory** | Current session conversation (lives in context window) | In-process ring buffer | Direct access |
| **FactMemory** | Episodes and extracted facts with vector embeddings | LanceDB | ANN vector search + Arrow payload filter |
| **RelationshipMemory** | Entity graph — who/what is connected to whom/what | LanceGraph | Graph traversal over Arrow/Lance datasets |
| **LifecycleBrain** | TTL state, consolidation progress, embedding cache, session index | DuckDB | SQL; federates across LanceDB/Iceberg |

**Mapping to OSS reference designs:**

- MemGPT's in-context / recall / archival → WorkingMemory / FactMemory / LifecycleBrain
- Zep's bi-temporal KG → FactMemory holds `valid_from` / `valid_until` on fact payloads
- Neo4j Memory MCP's graph → RelationshipMemory via LanceGraph
- Vertex AI Memory Bank's async background processing → LifecycleBrain (DuckDB) drives consolidation triggers

---

## Storage Schema

### LanceDB — Fact Memory (`facts.lance`)

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

### LanceGraph — Relationship Memory (`memory_graph/`)

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

### DuckDB — Lifecycle Brain

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
