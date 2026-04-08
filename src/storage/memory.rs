//! In-memory `FactStore` test stub — no external dependencies.
//!
//! `InMemoryFactStore` stores one 1-row `RecordBatch` per episode, keyed by
//! `MemoryId`. Search performs a linear dot-product scan (assumes unit-normalized
//! vectors from the embedding model). Suitable for unit tests and CI only —
//! not for production use.

use std::collections::HashMap;
use std::sync::Mutex;

use arrow_array::{Array, BinaryArray, FixedSizeListArray, Float32Array, RecordBatch};
use arrow_schema::SchemaRef;
use arrow_select::concat::concat_batches;

use crate::error::MembridError;
use crate::types::MemoryId;

use super::FactStore;

// ---------------------------------------------------------------------------
// InMemoryFactStore
// ---------------------------------------------------------------------------

/// HashMap-backed `FactStore` for unit tests and CI.
///
/// No LanceDB or external dependencies required.
///
/// # Usage
/// ```rust,ignore
/// let store = InMemoryFactStore::new(facts_schema());
/// store.insert(batch).await?;
/// let results = store.search(&query_vec, 5, None).await?;
/// ```
pub struct InMemoryFactStore {
    /// One 1-row RecordBatch per stored fact, keyed by MemoryId.
    rows: Mutex<HashMap<MemoryId, RecordBatch>>,
    schema: SchemaRef,
}

impl InMemoryFactStore {
    pub fn new(schema: SchemaRef) -> Self {
        Self {
            rows: Mutex::new(HashMap::new()),
            schema,
        }
    }

    pub fn len(&self) -> usize {
        self.rows.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.lock().unwrap().is_empty()
    }
}

// ---------------------------------------------------------------------------
// FactStore impl
// ---------------------------------------------------------------------------

impl FactStore for InMemoryFactStore {
    type Error = MembridError;

    fn insert(&self, batch: RecordBatch) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let mut guard = self.rows.lock().unwrap();
        for row in 0..batch.num_rows() {
            if let Some(id) = extract_id(&batch, row) {
                guard.insert(id, batch.slice(row, 1));
            }
        }
        std::future::ready(Ok(()))
    }

    fn search(
        &self,
        query_vector: &[f32],
        limit: usize,
        _filter: Option<&str>,
        // Note: filter is ignored in the in-memory stub.
        // LanceFactStore (M1.6) applies it as an Arrow predicate on facts.lance.
    ) -> impl std::future::Future<Output = Result<RecordBatch, Self::Error>> + Send {
        let query = query_vector.to_vec();
        let schema = self.schema.clone();

        let guard = self.rows.lock().unwrap();
        let mut scored: Vec<(f32, RecordBatch)> = guard
            .values()
            .filter_map(|row_batch| {
                let vec = extract_vector(row_batch, 0)?;
                let score = dot_product(&query, &vec);
                Some((score, row_batch.clone()))
            })
            .collect();
        drop(guard);

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        let result = if scored.is_empty() {
            Ok(RecordBatch::new_empty(schema))
        } else {
            let batches: Vec<RecordBatch> = scored.into_iter().map(|(_, b)| b).collect();
            concat_batches(&schema, &batches).map_err(MembridError::Arrow)
        };

        std::future::ready(result)
    }

    fn delete(&self, ids: &[MemoryId]) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let mut guard = self.rows.lock().unwrap();
        for id in ids {
            guard.remove(id);
        }
        std::future::ready(Ok(()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the 16-byte MemoryId from the `id` (Binary) column at `row`.
fn extract_id(batch: &RecordBatch, row: usize) -> Option<MemoryId> {
    let col = batch
        .column_by_name("id")?
        .as_any()
        .downcast_ref::<BinaryArray>()?;
    let bytes = col.value(row);
    if bytes.len() < 16 {
        return None;
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&bytes[..16]);
    Some(id)
}

/// Extract the embedding vector from the `vector` (FixedSizeList<Float32>) column at `row`.
/// Returns `None` if the column is absent or the value is null (embedding not yet computed).
fn extract_vector(batch: &RecordBatch, row: usize) -> Option<Vec<f32>> {
    let col = batch
        .column_by_name("vector")?
        .as_any()
        .downcast_ref::<FixedSizeListArray>()?;
    if col.is_null(row) {
        return None;
    }
    let values = col.value(row);
    let f32_arr = values.as_any().downcast_ref::<Float32Array>()?;
    Some(f32_arr.values().to_vec())
}

/// Dot product of two equal-length slices.
/// For unit-normalized embedding vectors this equals cosine similarity.
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        arrow::{convert::episodes_to_record_batch, facts_schema},
        types::{Episode, Role},
    };

    fn make_episode(content: &str, vec: Vec<f32>) -> Episode {
        let mut ep = Episode::new("test", Role::User, content);
        ep.embedding = Some(vec);
        ep
    }

    #[tokio::test]
    async fn insert_and_len() {
        let store = InMemoryFactStore::new(facts_schema());
        assert!(store.is_empty());

        let ep = make_episode("hello", vec![1.0; 768]);
        let batch = episodes_to_record_batch(&[ep], 768).unwrap();
        store.insert(batch).await.unwrap();

        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn search_returns_closest() {
        let store = InMemoryFactStore::new(facts_schema());

        // Insert two episodes with orthogonal unit vectors.
        let mut v1 = vec![0.0f32; 768];
        v1[0] = 1.0;
        let mut v2 = vec![0.0f32; 768];
        v2[1] = 1.0;

        let ep1 = make_episode("episode one", v1.clone());
        let ep2 = make_episode("episode two", v2.clone());
        store.insert(episodes_to_record_batch(&[ep1], 768).unwrap()).await.unwrap();
        store.insert(episodes_to_record_batch(&[ep2], 768).unwrap()).await.unwrap();

        // Query aligned with v1 — ep1 should score higher.
        let result = store.search(&v1, 2, None).await.unwrap();
        assert_eq!(result.num_rows(), 2);

        let content_col = result
            .column_by_name("content")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow_array::StringArray>()
            .unwrap();
        assert_eq!(content_col.value(0), "episode one");
    }

    #[tokio::test]
    async fn delete_removes_episode() {
        let store = InMemoryFactStore::new(facts_schema());

        let ep = make_episode("to be deleted", vec![1.0; 768]);
        let id = ep.id;
        let batch = episodes_to_record_batch(&[ep], 768).unwrap();
        store.insert(batch).await.unwrap();
        assert_eq!(store.len(), 1);

        store.delete(&[id]).await.unwrap();
        assert!(store.is_empty());
    }

    #[tokio::test]
    async fn upsert_overwrites_same_id() {
        let store = InMemoryFactStore::new(facts_schema());

        let ep = make_episode("original", vec![1.0; 768]);
        let batch = episodes_to_record_batch(&[ep.clone()], 768).unwrap();
        store.insert(batch).await.unwrap();

        // Re-insert same id — store should still have 1 row.
        let batch2 = episodes_to_record_batch(&[ep], 768).unwrap();
        store.insert(batch2).await.unwrap();

        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn search_empty_store_returns_empty_batch() {
        let store = InMemoryFactStore::new(facts_schema());
        let result = store.search(&[0.0f32; 768], 5, None).await.unwrap();
        assert_eq!(result.num_rows(), 0);
    }
}
