//! Arrow schema definitions for membrid storage backends.
//!
//! This module is the single source of truth for all Lance table schemas.
//! All other modules import schemas from here — never redefine them elsewhere.

use arrow_schema::{DataType, Field, Fields, Schema};
use std::sync::Arc;

pub mod convert;

// ---------------------------------------------------------------------------
// facts.lance schema
// ---------------------------------------------------------------------------

/// Number of embedding dimensions. Matches nomic-embed-text-1.5 (768-dim).
/// Override via `facts_schema_with_dims()` for a different model.
pub const DEFAULT_EMBEDDING_DIMS: i32 = 768;

/// Arrow schema for `facts.lance`.
/// Used for both episodes (fact_kind = "episode") and extracted SPO facts (fact_kind = "extracted_fact").
pub fn facts_schema() -> Arc<Schema> {
    facts_schema_with_dims(DEFAULT_EMBEDDING_DIMS)
}

pub fn facts_schema_with_dims(dims: i32) -> Arc<Schema> {
    Arc::new(Schema::new(Fields::from(vec![
        // Primary key: first 16 bytes of blake3(content + timestamp_ms)
        Field::new("id", DataType::Binary, false),
        // Embedding vector — FixedSizeList so LanceDB can build ANN index
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), dims),
            true, // nullable: not yet embedded episodes are stored with null
        ),
        // "episode" | "extracted_fact"
        Field::new("fact_kind", DataType::Utf8, false),
        Field::new("session_id", DataType::Utf8, false),
        // "user" | "assistant" | "system" — null for extracted_fact
        Field::new("role", DataType::Utf8, true),
        Field::new("content", DataType::Utf8, false),
        // First 256 chars of content
        Field::new("preview", DataType::Utf8, false),
        // --- SPO fields (null for episodes) ---
        Field::new("subject_id", DataType::Utf8, true),
        Field::new("predicate", DataType::Utf8, true),
        Field::new("object_id", DataType::Utf8, true),
        Field::new("object_value", DataType::Utf8, true),
        Field::new("confidence", DataType::Float32, true),
        // Bi-temporal validity (ms since epoch)
        Field::new("valid_from", DataType::Int64, true),
        Field::new("valid_until", DataType::Int64, true),
        // Creation timestamp (ms since epoch)
        Field::new("timestamp_ms", DataType::Int64, false),
        // Lists
        Field::new(
            "topic_tags",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        Field::new(
            "entity_ids",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
    ])))
}

// ---------------------------------------------------------------------------
// entities.lance schema (LanceGraph)
// ---------------------------------------------------------------------------

pub fn entities_schema() -> Arc<Schema> {
    entities_schema_with_dims(DEFAULT_EMBEDDING_DIMS)
}

pub fn entities_schema_with_dims(dims: i32) -> Arc<Schema> {
    Arc::new(Schema::new(Fields::from(vec![
        // "person:alice"
        Field::new("id", DataType::Utf8, false),
        // POLE+O kind string
        Field::new("label", DataType::Utf8, false),
        Field::new("name", DataType::Utf8, false),
        Field::new(
            "aliases",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            false,
        ),
        // JSON-encoded attributes
        Field::new("attributes", DataType::Utf8, false),
        // Entity description embedding (for semantic entity search)
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), dims),
            true,
        ),
        Field::new("first_seen_ms", DataType::Int64, false),
        Field::new("last_seen_ms", DataType::Int64, false),
        Field::new("mention_count", DataType::UInt32, false),
    ])))
}

// ---------------------------------------------------------------------------
// edge dataset schema (LanceGraph)
// ---------------------------------------------------------------------------

/// Arrow schema for edge Lance datasets (RELATED_TO, CAUSES, CONTRADICTS, MENTIONED_WITH).
pub fn edge_schema() -> Arc<Schema> {
    Arc::new(Schema::new(Fields::from(vec![
        Field::new("src_id", DataType::Utf8, false),
        Field::new("dst_id", DataType::Utf8, false),
        Field::new("weight", DataType::Float32, false),
        // JSON-encoded extra properties
        Field::new("properties", DataType::Utf8, true),
    ])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_schema_has_required_fields() {
        let schema = facts_schema();
        let names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        for required in &["id", "vector", "fact_kind", "session_id", "content", "timestamp_ms"] {
            assert!(names.contains(required), "missing field: {required}");
        }
    }

    #[test]
    fn entities_schema_has_required_fields() {
        let schema = entities_schema();
        let names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        for required in &["id", "label", "name", "aliases"] {
            assert!(names.contains(required), "missing field: {required}");
        }
    }

    #[test]
    fn edge_schema_has_required_fields() {
        let schema = edge_schema();
        let names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        for required in &["src_id", "dst_id", "weight"] {
            assert!(names.contains(required), "missing field: {required}");
        }
    }
}
