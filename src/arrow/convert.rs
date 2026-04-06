//! Conversions between domain types and Arrow RecordBatch.
//!
//! All storage backends receive and return RecordBatch — never raw domain structs.
//! This module owns the single canonical mapping between the two.

use crate::{
    arrow::facts_schema_with_dims,
    error::{MembridError, Result},
    types::{Episode, MemoryId, MemoryTier, RetrievedMemory, Score},
};
use arrow_array::{
    BinaryArray, FixedSizeListArray, Float32Array, Int64Array, ListArray, RecordBatch,
    StringArray,
};
use arrow_schema::Schema;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Episode → RecordBatch
// ---------------------------------------------------------------------------

/// Convert a slice of episodes into a `RecordBatch` matching `facts_schema`.
///
/// `embedding_dims` must match the schema used for the target LanceDB table.
/// Episodes with `embedding = None` will have a null vector column entry.
pub fn episodes_to_record_batch(
    episodes: &[Episode],
    embedding_dims: i32,
) -> Result<RecordBatch> {
    let schema = facts_schema_with_dims(embedding_dims);
    let n = episodes.len();

    // id: Binary
    let ids: BinaryArray = episodes.iter().map(|e| Some(e.id.as_ref())).collect();

    // vector: FixedSizeList<Float32> with validity bitmap for null embeddings
    let vector = build_fixed_size_list_with_validity(episodes, embedding_dims)?;

    let fact_kind: StringArray = episodes.iter().map(|_| Some("episode")).collect();
    let session_id: StringArray = episodes.iter().map(|e| Some(e.session_id.as_str())).collect();
    let role: StringArray = episodes.iter().map(|e| Some(e.role.as_str())).collect();
    let content: StringArray = episodes.iter().map(|e| Some(e.content.as_str())).collect();
    let preview: StringArray = episodes.iter().map(|e| Some(e.preview())).collect();

    // SPO fields — null for episodes
    let confidence: Float32Array = (0..n).map(|_| None::<f32>).collect();

    let valid_from: Int64Array = (0..n).map(|_| None::<i64>).collect();
    let valid_until: Int64Array = (0..n).map(|_| None::<i64>).collect();
    let timestamp_ms: Int64Array = episodes
        .iter()
        .map(|e| Some(e.timestamp_ms as i64))
        .collect();

    // topic_tags: List<Utf8>
    let topic_tags = build_string_list(episodes.iter().map(|e| &e.metadata.topic_tags))?;
    // entity_ids: List<Utf8>
    let entity_ids = build_string_list(episodes.iter().map(|e| &e.metadata.entity_ids))?;

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(ids),
            Arc::new(vector),
            Arc::new(fact_kind),
            Arc::new(session_id),
            Arc::new(role),
            Arc::new(content),
            Arc::new(preview),
            Arc::new(arrow_array::StringArray::from(vec![None::<&str>; n])), // subject_id
            Arc::new(arrow_array::StringArray::from(vec![None::<&str>; n])), // predicate
            Arc::new(arrow_array::StringArray::from(vec![None::<&str>; n])), // object_id
            Arc::new(arrow_array::StringArray::from(vec![None::<&str>; n])), // object_value
            Arc::new(confidence),
            Arc::new(valid_from),
            Arc::new(valid_until),
            Arc::new(timestamp_ms),
            Arc::new(topic_tags),
            Arc::new(entity_ids),
        ],
    )
    .map_err(|e| MembridError::Arrow(e))?;

    Ok(batch)
}

// ---------------------------------------------------------------------------
// RecordBatch → RetrievedMemory
// ---------------------------------------------------------------------------

