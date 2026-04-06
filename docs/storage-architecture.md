# Storage Architecture

## Arrow as IO Common Language

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

## Backend Roles

| Backend | Role | Why |
|---------|------|-----|
| **LanceDB** | **Fact memory** — stores episodes and extracted facts with their vector embeddings | Lance format is Arrow/Parquet columnar; ANN search + payload filter in one query; same API for local FS and object storage |
| **LanceGraph** | **Relationship memory** — entity graph stored as Arrow/Lance datasets | Graph edges as Lance tables; same lakehouse model as LanceDB; traversal via Arrow joins |
| **DuckDB** | **Lifecycle brain** — orchestrates TTL, consolidation state, embedding cache, session index; federates across lakehouses | Speaks Arrow natively; can `read_parquet()` Lance files directly; Iceberg catalog support for cross-store queries |
| **In-process** | **Working memory** — current session ring buffer | Zero latency; lives entirely in the context window; no Arrow overhead needed |

## Lakehouse Deployment Model

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

## Edge ↔ Cloud Sync — Intentionally Out of Scope

This is a deliberate scope decision, not an omission.

membrane's role is to provide knowledge that assists judgment: similar past cases, trend/frequency signals, and entity relationships. In edge AI deployments, anything on the critical decision path must be fully local — a cloud round-trip is not acceptable for real-time inference. membrane therefore operates entirely within the edge process.

Learning and improvement — aggregating episodes and extracted facts into object storage for offline analysis or model fine-tuning — is post-processing work that happens outside the judgment loop. That belongs in a separate layer, not in a memory management library.

Possible sync strategies (shared object storage, log-based replication, Iceberg table versioning) are application-level decisions. membrane imposes no constraint and is compatible with any of them via the lakehouse model: the same Lance/Parquet files readable locally are readable from object storage without code changes.
