//! Ready-edge graph — dependency-aware admission membership (not CC annotation).
//!
//! Protocol: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.
//! First-wave Avoid at **schedule** for **known** consumers only.
//! Refuse only when ProducerStage(w) is runnable — v6 “defer consumer only”
//! deadlocked when w was off the collaborative index.
//!
//! Ungated / A0-majority txs use an OCC-class `may_execute` (bitset; no DashMap).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::engagement::profile_timing_enabled;
use super::wave::WaveParkTable;

const NONE: usize = usize::MAX;
/// Bitset words for ordered-admit gated txs (64×64 = 4096). Beyond this,
/// `is_gated` falls back to the consumer map.
const GATED_WORDS: usize = 64;

/// `(consumer_t) ← producer_t` on PE / RAW class.
#[derive(Debug)]
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
    /// P1: skip the deferred mutex when the bag is empty (A0 / no refuse).
    deferred_n: AtomicUsize,
    refuse: AtomicUsize,
    /// D1: consensus-order writers observed on each location (lazy + Data).
    location_writers: DashMap<MemoryLocationHash, Vec<TxIdx>, BuildIdentityHasher>,
    /// PC-3: each tx belongs to at most one ReadyEdge location queue.
    queued_on: DashMap<TxIdx, MemoryLocationHash, BuildIdentityHasher>,
    /// PC-5: EV forced A0 — `may_execute` even if a consumer bit exists.
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
    /// Ordered-admit gated txs. Marked **before** the consumer map insert so
    /// an ungated steal cannot race past a newly created wait-for edge.
    gated_bits: [AtomicU64; GATED_WORDS],
    /// Live gated-tx count (P1/P3: A1=0 → OCC-class pick, no ReadyEdge walk).
    gated_n: AtomicUsize,
    /// Done writers (bitset). A0 finish is an atomic or — no `finished` DashMap.
    done_bits: [AtomicU64; GATED_WORDS],
    /// Execute started (bitset). Write-set must not refuse an in-flight succ.
    started_bits: [AtomicU64; GATED_WORDS],
}

