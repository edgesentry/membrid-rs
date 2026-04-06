# Automation Guidelines — Library Layer vs. User Layer

membrane is a general-purpose library with no fixed use case. This boundary is the most important design decision.

## What membrane guarantees

- Storage integrity (no duplicates, TTL enforcement, content-hash uniqueness)
- Context window is never exceeded (token budget strictly enforced)
- Retrieval diversity (Recency × Relevance × Diversity composite score)
- Entity consistency (same entity not stored under multiple IDs)
- Backend error isolation (one storage backend failing does not crash the engine)

## What membrane does not guarantee (user's responsibility)

- What qualifies as a memory-worthy event
- Which tier to route an episode to
- When to trigger retrieval
- How to construct retrieval queries

---

## Automation Level A — Always done by membrane (non-configurable)

| Function | Rationale |
|----------|-----------|
| Embedding cache (blake3 key) | Re-computing on edge hardware is expensive; savings are unconditional |
| Token budget enforcement | Exceeding the limit breaks the model; no exceptions |
| Content-hash deduplication | Storing identical text twice has no benefit |
| TTL expiry cleanup | Serving expired memories to the model is harmful |

## Automation Level B — Opinionated defaults, user-configurable

| Function | Default | How to change |
|----------|---------|---------------|
| Scoring weights | Relevance 0.6 / Recency 0.3 / Diversity 0.1 | `RetrievalConfig::weights` |
| Search result limit | 10 | `RetrievalConfig::limit` |
| WorkingMemory max turns | 20 | `WorkingMemoryConfig::max_turns` |
| Summarization threshold | 100 episodes | `ConsolidateConfig::summarize_threshold` |
| Entity confidence threshold | 0.7 | `EntityConfig::confidence_threshold` |

## Automation Level C — User-controlled via hooks

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

## Automation Level D — User designs entirely (membrane provides primitives only)

| Concern | What membrane provides |
|---------|----------------------|
| Entity taxonomy | `EntityKind::Custom(String)` for arbitrary extension |
| Fact predicate vocabulary | Free string ("works_at", "contradicts" — domain-defined) |
| Consolidation business logic | `ConsolidationPolicy` trait to implement |
| Contradiction detection rules | `ConsistencyChecker` trait to implement |

---

## Design Checklist for Small LLMs

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

## Recommended Configurations by Use Case

### Use Case A: Conversational Agent

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

### Use Case B: Document Q&A (RAG)

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

### Use Case C: Constrained Edge Device (≤2 GB RAM)

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
