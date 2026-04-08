//! Embedding engine implementations.
//!
//! - [`NoopEmbeddingEngine`] — always available; returns zero vectors. Enables
//!   CI and unit tests without model files.
//! - `MistralEmbeddingEngine` — feature = `embedding-local`; implemented in M1.8.

use std::sync::Arc;

use arrow_array::{FixedSizeListArray, Float32Array};
use arrow_schema::{DataType, Field};

use crate::{
    error::MembridError,
    storage::EmbeddingEngine,
};

// ---------------------------------------------------------------------------
// NoopEmbeddingEngine
// ---------------------------------------------------------------------------

/// Embedding engine that returns zero vectors.
///
/// Used in CI and unit tests when no model file is available. Every call
/// returns a vector of `dims` zeros, which is valid Arrow but semantically
/// meaningless for retrieval. Pair with `InMemoryFactStore` for in-process
/// tests that don't need meaningful ANN ranking.
pub struct NoopEmbeddingEngine {
    dims: usize,
}

impl NoopEmbeddingEngine {
    pub fn new(dims: usize) -> Self {
        assert!(dims > 0, "dims must be > 0");
        Self { dims }
    }
}

impl Default for NoopEmbeddingEngine {
    /// Returns a `NoopEmbeddingEngine` with 768 dimensions (nomic-embed default).
    fn default() -> Self {
        Self::new(768)
    }
}

impl EmbeddingEngine for NoopEmbeddingEngine {
    type Error = MembridError;

    fn embed(&self, _text: &str) -> impl std::future::Future<Output = Result<Vec<f32>, Self::Error>> + Send {
        let zeros = vec![0.0f32; self.dims];
        std::future::ready(Ok(zeros))
    }

    fn embed_batch(
        &self,
        texts: &[&str],
    ) -> impl std::future::Future<Output = Result<FixedSizeListArray, Self::Error>> + Send {
        let n = texts.len();
        let dims = self.dims;
        async move {
            let flat = vec![0.0f32; n * dims];
            let values = Arc::new(Float32Array::from(flat));
            FixedSizeListArray::try_new(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dims as i32,
                values,
                None,
            )
            .map_err(MembridError::Arrow)
        }
    }

    fn dimensions(&self) -> usize {
        self.dims
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Array;

    #[tokio::test]
    async fn embed_returns_zero_vector() {
        let engine = NoopEmbeddingEngine::new(4);
        let result = engine.embed("hello").await.unwrap();
        assert_eq!(result, vec![0.0f32; 4]);
        assert_eq!(engine.dimensions(), 4);
    }

    #[tokio::test]
    async fn embed_batch_shape() {
        let engine = NoopEmbeddingEngine::new(3);
        let result = engine.embed_batch(&["a", "b", "c"]).await.unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result.value_length(), 3);
    }

    #[tokio::test]
    async fn embed_batch_empty() {
        let engine = NoopEmbeddingEngine::default();
        let result = engine.embed_batch(&[]).await.unwrap();
        assert_eq!(result.len(), 0);
    }
}
