//! ProducerStage — progress reservation co-designed with ReadyEdges.
//!
//! Protocol: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.
//!
//! Scheduler owns the Stage (Execute/Repair of writer \(w\)). Admission
//! co-owns when consumer \(t\) may enter ready. They are peers: refuse \(t\)
//! only when a ProducerStage(\(w\)) is reserved and runnable. If the
//! collaborative index cannot see \(w\), **promote** \(w\) — never spin.

use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;

use crate::{BuildIdentityHasher, TxIdx};

/// Reserved producer Stages for the current block.
#[derive(Debug, Default)]
pub(crate) struct ProducerStageTable {
    reserved: DashMap<TxIdx, AtomicUsize, BuildIdentityHasher>,
    reserved_n: AtomicUsize,
    promote: AtomicUsize,
}

impl ProducerStageTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reserve Execute/Repair work for writer \(w\) (edge insert / refuse).
    #[inline]
    pub(crate) fn reserve(&self, writer: TxIdx) {
        match self.reserved.entry(writer) {
            Entry::Occupied(e) => {
                e.get().fetch_add(1, Ordering::Relaxed);
            }
            Entry::Vacant(e) => {
                e.insert(AtomicUsize::new(1));
                self.reserved_n.fetch_add(1, Ordering::Relaxed);
            }
        }
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
        if self.reserved.remove(&writer).is_some() {
            self.reserved_n.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Empty-table check so A0-majority steals skip the reserved-min scan.
    #[inline]
    pub(crate) fn has_reserved(&self) -> bool {
        self.reserved_n.load(Ordering::Relaxed) > 0
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
        assert!(t.has_reserved());
        assert!(t.is_reserved(3));
        assert_eq!(t.next_reserved(), Some(1));
        t.note_done(1);
        assert_eq!(t.next_reserved(), Some(3));
        t.note_done(3);
        assert!(t.next_reserved().is_none());
        assert!(!t.has_reserved());
    }

    #[test]
    fn empty_table_has_no_reserved() {
        let t = ProducerStageTable::new();
        assert!(!t.has_reserved());
        assert!(t.next_reserved().is_none());
    }
}