impl Default for ReadyEdgeTable {
    fn default() -> Self {
        Self {
            producers: DashMap::default(),
            consumers: DashMap::default(),
            finished: DashMap::default(),
            tips: DashMap::default(),
            deferred: Mutex::new(Vec::new()),
            deferred_n: AtomicUsize::new(0),
            refuse: AtomicUsize::new(0),
            location_writers: DashMap::default(),
            queued_on: DashMap::default(),
            a0_force: DashSet::default(),
            ready_width_sum: AtomicU64::new(0),
            ready_width_n: AtomicUsize::new(0),
            idle_core_ns: AtomicU64::new(0),
            sleeping: DashSet::default(),
            refuse_ns: AtomicU64::new(0),
            waiters: DashMap::default(),
            gated_bits: std::array::from_fn(|_| AtomicU64::new(0)),
            gated_n: AtomicUsize::new(0),
            done_bits: std::array::from_fn(|_| AtomicU64::new(0)),
            started_bits: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
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
        if producer >= consumer || self.is_writer_done(producer) {
            return;
        }
        // Mark first: ungated `may_execute` must not observe a missing bit
        // while the consumer map already refuses.
        self.mark_gated(consumer);
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
        drop(e);
        self.waiters.entry(producer).or_default().push(consumer);
        // Insert raced with pred Done — do not leave a refuse-forever gate.
        if self.is_writer_done(producer)
            && let Some(c) = self.consumers.get(&consumer)
            && c.load(Ordering::Relaxed) == producer
        {
            c.store(NONE, Ordering::Relaxed);
        }
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

    /// Ordered-admit readiness: this tx has (or is gaining) a wait-for edge.
    #[inline]
    fn mark_gated(&self, tx: TxIdx) {
        let i = tx / 64;
        if i < self.gated_bits.len() {
            let bit = 1u64 << (tx % 64);
            let prev = self.gated_bits[i].fetch_or(bit, Ordering::Release);
            if prev & bit == 0 {
                self.gated_n.fetch_add(1, Ordering::Relaxed);
            }
        } else if !self.consumers.contains_key(&tx) {
            self.gated_n.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// True when this tx is on a dependency-aware admission edge.
    /// Ungated (A0 / independent) is a single Acquire bit load — OCC-class.
    #[inline]
    pub(crate) fn is_gated(&self, tx: TxIdx) -> bool {
        let i = tx / 64;
        if i < self.gated_bits.len() {
            let bit = 1u64 << (tx % 64);
            self.gated_bits[i].load(Ordering::Acquire) & bit != 0
        } else {
            self.consumers.contains_key(&tx)
        }
    }

    /// Any live A1 / gated tx in this block (P1: if false, pick ≡ OCC).
    #[inline]
    pub(crate) fn has_any_gated(&self) -> bool {
        self.gated_n.load(Ordering::Relaxed) > 0
    }

    /// Stamp Done without waiter wake (A0 OCC wrap / P3).
    #[inline]
    pub(crate) fn note_producer_done_stamp(&self, writer: TxIdx) {
        self.mark_done(writer);
    }

    /// Mark Execute started so write-set will not refuse this incarnation.
    #[inline]
    pub(crate) fn note_started(&self, tx: TxIdx) {
        let i = tx / 64;
        if i < self.started_bits.len() {
            let bit = 1u64 << (tx % 64);
            self.started_bits[i].fetch_or(bit, Ordering::Release);
        }
    }

    /// O3: incarnation retry is idle again — allow `note_consumer_on_if_idle`.
    #[inline]
    pub(crate) fn clear_started(&self, tx: TxIdx) {
        let i = tx / 64;
        if i < self.started_bits.len() {
            let bit = 1u64 << (tx % 64);
            self.started_bits[i].fetch_and(!bit, Ordering::Release);
        }
    }

    #[inline]
    pub(crate) fn is_started(&self, tx: TxIdx) -> bool {
        let i = tx / 64;
        if i < self.started_bits.len() {
            let bit = 1u64 << (tx % 64);
            self.started_bits[i].load(Ordering::Acquire) & bit != 0
        } else {
            false
        }
    }

    /// C1: raise a short edge only when the successor has not started (idle).
    /// In-flight successors stay OCC this incarnation; L2 abort seeds reexec.
    #[inline]
    pub(crate) fn note_consumer_on_if_idle(
        &self,
        consumer: TxIdx,
        producer: TxIdx,
        location: Option<MemoryLocationHash>,
    ) -> bool {
        if self.is_started(consumer) {
            return false;
        }
        self.note_consumer_on(consumer, producer, location);
        true
    }

    #[inline]
    fn mark_done(&self, writer: TxIdx) {
        let i = writer / 64;
        if i < self.done_bits.len() {
            let bit = 1u64 << (writer % 64);
            self.done_bits[i].fetch_or(bit, Ordering::Release);
        } else {
            self.finished.insert(writer, ());
        }
    }

    #[inline]
    pub(crate) fn is_writer_done(&self, writer: TxIdx) -> bool {
        let i = writer / 64;
        if i < self.done_bits.len() {
            let bit = 1u64 << (writer % 64);
            self.done_bits[i].load(Ordering::Acquire) & bit != 0
        } else {
            self.finished.contains_key(&writer)
        }
    }

    /// PC-5: force A0 on this consumer (execute anyway).
    #[inline]
    pub(crate) fn force_optimistic(&self, tx: TxIdx) {
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

    /// True when some consumer is already gated on this writer (publish-wake needed).
    #[inline]
    pub(crate) fn has_known_waiters(&self, writer: TxIdx) -> bool {
        self.waiters.get(&writer).is_some_and(|v| !v.is_empty())
    }

    /// Writer finished. Wake known consumers whose producer is now done.
    ///
    /// Independents (never gated anyone, never queued) skip the deferred lock
    /// and the producer/consumer DashMap scans — those were the A0-majority tax.
    /// A1=0 / no gated txs: no `finished` DashMap insert.
    pub(crate) fn note_producer_done(&self, writer: TxIdx, wave: &WaveParkTable) {
        self.mark_done(writer);
        if !self.has_any_gated() {
            return;
        }
        if !self.has_known_waiters(writer)
            && !self.was_queued(writer)
            && self.deferred_n.load(Ordering::Relaxed) == 0
        {
            return;
        }
        let waiters = self.waiters.remove(&writer);
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
        if self.deferred_n.load(Ordering::Relaxed) == 0 {
            return;
        }
        let mut d = self.deferred.lock().unwrap();
        if d.is_empty() {
            self.deferred_n.store(0, Ordering::Relaxed);
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
        self.deferred_n.store(d.len(), Ordering::Relaxed);
    }

    /// Refuse only known consumers still gated by an unpublished producer.
    /// Stale bits (producer already flushed to NONE) must not refuse.
    /// Ungated / independent txs skip DashMap (OCC-class pick).
    /// PC-5 EV A0 override executes anyway (gated set only).
    #[inline]
    pub(crate) fn may_execute(&self, tx_idx: TxIdx) -> bool {
        if tx_idx == 0 {
            return true;
        }
        if !self.is_gated(tx_idx) {
            return true;
        }
        if self.a0_force.contains(&tx_idx) {
            return true;
        }
        match self.consumers.get(&tx_idx) {
            Some(e) => {
                let w = e.load(Ordering::Relaxed);
                w == NONE || w >= tx_idx || self.is_writer_done(w)
            }
            // Gated bit is stored *before* the consumer-map insert. Treat the
            // window as not-ready so an OCC-class steal cannot pass the edge.
            None => false,
        }
    }

    #[inline]
    pub(crate) fn blocking_producer(&self, tx_idx: TxIdx) -> Option<TxIdx> {
        self.consumers
            .get(&tx_idx)
            .map(|e| e.load(Ordering::Relaxed))
            .filter(|&w| w < tx_idx && !self.is_writer_done(w))
    }

    /// Defer a known consumer. Count once until the producer finishes —
    /// re-probing the same head must not spin `refuse_admit` (19606599 31k).
    /// PC-W1: mark sleeping so steal/index skip this head until pred Done.
    #[inline]
    pub(crate) fn defer(&self, tx_idx: TxIdx) {
        let t0 = profile_timing_enabled().then(Instant::now);
        let mut d = self.deferred.lock().unwrap();
        if d.iter().any(|&t| t == tx_idx) {
            self.sleeping.insert(tx_idx);
            return;
        }
        self.refuse.fetch_add(1, Ordering::Relaxed);
        self.sleeping.insert(tx_idx);
        d.push(tx_idx);
        self.deferred_n.store(d.len(), Ordering::Relaxed);
        drop(d);
        if let Some(t0) = t0 {
            self.add_refuse_ns(t0.elapsed().as_nanos() as u64);
        }
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
        self.deferred_n.store(d.len(), Ordering::Relaxed);
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
    fn pc5_force_optimistic_allows_execute() {
        let t = ReadyEdgeTable::new();
        t.note_consumer(8, 3);
        assert!(!t.may_execute(8));
        t.force_optimistic(8);
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

    #[test]
    fn ungated_may_execute_is_occ_class() {
        let t = ReadyEdgeTable::new();
        assert!(!t.is_gated(3));
        assert!(t.may_execute(3), "no dependency gate → OCC-class pick");
        t.note_unpublished(7, 1);
        assert!(
            !t.is_gated(3),
            "unpublished producer alone does not gate strangers"
        );
        t.note_consumer(3, 1);
        assert!(t.is_gated(3));
        assert!(t.has_any_gated());
        assert!(!t.may_execute(3));
        t.force_optimistic(3);
        assert!(
            t.may_execute(3),
            "EV A0 override still executes a gated consumer"
        );
    }

    #[test]
    fn idle_edge_skips_started_and_plants_after_clear() {
        let t = ReadyEdgeTable::new();
        t.note_started(31);
        assert!(
            !t.note_consumer_on_if_idle(31, 4, Some(0x32be)),
            "O3: started consumer must not gain a gate (done-stamp race)"
        );
        assert!(t.may_execute(31));
        t.clear_started(31);
        assert!(
            t.note_consumer_on_if_idle(31, 4, Some(0x32be)),
            "O3: incarnation retry may take the windowed edge"
        );
        assert_eq!(t.blocking_producer(31), Some(4));
    }
}
