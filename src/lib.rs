pub mod arrow;
pub mod error;
pub mod types;

// Phase 1 modules (stubs — filled in by subsequent milestones)
pub mod memory;
pub mod storage;
pub mod embedding;
pub mod ops;
pub mod context;
pub mod entity;
pub mod audit;

pub use error::{MembridError, Result};
pub use types::{
    AssembledContext, Episode, EpisodeMetadata, Entity, EntityKind, Fact,
    MemoryId, MemoryTier, RetrievedMemory, Role, Score, TokenCount,
    compute_memory_id, memory_id_to_hex,
};
