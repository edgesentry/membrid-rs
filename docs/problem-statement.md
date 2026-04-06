# Problem Statement

## Core Challenge

Small LLMs (Gemma 4 2B/4B) cannot be made performant by scaling up the model — resource constraints on edge devices make that path closed. membrane's role is to **compensate on the outside of the model**, raising cost-performance by improving what goes *in*, not what the model *is*.

Three strategies drive the design:

**Information Diet** — Rather than stuffing all available context into the prompt, membrane selects only what is needed right now. Small models degrade sharply as context grows (Lost-in-the-Middle effect). Less is more.

**Externalized Decision-Making** — Entity resolution, fact consistency checks, deduplication, and structured retrieval are handled by membrane, not the LLM. The model's job is reasoning; everything else is delegated out.

**Consistency Enforcement** — Contradictory memories, stale facts, and duplicate episodes are resolved *before* being handed to the model, so the model never has to choose between conflicting information.

## Library Scope

membrane is a **general-purpose library** — it does not target a fixed use case. Because of this, the boundary between what the library automates and what the user designs must be explicit. See [automation-guidelines.md](automation-guidelines.md) for the full guideline.

## Existing Rust Ecosystem Survey

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

## Reference Implementations

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

## Technology Stack

- **Target models**: Gemma 4 2B / 4B (quantized INT8/FP16, 2–8 GB RAM)
- **Deployment**: edge-first; cloud deployment uses the same codebase — sync strategy is out of scope
- **Integration**: edgesentry-rs (Rust), arktrace (Python)
- **IO common language**: **Apache Arrow** — all data crossing backend boundaries is `RecordBatch`; zero-copy between LanceDB, DuckDB, and the embedding pipeline
- **Fact memory**: **LanceDB** — stores facts and episodes as Arrow/Lance columnar data with vector embeddings; lakehouse model (local path or object storage URI, same API)
- **Relationship memory**: **LanceGraph** — entity graph stored as Arrow/Lance datasets; lakehouse model
- **Lifecycle brain**: **DuckDB** — orchestrates TTL, consolidation state, embedding cache, session index; federates across lakehouses via Arrow, Parquet, and Iceberg
- **Embedding / inference**: **Mistral.rs** (local quantized models; outputs Arrow arrays)
