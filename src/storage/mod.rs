//! Storage traits and in-memory test implementations.
//!
//! All storage backends communicate via `arrow_array::RecordBatch` — never raw
//! domain structs. This enables zero-copy between backends and composable pipelines.
//!
//! - [`FactStore`] — fact memory backend (implemented by `LanceFactStore` in M1.6)
//! - [`EmbeddingEngine`] — embedding computation (implemented by `MistralEmbeddingEngine` in M1.8)
//! - [`InMemoryFactStore`] — test stub; HashMap-backed, no external deps
//! - [`NoopEmbeddingEngine`] is in `crate::embedding`

pub mod memory;

pub use memory::InMemoryFactStore;

use arrow_array::{FixedSizeListArray, RecordBatch};
use std::future::Future;

use crate::types::MemoryId;

// ---------------------------------------------------------------------------
// FactStore
// ---------------------------------------------------------------------------

/// Fact memory backend. Inserts and ANN-searches Arrow RecordBatches.
///
/// Implemented by `LanceFactStore` (feature = `store-lance`) and
/// `InMemoryFactStore` (always available, for tests).
///
/// Uses RPITIT — no `async_trait` macro required (stable since Rust 1.75).
pub trait FactStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Insert a RecordBatch of facts.
    ///
    /// Schema must match `facts.lance` (see `crate::arrow::facts_schema`).
    /// Operations are upsert-semantics: re-inserting the same id overwrites.
    fn insert(&self, batch: RecordBatch)
        -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// ANN vector search. Returns up to `limit` matching rows as a RecordBatch.
    ///
    /// `filter` is an Arrow compute expression string applied after the ANN search
    /// (e.g. `"fact_kind = 'episode' AND ttl_expires_ms > 1234"`).
    /// The in-memory stub ignores `filter`.
    fn search(
        &self,
        query_vector: &[f32],
        limit: usize,
        filter: Option<&str>,
    ) -> impl Future<Output = Result<RecordBatch, Self::Error>> + Send;

    /// Batch-delete by `id` column values. Missing ids are silently ignored.
    fn delete(&self, ids: &[MemoryId])
        -> impl Future<Output = Result<(), Self::Error>> + Send;
}

// ---------------------------------------------------------------------------
// EmbeddingEngine
// ---------------------------------------------------------------------------

/// Embedding computation engine.
///
/// Implemented by `MistralEmbeddingEngine` (feature = `embedding-local`) and
/// `NoopEmbeddingEngine` (always available, returns zero vectors for CI).
///
/// Output uses Arrow `FixedSizeListArray` for zero-copy insertion into LanceDB.
pub trait EmbeddingEngine: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Embed a single text string. Returns a `Vec<f32>` for convenience.
    fn embed(&self, text: &str)
        -> impl Future<Output = Result<Vec<f32>, Self::Error>> + Send;

    /// Batch embed. Returns a `FixedSizeListArray` for zero-copy Lance insert.
    fn embed_batch(&self, texts: &[&str])
        -> impl Future<Output = Result<FixedSizeListArray, Self::Error>> + Send;

    /// Number of dimensions in the embedding output.
    fn dimensions(&self) -> usize;
}
