//! Ready-edge graph — PC ⊗ CC shared ready membership (not CC annotation).
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.
//! First-wave Avoid at **schedule** for **known** consumers only.
//! Refuse only when ProducerStage(w) is runnable — v6 “defer consumer only”
//! deadlocked when w was off the collaborative index.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use dashmap::{DashMap, DashSet};

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
    /// D1: consensus-order writers observed on each location (lazy + Data).
    location_writers: DashMap<MemoryLocationHash, Vec<TxIdx>, BuildIdentityHasher>,
    /// PC-3: each tx belongs to at most one ReadyEdge location queue.
    queued_on: DashMap<TxIdx, MemoryLocationHash, BuildIdentityHasher>,
    /// PC-5: Lean / EV forced A0 — `may_execute` even if a consumer bit exists.
    a0_force: DashSet<TxIdx, BuildIdentityHasher>,
    /// PC-4: sampled ready width (may_execute ∧ Ready).
    ready_width_sum: AtomicU64,
    ready_width_n: AtomicUsize,
    /// PC-4: ns spent in scheduler yield / empty refuse.
    idle_core_ns: AtomicU64,
    /// PC-W1: A1-blocked heads — do not re-probe until pred Done.
    sleeping: DashSet<TxIdx, BuildIdentityHasher>,
    /// L6: ns spent in refuse / defer path.
    refuse_ns: AtomicU64,
    /// Producer → known consumers (completion event → bag; no DashMap scan).
    waiters: DashMap<TxIdx, Vec<TxIdx>, BuildIdentityHasher>,
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
        self.note_consumer_on(consumer, producer, None);
    }

    /// PC-3: bind `consumer` to at most one location queue. Write-set upgrade
    /// rebinds from a cold-start probe onto the real location.
    #[inline]
    pub(crate) fn note_consumer_on(
        &self,
        consumer: TxIdx,
        producer: TxIdx,
        location: Option<MemoryLocationHash>,
    ) {
        if producer >= consumer || self.finished.contains_key(&producer) {
            return;
        }
        if let Some(loc) = location {
            if let Some(prev) = self.queued_on.get(&consumer) {
                let prev_loc = *prev;
                drop(prev);
                if prev_loc != loc {
                    // Rebind: drop the stale envelope queue (from+to dual tax).
                    if let Some(e) = self.consumers.get(&consumer) {
                        e.store(NONE, Ordering::Relaxed);
                    }
                }
            }
            self.queued_on.insert(consumer, loc);
        } else if self.queued_on.contains_key(&consumer) {
            // Already on a location queue — do not add a second anonymous edge.
            // Immediate-pred upgrade on the existing bit is still allowed below
            // only if a consumer entry exists.
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
        self.waiters.entry(producer).or_default().push(consumer);
    }

    /// D1: record `writer` on location `ℓ` (lazy or Data). Order is consensus.
    pub(crate) fn note_location_writer(&self, location: MemoryLocationHash, writer: TxIdx) {
        let mut v = self.location_writers.entry(location).or_default();
        match v.last() {
            Some(&last) if last == writer => {}
            Some(&last) if last < writer => v.push(writer),
            _ => {
                if v.binary_search(&writer).is_err() {
                    v.push(writer);
                    v.sort_unstable();
                }
            }
        }
    }

    /// D1: extend ReadyEdge total order over **all** writers observed on `ℓ`.
    pub(crate) fn extend_writer_order(&self, location: MemoryLocationHash) -> usize {
        let Some(e) = self.location_writers.get(&location) else {
            return 0;
        };
        let writers = e.clone();
        drop(e);
        let mut edges = 0;
        for pair in writers.windows(2) {
            let (pred, succ) = (pair[0], pair[1]);
            if pred >= succ {
                continue;
            }
            self.note_consumer_on(succ, pred, Some(location));
            edges += 1;
        }
        edges
    }

    /// D1 hot path: only the immediate predecessor → `writer` edge.
    pub(crate) fn note_immediate_pred(&self, location: MemoryLocationHash, writer: TxIdx) {
        let pred = self
            .location_writers
            .get(&location)
            .and_then(|e| e.iter().rev().copied().find(|&w| w < writer));
        if let Some(p) = pred {
            self.note_consumer_on(writer, p, Some(location));
        }
    }

    /// Consensus-order writers published on `ℓ` (lab / compare).
    pub(crate) fn writers_of(&self, location: MemoryLocationHash) -> Vec<TxIdx> {
        self.location_writers
            .get(&location)
            .map(|e| e.clone())
            .unwrap_or_default()
    }

    pub(crate) fn writer_order_snapshot(&self) -> Vec<(MemoryLocationHash, Vec<TxIdx>)> {
        let mut out: Vec<(MemoryLocationHash, Vec<TxIdx>)> = self
            .location_writers
            .iter()
            .map(|e| (*e.key(), e.value().clone()))
            .collect();
        out.sort_by_key(|(loc, _)| *loc);
        out
    }

    /// Consumers that cannot execute at the moment (begin-block tax snapshot).
    pub(crate) fn blocked_consumers(&self) -> Vec<TxIdx> {
        let mut out: Vec<TxIdx> = self
            .consumers
            .iter()
            .filter_map(|e| {
                let t = *e.key();
                if self.may_execute(t) { None } else { Some(t) }
            })
            .collect();
        out.sort_unstable();
        out
    }

    #[inline]
    pub(crate) fn was_queued(&self, tx: TxIdx) -> bool {
        // Probe heads are producers (waiters) but not consumers — they are A1.
        self.queued_on.contains_key(&tx)
            || self.consumers.contains_key(&tx)
            || self.waiters.contains_key(&tx)
    }

    /// PC-5: force A0 on this consumer (execute anyway).
    #[inline]
    pub(crate) fn force_a0(&self, tx: TxIdx) {
        self.a0_force.insert(tx);
    }

    #[inline]
    pub(crate) fn sample_ready_width(&self, width: usize) {
        self.ready_width_sum
            .fetch_add(width as u64, Ordering::Relaxed);
        self.ready_width_n.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn add_idle_ns(&self, ns: u64) {
        if ns > 0 {
            self.idle_core_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn ready_width_mean(&self) -> f64 {
        let n = self.ready_width_n.load(Ordering::Relaxed);
        if n == 0 {
            0.0
        } else {
            self.ready_width_sum.load(Ordering::Relaxed) as f64 / n as f64
        }
    }

    #[inline]
    pub(crate) fn idle_core_ns(&self) -> u64 {
        self.idle_core_ns.load(Ordering::Relaxed)
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
    ///
    /// Independents (never gated anyone, never queued) skip the deferred lock
    /// and the producer/consumer DashMap scans — those were the thin-shell tax.
    pub(crate) fn note_producer_done(&self, writer: TxIdx, wave: &WaveParkTable) {
        self.finished.insert(writer, ());
        let waiters = self.waiters.remove(&writer);
        if waiters.is_none() && !self.was_queued(writer) {
            return;
        }
        if let Some((_, cs)) = waiters {
            for c in cs {
                // Only the consumers still gated on *this* writer. Probe-star
                // leftovers that already rebased onto a later pred must stay.
                let Some(e) = self.consumers.get(&c) else {
                    continue;
                };
                if e.load(Ordering::Relaxed) != writer {
                    continue;
                }
                e.store(NONE, Ordering::Relaxed);
                self.sleeping.remove(&c);
                if self.may_execute(c) {
                    wave.push_ready(c);
                }
            }
        }
        let mut d = self.deferred.lock().unwrap();
        if d.is_empty() {
            return;
        }
        d.retain(|&t| {
            if self.may_execute(t) {
                self.sleeping.remove(&t);
                wave.push_ready(t);
                false
            } else {
                true
            }
        });
    }

    /// Refuse only known consumers still gated by an unpublished producer.
    /// Stale bits (producer already flushed to NONE) must not refuse.
    /// PC-5 Lean / EV A0 override executes anyway.
    #[inline]
    pub(crate) fn may_execute(&self, tx_idx: TxIdx) -> bool {
        if tx_idx == 0 || self.a0_force.contains(&tx_idx) {
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
    /// PC-W1: mark sleeping so steal/index skip this head until pred Done.
    #[inline]
    pub(crate) fn defer(&self, tx_idx: TxIdx) {
        let t0 = Instant::now();
        let mut d = self.deferred.lock().unwrap();
        if d.iter().any(|&t| t == tx_idx) {
            self.sleeping.insert(tx_idx);
            return;
        }
        self.refuse.fetch_add(1, Ordering::Relaxed);
        self.sleeping.insert(tx_idx);
        d.push(tx_idx);
        drop(d);
        self.add_refuse_ns(t0.elapsed().as_nanos() as u64);
    }

    #[inline]
    pub(crate) fn is_sleeping(&self, tx_idx: TxIdx) -> bool {
        self.sleeping.contains(&tx_idx) && !self.may_execute(tx_idx)
    }

    #[inline]
    pub(crate) fn add_refuse_ns(&self, ns: u64) {
        if ns > 0 {
            self.refuse_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn refuse_ns(&self) -> u64 {
        self.refuse_ns.load(Ordering::Relaxed)
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
        self.sleeping.remove(&consumer);
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

    #[test]
    fn d1_writer_order_includes_all_earlier() {
        let t = ReadyEdgeTable::new();
        t.note_location_writer(0x32be, 4);
        t.note_location_writer(0x32be, 31);
        t.note_location_writer(0x32be, 66);
        assert_eq!(t.extend_writer_order(0x32be), 2);
        assert_eq!(t.writers_of(0x32be), vec![4, 31, 66]);
        assert_eq!(t.blocking_producer(31), Some(4));
        assert_eq!(t.blocking_producer(66), Some(31));
    }

    #[test]
    fn pc5_force_a0_allows_execute() {
        let t = ReadyEdgeTable::new();
        t.note_consumer(8, 3);
        assert!(!t.may_execute(8));
        t.force_a0(8);
        assert!(t.may_execute(8));
    }

    #[test]
    fn pcw1_sleeping_head_not_redeferred() {
        let t = ReadyEdgeTable::new();
        t.note_consumer(8, 3);
        t.defer(8);
        t.defer(8);
        assert_eq!(t.refuse_count(), 1);
        assert!(t.is_sleeping(8));
        let wave = WaveParkTable::new();
        t.note_producer_done(3, &wave);
        assert!(!t.is_sleeping(8));
        assert!(t.may_execute(8));
    }

    #[test]
    fn producer_done_does_not_wake_rebased_probe_star() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_consumer(66, 31);
        t.note_consumer(67, 31);
        t.note_consumer(67, 66);
        assert_eq!(t.blocking_producer(67), Some(66));
        t.note_producer_done(31, &wave);
        assert_eq!(wave.pop_ready(), Some(66));
        assert!(
            wave.pop_ready().is_none(),
            "67 must stay behind 66 after probe-star rebase"
        );
        assert_eq!(t.blocking_producer(67), Some(66));
    }

    #[test]
    fn producer_done_wakes_non_deferred_waiter_into_bag() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_consumer(16, 14);
        assert!(!t.may_execute(16));
        t.note_producer_done(14, &wave);
        assert!(t.may_execute(16));
        assert_eq!(
            wave.pop_ready(),
            Some(16),
            "completion event must bag the waiter without a full-block steal"
        );
    }

    #[test]
    fn independent_done_skips_bag_work() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_producer_done(5, &wave);
        assert!(wave.pop_ready().is_none());
        assert!(t.may_execute(6));
    }
}