/// Extract `RetrievedMemory` entries from a search result `RecordBatch`.
///
/// `scores` are the ANN similarity scores returned alongside the batch.
pub fn record_batch_to_retrieved(
    batch: &RecordBatch,
    scores: &[Score],
    tier: MemoryTier,
) -> Result<Vec<RetrievedMemory>> {
    let n = batch.num_rows();
    if scores.len() != n {
        return Err(MembridError::other(format!(
            "scores length {}, batch rows {n}",
            scores.len()
        )));
    }

    let id_col = batch
        .column_by_name("id")
        .ok_or_else(|| MembridError::other("missing 'id' column"))?
        .as_any()
        .downcast_ref::<BinaryArray>()
        .ok_or_else(|| MembridError::other("'id' column is not Binary"))?;

    let content_col = batch
        .column_by_name("content")
        .ok_or_else(|| MembridError::other("missing 'content' column"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| MembridError::other("'content' column is not Utf8"))?;

    let timestamp_col = batch
        .column_by_name("timestamp_ms")
        .ok_or_else(|| MembridError::other("missing 'timestamp_ms' column"))?
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| MembridError::other("'timestamp_ms' column is not Int64"))?;

    let mut results = Vec::with_capacity(n);
    for i in 0..n {
        let raw_id = id_col.value(i);
        let mut id: MemoryId = [0u8; 16];
        let copy_len = raw_id.len().min(16);
        id[..copy_len].copy_from_slice(&raw_id[..copy_len]);

        results.push(RetrievedMemory {
            id,
            content: content_col.value(i).to_owned(),
            score: scores[i],
            tier: tier.clone(),
            timestamp_ms: timestamp_col.value(i) as u64,
            metadata: serde_json::Value::Null,
        });
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn build_fixed_size_list_with_validity(
    episodes: &[Episode],
    dims: i32,
) -> Result<FixedSizeListArray> {
    let dim = dims as usize;
    let mut flat: Vec<f32> = Vec::with_capacity(episodes.len() * dim);
    let mut valid_bits: Vec<bool> = Vec::with_capacity(episodes.len());
    for ep in episodes {
        match &ep.embedding {
            Some(v) => {
                if v.len() != dim {
                    return Err(MembridError::other(format!(
                        "episode embedding has {} dims, expected {dim}",
                        v.len()
                    )));
                }
                flat.extend_from_slice(v);
                valid_bits.push(true);
            }
            None => {
                flat.extend(std::iter::repeat(0.0f32).take(dim));
                valid_bits.push(false);
            }
        }
    }
    let all_valid = valid_bits.iter().all(|&b| b);
    let null_buffer = if all_valid {
        None
    } else {
        Some(arrow_buffer::NullBuffer::from(valid_bits))
    };
    let values = Arc::new(Float32Array::from(flat));
    FixedSizeListArray::try_new(
        Arc::new(arrow_schema::Field::new("item", arrow_schema::DataType::Float32, true)),
        dims,
        values,
        null_buffer,
    )
    .map_err(MembridError::Arrow)
}

fn build_string_list<'a>(
    lists: impl Iterator<Item = &'a Vec<String>>,
) -> Result<ListArray> {
    use arrow_array::builder::{ListBuilder, StringBuilder};
    let mut builder = ListBuilder::new(StringBuilder::new());
    for list in lists {
        let values = builder.values();
        for s in list {
            values.append_value(s);
        }
        builder.append(true);
    }
    Ok(builder.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EpisodeMetadata, Role};

    fn make_episode(content: &str, dims: usize) -> Episode {
        let mut ep = Episode::new("test-session", Role::User, content);
        ep.embedding = Some(vec![0.1f32; dims]);
        ep
    }

    #[test]
    fn round_trip_episode_to_record_batch() {
        let ep = make_episode("hello world", 768);
        let batch = episodes_to_record_batch(&[ep.clone()], 768).unwrap();
        assert_eq!(batch.num_rows(), 1);

        let scores = vec![1.0f32];
        let retrieved = record_batch_to_retrieved(&batch, &scores, MemoryTier::Fact).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].content, "hello world");
        assert_eq!(retrieved[0].id, ep.id);
    }

    #[test]
    fn null_embedding_is_allowed() {
        let mut ep = Episode::new("test-session", Role::Assistant, "no embedding yet");
        ep.embedding = None;
        let batch = episodes_to_record_batch(&[ep], 768).unwrap();
        assert_eq!(batch.num_rows(), 1);
    }

    #[test]
    fn wrong_embedding_dims_returns_error() {
        let mut ep = Episode::new("test-session", Role::User, "bad dims");
        ep.embedding = Some(vec![0.0f32; 512]); // wrong: schema expects 768
        let result = episodes_to_record_batch(&[ep], 768);
        assert!(result.is_err());
    }
}
