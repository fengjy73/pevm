//! Ready-edge graph over Region-accesses — PE unpublished-RAW refuses Execute.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v6-essence.md` §2.1.
//! First-wave Avoid at **schedule**, not abort-then-reincarnate.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::rem::WaveParkTable;

const NONE: usize = usize::MAX;

/// `(consumer_t) ← producer_t` on PE / RAW class.
#[derive(Debug, Default)]
pub(crate) struct ReadyEdgeTable {
    /// Earliest unpublished !done producer per PE location.
    producers: DashMap<MemoryLocationHash, AtomicUsize, BuildIdentityHasher>,
    /// Ready txs skipped because a PE producer was unpublished.
    deferred: Mutex<Vec<TxIdx>>,
    refuse: AtomicUsize,
}

impl ReadyEdgeTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// PE unpublished-RAW: `writer` gates later Executes.
    #[inline]
    pub(crate) fn note_unpublished(&self, location: MemoryLocationHash, writer: TxIdx) {
        let e = self
            .producers
            .entry(location)
            .or_insert_with(|| AtomicUsize::new(NONE));
        let mut cur = e.load(Ordering::Relaxed);
        while writer < cur {
            match e.compare_exchange_weak(cur, writer, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(v) => cur = v,
            }
        }
    }

    /// Producer published Data for \(\ell\) — drop this writer if it was the tip.
    #[inline]
    pub(crate) fn note_published(&self, location: MemoryLocationHash, writer: TxIdx) {
        if let Some(e) = self.producers.get(&location)
            && e.load(Ordering::Relaxed) == writer
        {
            e.store(NONE, Ordering::Relaxed);
        }
    }

    /// Writer finished (Executed|Validated). Flush deferred Ready consumers.
    pub(crate) fn note_producer_done(&self, writer: TxIdx, wave: &WaveParkTable) {
        for e in self.producers.iter() {
            if e.load(Ordering::Relaxed) == writer {
                e.store(NONE, Ordering::Relaxed);
            }
        }
        if self.has_unpublished() {
            return;
        }
        let mut d = self.deferred.lock().unwrap();
        for t in d.drain(..) {
            wave.push_ready(t);
        }
    }

    #[inline]
    fn has_unpublished(&self) -> bool {
        self.producers
            .iter()
            .any(|e| e.load(Ordering::Relaxed) != NONE)
    }

    /// `Execute(t)` is ready iff no PE unpublished producer `w < t` remains.
    #[inline]
    pub(crate) fn may_execute(&self, tx_idx: TxIdx) -> bool {
        if tx_idx == 0 {
            return true;
        }
        !self
            .producers
            .iter()
            .any(|e| e.load(Ordering::Relaxed) < tx_idx)
    }

    #[inline]
    pub(crate) fn blocking_producer(&self, tx_idx: TxIdx) -> Option<TxIdx> {
        let mut best = None;
        for e in self.producers.iter() {
            let w = e.load(Ordering::Relaxed);
            if w < tx_idx {
                best = Some(best.map_or(w, |b: TxIdx| b.min(w)));
            }
        }
        best
    }

    #[inline]
    pub(crate) fn defer(&self, tx_idx: TxIdx) {
        self.refuse.fetch_add(1, Ordering::Relaxed);
        self.deferred.lock().unwrap().push(tx_idx);
    }

    #[inline]
    pub(crate) fn refuse_count(&self) -> usize {
        self.refuse.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpublished_refuses_later_execute() {
        let t = ReadyEdgeTable::new();
        assert!(t.may_execute(3));
        t.note_unpublished(7, 1);
        assert!(t.may_execute(0));
        assert!(t.may_execute(1));
        assert!(!t.may_execute(3));
        assert_eq!(t.blocking_producer(3), Some(1));
        t.note_published(7, 1);
        assert!(t.may_execute(3));
    }

    #[test]
    fn done_flushes_deferred() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_unpublished(9, 0);
        t.defer(4);
        t.note_producer_done(0, &wave);
        assert!(t.may_execute(4));
        assert_eq!(wave.pop_ready(), Some(4));
    }
}
