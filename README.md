# membrane

**LLM memory management for edge AI with small models.**

membrane gives small language models (Gemma 4 2B/4B) better recall without making them bigger. It manages what goes *into* the model — not what the model *is*.

The model's job is reasoning. membrane handles everything else: selecting relevant context, resolving entities, enforcing token budgets, and keeping memory consistent.

See [plan.md](plan.md) for full design rationale, storage schemas, trait definitions, and implementation phases.

---

## The name

**membrid** is a portmanteau of two concepts that define the project's design philosophy.

### Membrane

The name reflects the core role of this library: *complementing* small models from the outside rather than making them larger.

- **Selective permeability** — just as a cell membrane filters what enters and exits, membrid selects only the most relevant context for the model to process. Information that isn't useful is kept out; what is useful is surfaced efficiently. This is the *information diet* principle.
- **Protective boundary** — on resource-constrained edge devices, the model cannot process everything. membrid acts as a boundary layer that shields the model's reasoning capacity from information overload.

### Hybrid

The `id` suffix captures the architectural and data-flow design.

- **Multi-backend unification** — membrid bridges three fundamentally different storage systems — LanceDB (vector), LanceGraph (graph), and DuckDB (SQL analytics) — treating them as a single coherent memory layer rather than three separate databases.
- **Zero-copy bridging** — Apache Arrow is the common language across all components. Data moves between backends without serialization or copying, enabling the kind of *hybrid data flow* that would otherwise require multiple format conversions.

Together: a membrane that manages information selection, built on a hybrid of storage primitives.

---

## Quick start

```rust
use membrid::{MembraneEngine, MembraneConfig, Episode, Role};

let engine = MembraneEngine::open("./data/memory", MembraneConfig::default()).await?;

engine.store_episode(Episode {
    session_id: "session-42".into(),
    role: Role::User,
    content: "What anomalies were detected last Tuesday?".into(),
    ..Default::default()
}).await?;

// Assemble context within a token budget, ready to prepend to your prompt
let ctx = engine.assemble_context("anomalies last week", 4096).await?;
println!("{}", ctx.prompt_prefix);
```

### Configuration

```rust
// Constrained edge device (≤2 GB RAM)
let config = MembraneConfig {
    working_memory: WorkingMemoryConfig { max_turns: 5, ..Default::default() },
    retrieval: RetrievalConfig {
        limit: 3,
        weights: ScoringWeights { relevance: 0.7, recency: 0.2, diversity: 0.1 },
        ..Default::default()
    },
    ..Default::default()
};
```

### Extension hooks

```rust
engine.set_worthy_filter(Box::new(MyFilter));       // what is worth storing
engine.set_tier_router(Box::new(MyRouter));          // which tier to route to
engine.set_context_formatter(Box::new(MyFormatter)); // how to format the prompt prefix
```

---

## Feature flags

| Flag | Default | Enables |
|------|---------|---------|
| `async` | on | tokio async API |
| `store-lance` | off | LanceDB (fact memory) + LanceGraph (relationship memory) |
| `store-duck` | off | DuckDB analytics layer (Phase 3) |
| `embedding-local` | off | mistral.rs local inference |
| `audit-bridge` | off | tamper-evident writes via edgesentry-audit |
| `pyo3-bindings` | off | Python bindings |

The default build includes only the async runtime, in-memory stores, and zero-copy traits — no native libraries required. Enable backends as needed:

```toml
# In-memory only (traits + WorkingMemory + InMemoryFactStore)
membrid-rs = "0.0.1"

# With LanceDB vector store
membrid-rs = { version = "0.0.1", features = ["store-lance"] }

# Full stack
membrid-rs = { version = "0.0.1", features = ["store-lance", "embedding-local"] }
```

---

## Status

Early development.

| Phase | Scope | Status |
|-------|-------|--------|
| 1 | WorkingMemory + FactStore (LanceDB) + EmbeddingEngine + context assembly | In design |
| 2 | LifecycleStore (DuckDB) + RelationshipStore (LanceGraph) + entity extraction + TTL | Planned |
| 3 | Consolidation · summarization · AuditBridge · PyO3 bindings | Planned |

---

## License

MIT OR Apache-2.0
