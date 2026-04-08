use std::collections::VecDeque;

use crate::types::{Episode, MemoryTier, RetrievedMemory};

/// Strategy applied when the ring buffer is full and a new episode arrives.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum OverflowStrategy {
    /// Drop the oldest episode unconditionally.
    #[default]
    DropOldest,
    /// Summarize the oldest episodes before dropping (Phase 2+).
    ///
    /// Falls back to `DropOldest` in Phase 1 — no summarizer is wired in yet.
    SummarizeOldest,
}

/// In-process ring buffer of recent conversation turns.
///
/// Used as step 1 of `assemble_context()`: always included in the assembled
/// context to anchor recency before the ANN search results.
///
/// No external dependencies. Thread-safety is the caller's concern.
pub struct WorkingMemory {
    buffer: VecDeque<Episode>,
    max_turns: usize,
    overflow: OverflowStrategy,
}

impl WorkingMemory {
    pub fn new(max_turns: usize) -> Self {
        assert!(max_turns > 0, "max_turns must be > 0");
        Self {
            buffer: VecDeque::with_capacity(max_turns),
            max_turns,
            overflow: OverflowStrategy::default(),
        }
    }

    pub fn with_overflow(mut self, strategy: OverflowStrategy) -> Self {
        self.overflow = strategy;
        self
    }

    /// Push an episode into the ring buffer.
    ///
    /// If the buffer is full, the overflow strategy is applied.
    /// `SummarizeOldest` falls back to `DropOldest` in Phase 1.
    pub fn push(&mut self, episode: Episode) {
        if self.buffer.len() >= self.max_turns {
            // SummarizeOldest is a Phase 2+ feature; drop for now.
            self.buffer.pop_front();
        }
        self.buffer.push_back(episode);
    }

    /// Return all episodes in chronological order as [`RetrievedMemory`].
    ///
    /// Score is 1.0 — working-memory entries are always considered maximally
    /// relevant. They anchor recency in the assembled context.
    pub fn scan(&self) -> Vec<RetrievedMemory> {
        self.buffer
            .iter()
            .map(|ep| RetrievedMemory {
                id: ep.id,
                content: ep.content.clone(),
                score: 1.0,
                tier: MemoryTier::Working,
                timestamp_ms: ep.timestamp_ms,
                metadata: serde_json::json!({
                    "role": ep.role.as_str(),
                    "session_id": ep.session_id,
                }),
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn max_turns(&self) -> usize {
        self.max_turns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Role, Episode};

    fn ep(content: &str) -> Episode {
        Episode::new("test-session", Role::User, content)
    }

    #[test]
    fn push_and_scan_in_order() {
        let mut wm = WorkingMemory::new(4);
        wm.push(ep("first"));
        wm.push(ep("second"));
        wm.push(ep("third"));

        let items = wm.scan();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].content, "first");
        assert_eq!(items[1].content, "second");
        assert_eq!(items[2].content, "third");
    }

    #[test]
    fn overflow_drops_oldest() {
        let mut wm = WorkingMemory::new(3);
        wm.push(ep("a"));
        wm.push(ep("b"));
        wm.push(ep("c"));
        wm.push(ep("d")); // overflows; "a" is dropped

        assert_eq!(wm.len(), 3);
        let items = wm.scan();
        assert_eq!(items[0].content, "b");
        assert_eq!(items[2].content, "d");
    }

    #[test]
    fn overflow_summarize_falls_back_to_drop() {
        let mut wm = WorkingMemory::new(2).with_overflow(OverflowStrategy::SummarizeOldest);
        wm.push(ep("x"));
        wm.push(ep("y"));
        wm.push(ep("z")); // "x" is dropped (SummarizeOldest falls back to DropOldest)

        assert_eq!(wm.len(), 2);
        let items = wm.scan();
        assert_eq!(items[0].content, "y");
        assert_eq!(items[1].content, "z");
    }

    #[test]
    fn scan_score_and_tier() {
        let mut wm = WorkingMemory::new(4);
        wm.push(ep("hello"));

        let items = wm.scan();
        assert_eq!(items[0].score, 1.0);
        assert_eq!(items[0].tier, MemoryTier::Working);
    }

    #[test]
    fn clear_empties_buffer() {
        let mut wm = WorkingMemory::new(4);
        wm.push(ep("hello"));
        wm.clear();

        assert!(wm.is_empty());
        assert_eq!(wm.scan().len(), 0);
    }

    #[test]
    fn scan_empty_returns_empty_vec() {
        let wm = WorkingMemory::new(4);
        assert_eq!(wm.scan().len(), 0);
    }
}
