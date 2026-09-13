//! ProducerStage — PC progress reservation co-designed with CC ReadyEdges.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.
//!
//! PC owns the Stage (Execute/Repair of writer \(w\)). CC co-owns when
//! consumer \(t\) may enter ready. They are peers: refuse \(t\) only when a
//! ProducerStage(\(w\)) is reserved and runnable. If the collaborative index
//! cannot see \(w\), **promote** \(w\) — never spin.

use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;

use crate::{BuildIdentityHasher, TxIdx};

/// Reserved producer Stages for the current block.
#[derive(Debug, Default)]
pub(crate) struct ProducerStageTable {
    reserved: DashMap<TxIdx, AtomicUsize, BuildIdentityHasher>,
    promote: AtomicUsize,
}

impl ProducerStageTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reserve Execute/Repair work for writer \(w\) (CC edge insert / refuse).
    #[inline]
    pub(crate) fn reserve(&self, writer: TxIdx) {
        self.reserved
            .entry(writer)
            .or_insert_with(|| AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn is_reserved(&self, writer: TxIdx) -> bool {
        self.reserved
            .get(&writer)
            .is_some_and(|e| e.load(Ordering::Relaxed) > 0)
    }

    /// Writer finished — drop reservation (publish / Done).
    #[inline]
    pub(crate) fn note_done(&self, writer: TxIdx) {
        self.reserved.remove(&writer);
    }

    /// Earliest reserved producer (PC prefers ProducerStage over PE-blocked Execute).
    #[inline]
    pub(crate) fn next_reserved(&self) -> Option<TxIdx> {
        self.reserved.iter().map(|e| *e.key()).min()
    }

    #[inline]
    pub(crate) fn note_promote(&self) {
        self.promote.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn promote_count(&self) -> usize {
        self.promote.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_and_done() {
        let t = ProducerStageTable::new();
        t.reserve(3);
        t.reserve(1);
        assert!(t.is_reserved(3));
        assert_eq!(t.next_reserved(), Some(1));
        t.note_done(1);
        assert_eq!(t.next_reserved(), Some(3));
        t.note_done(3);
        assert!(t.next_reserved().is_none());
    }
}
