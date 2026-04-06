use serde::{Deserialize, Serialize};

/// First 16 bytes of blake3(content + timestamp_ms as little-endian bytes).
pub type MemoryId = [u8; 16];

/// Relevance score in [0.0, 1.0].
pub type Score = f32;

/// Number of tokens (exact or estimated).
pub type TokenCount = usize;

// ---------------------------------------------------------------------------
// Episode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
    System,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EpisodeMetadata {
    /// Arbitrary key/value pairs for application use.
    pub tags: Vec<String>,
    /// Entity IDs mentioned in this episode (populated by entity extraction).
    pub entity_ids: Vec<String>,
    /// Topic tags derived from content (populated by entity extraction).
    pub topic_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: MemoryId,
    pub session_id: String,
    pub timestamp_ms: u64,
    pub role: Role,
    pub content: String,
    /// Pre-computed embedding (None = will be computed by store_episode).
    pub embedding: Option<Vec<f32>>,
    /// TTL in seconds from timestamp_ms. None = no expiry.
    pub ttl_secs: Option<u64>,
    pub metadata: EpisodeMetadata,
}

impl Episode {
    pub fn new(session_id: impl Into<String>, role: Role, content: impl Into<String>) -> Self {
        let content = content.into();
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let id = compute_memory_id(&content, timestamp_ms);
        Self {
            id,
            session_id: session_id.into(),
            timestamp_ms,
            role,
            content,
            embedding: None,
            ttl_secs: None,
            metadata: EpisodeMetadata::default(),
        }
    }

    /// First 256 chars of content for use as a preview / ANN payload.
    pub fn preview(&self) -> &str {
        let end = self.content.char_indices().nth(256).map(|(i, _)| i).unwrap_or(self.content.len());
        &self.content[..end]
    }

    /// TTL expiry timestamp in ms. None if no TTL set.
    pub fn ttl_expires_ms(&self) -> Option<u64> {
        self.ttl_secs.map(|s| self.timestamp_ms + s * 1000)
    }
}

// ---------------------------------------------------------------------------
// Entity and Fact
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    Person,
    Organization,
    Location,
    Event,
    Object,
    Concept,
    Custom(String),
}

impl EntityKind {
    pub fn as_str(&self) -> &str {
        match self {
            EntityKind::Person => "person",
            EntityKind::Organization => "organization",
            EntityKind::Location => "location",
            EntityKind::Event => "event",
            EntityKind::Object => "object",
            EntityKind::Concept => "concept",
            EntityKind::Custom(s) => s.as_str(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Format: "{kind}:{canonical_name}", e.g. "person:alice".
    pub id: String,
    pub kind: EntityKind,
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub attributes: serde_json::Value,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub mention_count: u32,
}

/// Subject-Predicate-Object fact with bi-temporal validity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub subject_entity_id: String,
    pub predicate: String,
    pub object_entity_id: Option<String>,
    pub object_literal: Option<String>,
    pub confidence: f32,
    pub source_episode_ids: Vec<MemoryId>,
    /// Bi-temporal: when this fact became valid (ms).
    pub valid_from_ms: Option<u64>,
    /// Bi-temporal: when this fact stopped being valid (ms). None = still valid.
    pub valid_until_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Retrieval results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MemoryTier {
    Working,
    Fact,
    Relationship,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedMemory {
    pub id: MemoryId,
    pub content: String,
    pub score: Score,
    pub tier: MemoryTier,
    pub timestamp_ms: u64,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AssembledContext {
    /// Ready-to-prepend string for the LLM prompt.
    pub prompt_prefix: String,
    pub tokens_used: TokenCount,
    /// Source memories included (for attribution and debugging).
    pub sources: Vec<RetrievedMemory>,
    /// True if some memories were dropped to fit the budget.
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute a 16-byte MemoryId as the first 16 bytes of blake3(content || timestamp_ms_le).
pub fn compute_memory_id(content: &str, timestamp_ms: u64) -> MemoryId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(content.as_bytes());
    hasher.update(&timestamp_ms.to_le_bytes());
    let hash = hasher.finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&hash.as_bytes()[..16]);
    id
}

pub fn memory_id_to_hex(id: &MemoryId) -> String {
    hex::encode(id)
}
