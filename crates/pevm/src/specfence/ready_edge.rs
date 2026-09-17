//! Ready-edge graph — PC ⊗ CC shared ready membership (not CC annotation).
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.
//! First-wave Avoid at **schedule** for **known** consumers only.
//! Refuse only when ProducerStage(w) is runnable — v6 “defer consumer only”
//! deadlocked when w was off the collaborative index.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::wave::WaveParkTable;

const NONE: usize = usize::MAX;

/// `(consumer_t) ← producer_t` on PE / RAW class.
#[derive(Debug, Default)]
pub(crate) struct ReadyEdgeTable {
    /// Earliest unpublished producer per PE location (observe / wake).
    producers: DashMap<MemoryLocationHash, AtomicUsize, BuildIdentityHasher>,
    /// Known doomed consumers → blocking producer.
    consumers: DashMap<TxIdx, AtomicUsize, BuildIdentityHasher>,
    /// Producers that have already `note_producer_done` — stale
    /// `note_consumer` after Done must not refuse forever.
    finished: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// Predicted RAW producer tip per ℓ (CC OrderedAdmit-rare: tip == this writer).
    tips: DashMap<MemoryLocationHash, AtomicUsize, BuildIdentityHasher>,
    deferred: Mutex<Vec<TxIdx>>,
    refuse: AtomicUsize,
}

