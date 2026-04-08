# Embedding Pipeline and Context Assembly

## Embedding Pipeline (Mistral.rs)

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
  Phase 1+: embedding_cache.lance      Arrow-native, same engine as facts.lance
  Key: blake3(text) → [u8; 32]
  Invalidation: cache entries are keyed by model_id; changing models auto-invalidates
```

`embedding_cache.lance` replaces the DuckDB-based `embedding_cache` table from earlier designs. Keeping the cache in Lance (same engine as `facts.lance`) eliminates the cross-store desync scenario — cache writes and fact writes fail or succeed within the same storage layer.

Fallback: `NoopEmbeddingEngine` returns zero vectors — enables CI and tests without model files.

---

## Context Assembly

Given query `q` and token budget `budget`:

```
1. WorkingMemory scan            → most recent N turns (always included; anchors recency)
2. FactStore ANN search          → embed(q) → LanceDB ANN on facts.lance
                                   filter: ttl_expires_ms IS NULL OR ttl_expires_ms > now_ms()
3. RelationshipStore traversal   → LanceGraph neighbors of entities found in step 2 (Phase 2+)
4. [Phase 3 only] DuckDB filter  → exclude ids under active consolidation
5. Merge results as RecordBatch
6. Score: Relevance × Recency × Diversity
7. Deduplicate by blake3(content)
8. Trim to fit within token_budget
9. Format as AssembledContext.prompt_prefix
```

Key changes from earlier design:
- **TTL filtering** (step 2) is now a Lance column predicate, not a DuckDB lifecycle query. No secondary store required in Phases 1 and 2.
- **LanceGraph traversal** (step 3) moves to immediately after ANN — it is the Phase 2 enhancement, not DuckDB.
- **DuckDB** (step 4) is Phase 3 only, used for consolidation-job-aware filtering, not basic lifecycle management.

All intermediate results are Arrow RecordBatches — no intermediate serialization.

**Token counting:**
- Phase 1: `content.len() / 4` character heuristic
- Phase 2: `tokenizers` crate loads Gemma 4 `tokenizer.json` (can be `include_bytes!` for embedded targets)
