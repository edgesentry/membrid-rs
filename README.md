# membrane

**LLM memory management for edge AI with small models.**

membrane gives small language models (Gemma 4 2B/4B) better recall without making them bigger. It manages what goes *into* the model — not what the model *is*.

The model's job is reasoning. membrane handles everything else: selecting relevant context, resolving entities, enforcing token budgets, and keeping memory consistent.

See [plan.md](plan.md) for full design rationale, storage schemas, trait definitions, and implementation phases.

---

## Quick start

```rust
use membrane::{MembraneEngine, MembraneConfig, Episode, Role};

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
| `store-lance` | on | LanceDB (fact memory) + LanceGraph (relationship memory) |
| `store-duck` | on | DuckDB lifecycle brain |
| `embedding-local` | off | mistral.rs local inference |
| `audit-bridge` | off | tamper-evident writes via edgesentry-audit |
| `pyo3-bindings` | off | Python bindings for arktrace |

No storage dependencies (in-memory only):

```toml
membrane = { version = "0.1", default-features = false }
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