impl ReadyEdgeTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// PE unpublished-RAW producer (observe). Does **not** gate all later txs.
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

    /// Register a **known** consumer (this reader hit unpublished RAW / aborted).
    ///
    /// Keeps the **latest** unfinished predecessor (WAW immediate pred). A
    /// begin-block probe-star (all later txs wait on the first) can upgrade to
    /// a predecessor chain after the first write-set Detects a hidden location.
    #[inline]
    pub(crate) fn note_consumer(&self, consumer: TxIdx, producer: TxIdx) {
        if producer >= consumer || self.finished.contains_key(&producer) {
            return;
        }
        let e = self
            .consumers
            .entry(consumer)
            .or_insert_with(|| AtomicUsize::new(producer));
        let mut cur = e.load(Ordering::Relaxed);
        while cur == NONE || producer > cur {
            match e.compare_exchange_weak(cur, producer, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(v) => cur = v,
            }
        }
    }

    /// Producer published Data for \(\ell\).
    #[inline]
    pub(crate) fn note_published(&self, location: MemoryLocationHash, writer: TxIdx) {
        if let Some(e) = self.producers.get(&location)
            && e.load(Ordering::Relaxed) == writer
        {
            e.store(NONE, Ordering::Relaxed);
        }
    }

    /// Abort / HotSet RAW producer identity for OrderedAdmit-rare tip check.
    #[inline]
    pub(crate) fn note_raw_producer(&self, location: MemoryLocationHash, writer: TxIdx) {
        self.note_unpublished(location, writer);
        self.tips
            .entry(location)
            .or_insert_with(|| AtomicUsize::new(writer))
            .store(writer, Ordering::Relaxed);
    }

    /// Predicted conflicting producer for \(\ell\) (none ⇒ OrderedAdmit forbidden).
    #[inline]
    pub(crate) fn predicted_producer(&self, location: MemoryLocationHash) -> Option<TxIdx> {
        self.tips
            .get(&location)
            .map(|e| e.load(Ordering::Relaxed))
            .filter(|&w| w != NONE)
    }

    /// Writer finished. Wake known consumers whose producer is now done.
    pub(crate) fn note_producer_done(&self, writer: TxIdx, wave: &WaveParkTable) {
        self.finished.insert(writer, ());
        for e in self.producers.iter() {
            if e.load(Ordering::Relaxed) == writer {
                e.store(NONE, Ordering::Relaxed);
            }
        }
        for e in self.consumers.iter() {
            if e.load(Ordering::Relaxed) == writer {
                e.store(NONE, Ordering::Relaxed);
            }
        }
        let mut d = self.deferred.lock().unwrap();
        d.retain(|&t| {
            if self.may_execute(t) {
                wave.push_ready(t);
                false
            } else {
                true
            }
        });
    }

    /// Refuse only known consumers still gated by an unpublished producer.
    /// Stale bits (producer already flushed to NONE) must not refuse.
    #[inline]
    pub(crate) fn may_execute(&self, tx_idx: TxIdx) -> bool {
        if tx_idx == 0 {
            return true;
        }
        match self.consumers.get(&tx_idx) {
            Some(e) => {
                let w = e.load(Ordering::Relaxed);
                w == NONE || w >= tx_idx || self.finished.contains_key(&w)
            }
            None => true,
        }
    }

    #[inline]
    pub(crate) fn blocking_producer(&self, tx_idx: TxIdx) -> Option<TxIdx> {
        self.consumers
            .get(&tx_idx)
            .map(|e| e.load(Ordering::Relaxed))
            .filter(|&w| w < tx_idx && !self.finished.contains_key(&w))
    }

    /// Defer a known consumer. Count once until the producer finishes —
    /// re-probing the same head must not spin `refuse_admit` (19606599 31k).
    #[inline]
    pub(crate) fn defer(&self, tx_idx: TxIdx) {
        let mut d = self.deferred.lock().unwrap();
        if d.iter().any(|&t| t == tx_idx) {
            return;
        }
        self.refuse.fetch_add(1, Ordering::Relaxed);
        d.push(tx_idx);
    }

    #[inline]
    pub(crate) fn refuse_count(&self) -> usize {
        self.refuse.load(Ordering::Relaxed)
    }

    /// Drop a provisional consumer bit and wake it (lazy `to` was not a real WAW).
    pub(crate) fn release_consumer(&self, consumer: TxIdx, wave: &WaveParkTable) {
        if let Some(e) = self.consumers.get(&consumer) {
            e.store(NONE, Ordering::Relaxed);
        }
        let mut d = self.deferred.lock().unwrap();
        let was_deferred = d.iter().any(|&t| t == consumer);
        d.retain(|&t| t != consumer);
        drop(d);
        if was_deferred || self.may_execute(consumer) {
            wave.push_ready(consumer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpublished_alone_does_not_refuse_strangers() {
        let t = ReadyEdgeTable::new();
        t.note_unpublished(7, 1);
        assert!(
            t.may_execute(3),
            "suffix-global refuse is forbidden (deadlocks later producers)"
        );
        t.note_consumer(3, 1);
        assert!(!t.may_execute(3));
        assert_eq!(t.blocking_producer(3), Some(1));
        t.note_published(7, 1);
        assert!(!t.may_execute(3), "consumer bit stays until producer_done");
        let wave = WaveParkTable::new();
        t.note_producer_done(1, &wave);
        assert!(t.may_execute(3));
    }

    #[test]
    fn done_flushes_known_consumer() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_unpublished(9, 0);
        t.note_consumer(4, 0);
        t.defer(4);
        t.defer(4);
        assert_eq!(
            t.refuse_count(),
            1,
            "defer is idempotent until producer_done"
        );
        t.note_producer_done(0, &wave);
        assert!(t.may_execute(4));
        assert_eq!(wave.pop_ready(), Some(4));
    }

    #[test]
    fn predicted_producer_is_conflict_tip() {
        let t = ReadyEdgeTable::new();
        assert!(t.predicted_producer(7).is_none());
        t.note_raw_producer(7, 2);
        assert_eq!(t.predicted_producer(7), Some(2));
    }

    #[test]
    fn stale_consumer_after_done_does_not_refuse() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_unpublished(7, 1);
        t.note_producer_done(1, &wave);
        t.note_consumer(3, 1);
        assert!(
            t.may_execute(3),
            "note_consumer after Done must not refuse forever"
        );
    }

    #[test]
    fn note_consumer_upgrades_probe_to_immediate_pred() {
        let t = ReadyEdgeTable::new();
        t.note_consumer(67, 31);
        t.note_consumer(67, 66);
        assert_eq!(
            t.blocking_producer(67),
            Some(66),
            "WAW Avoid waits on the latest unfinished predecessor"
        );
    }

    #[test]
    fn release_consumer_clears_provisional_wait() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_consumer(5, 2);
        t.defer(5);
        assert!(!t.may_execute(5));
        t.release_consumer(5, &wave);
        assert!(t.may_execute(5));
        assert_eq!(wave.pop_ready(), Some(5));
    }
}
