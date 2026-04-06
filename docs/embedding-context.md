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
  Phase 2: persisted to DuckDB embedding_cache table
  Key: blake3(text) → [u8; 32]
  Invalidation: cache entries are keyed by model_id; changing models auto-invalidates
```

Fallback: `NoopEmbeddingEngine` returns zero vectors — enables CI and tests without model files.

---

## Context Assembly

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
