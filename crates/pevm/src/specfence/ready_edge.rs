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

use super::wave::WaveParkTable;

const NONE: usize = usize::MAX;
/// Bitset words for ordered-admit gated txs (64×64 = 4096). Beyond this,
/// `is_gated` falls back to the consumer map.
const GATED_WORDS: usize = 64;
/// Cap the skip-sleeping set. Dual-path fetch_max must not park a
/// thousands-wide RAW fan into an O(n) wake scan (ERC-20 livelock).
const SLEEP_CAP: usize = 64;

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
    /// Product-path wall-clock stall start per gated hole (S2).
    stall_start: DashMap<TxIdx, Instant, BuildIdentityHasher>,
    /// Product-path scheduler yield ns (P1). Not written into ĉ.
    yield_ns: AtomicU64,
    /// Dual-path pick census (P1).
    pick_occ_n: AtomicUsize,
    pick_gate_n: AtomicUsize,
    skip_gate_n: AtomicUsize,
    ungated_occ_while_gated: AtomicUsize,
    /// Producer → known consumers (completion event → bag; no DashMap scan).
    waiters: DashMap<TxIdx, Vec<TxIdx>, BuildIdentityHasher>,
    /// Atomic live location-waiter count. `queued_on.iter` in plant is racy
    /// (8 cores all see `< w_max`) and is a DashMap walk on the FullReplay path.
    loc_waiter_n: DashMap<MemoryLocationHash, AtomicUsize, BuildIdentityHasher>,
    /// Newest location-admitted waiter on `ℓ` (overflow chain start).
    loc_tip: DashMap<MemoryLocationHash, AtomicUsize, BuildIdentityHasher>,
    /// Last overflow consumer on `ℓ`. CAS-extended — never walk `waiters`.
    overflow_tip: DashMap<MemoryLocationHash, AtomicUsize, BuildIdentityHasher>,
    /// Lowest leftover writer seen on `ℓ` after the original peer published.
    /// Stops a reverse-order election cascade (later tip, then every earlier
    /// leftover also executing — 19807137 ~12 Q_released).
    leftover_min: DashMap<MemoryLocationHash, AtomicUsize, BuildIdentityHasher>,
    /// Block-wide leftover chain. 19807137 planted 23 locs × 1 tip and the
    /// tips milled each other (`live_wait=false`, pending≈23).
    global_leftover_min: AtomicUsize,
    global_leftover_chain: AtomicUsize,
    /// Leftover writers that called `plant_global_leftover`. Bitset — DashSet
    /// on the may_execute pick path heap-aborted 6196166 (`double free`).
    leftover_bits: [AtomicU64; GATED_WORDS],
    /// Ordered-admit gated txs. Marked **before** the consumer map insert so
    /// an ungated steal cannot race past a newly created wait-for edge.
    gated_bits: [AtomicU64; GATED_WORDS],
    /// Live gated-tx count (Detect edges). Zero gated → RunnableSet is the
    /// independent antichain (Avoid=noop Opt), still on Schedule.pick.
    gated_n: AtomicUsize,
    /// Gated txs that have not yet `mark_done`. Gates are edge constraints:
    /// when this hits 0 those txs rejoin the antichain — not a retreat to
    /// `next_occ_task`.
    pending_gated: AtomicUsize,
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
            stall_start: DashMap::default(),
            yield_ns: AtomicU64::new(0),
            pick_occ_n: AtomicUsize::new(0),
            pick_gate_n: AtomicUsize::new(0),
            skip_gate_n: AtomicUsize::new(0),
            ungated_occ_while_gated: AtomicUsize::new(0),
            waiters: DashMap::default(),
            loc_waiter_n: DashMap::default(),
            loc_tip: DashMap::default(),
            overflow_tip: DashMap::default(),
            leftover_min: DashMap::default(),
            global_leftover_min: AtomicUsize::new(NONE),
            global_leftover_chain: AtomicUsize::new(NONE),
            leftover_bits: std::array::from_fn(|_| AtomicU64::new(0)),
            gated_bits: std::array::from_fn(|_| AtomicU64::new(0)),
            gated_n: AtomicUsize::new(0),
            pending_gated: AtomicUsize::new(0),
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
        {
            let mut w = self.waiters.entry(producer).or_default();
            if !w.iter().any(|&c| c == consumer) {
                w.push(consumer);
            }
        }
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
    ///
    /// Must not call [`may_execute`] while the `consumers` iter is live —
    /// DashMap is not reentrant (same-map `iter` + `get` corrupts the heap).
    pub(crate) fn blocked_consumers(&self) -> Vec<TxIdx> {
        let mut out: Vec<TxIdx> = self
            .consumers
            .iter()
            .filter_map(|e| {
                let t = *e.key();
                if t == 0 || self.a0_force.contains(&t) {
                    return None;
                }
                let w = e.value().load(Ordering::Relaxed);
                let blocked = w != NONE && w < t && !self.is_writer_done(w);
                blocked.then_some(t)
            })
            .collect();
        out.sort_unstable();
        out
    }

    #[inline]
    pub(crate) fn was_queued(&self, tx: TxIdx) -> bool {
        // Consumers / location-queue only. `waiters.contains(tx)` means this
        // tx is a *producer* with dependents — those used to land on
        // Q_ordered and mill (19807137: ~58 Ordered heads, live_wait=false).
        self.queued_on.contains_key(&tx) || self.consumers.contains_key(&tx)
    }

    /// Location-admitted Detect/Win cohort. Anonymous fan-in
    /// (`note_consumer_on(..., None)`) is `was_queued` but must not
    /// take Q_ordered / OrderedTip — that milled 19807137 (~40 spine
    /// consumers). Heal/drain send those Released or Indep.
    #[inline]
    pub(crate) fn admitted_on_location(&self, tx: TxIdx) -> bool {
        self.queued_on.contains_key(&tx)
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
                self.pending_gated.fetch_add(1, Ordering::Relaxed);
            }
        } else if !self.consumers.contains_key(&tx) {
            self.gated_n.fetch_add(1, Ordering::Relaxed);
            self.pending_gated.fetch_add(1, Ordering::Relaxed);
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

    /// Any live Detect-gated tx in this block. False ⇒ RunnableSet is the
    /// independent antichain (Avoid=noop Opt), not an OCC-engine switch.
    #[inline]
    pub(crate) fn has_any_gated(&self) -> bool {
        self.gated_n.load(Ordering::Relaxed) > 0
    }

    /// Unfinished Detect wait-set. Cleared as each gated tx `mark_done`s
    /// so those txs rejoin the RunnableSet antichain (Avoid=noop Opt).
    /// This is **not** a retreat to `next_occ_task`.
    #[inline]
    pub(crate) fn has_pending_gated(&self) -> bool {
        self.pending_gated.load(Ordering::Relaxed) > 0
    }

    /// Live gated-tx count (Detect edges).
    #[inline]
    pub(crate) fn gated_count(&self) -> usize {
        self.gated_n.load(Ordering::Relaxed)
    }

    /// Unfinished gated txs still in the wait-set.
    #[inline]
    pub(crate) fn pending_gated_count(&self) -> usize {
        self.pending_gated.load(Ordering::Relaxed)
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
        if self.is_gated(tx) {
            self.note_stall_end(tx);
            self.pick_gate_n.fetch_add(1, Ordering::Relaxed);
        } else if self.has_any_gated() {
            self.pick_occ_n.fetch_add(1, Ordering::Relaxed);
            self.ungated_occ_while_gated.fetch_add(1, Ordering::Relaxed);
        } else {
            self.pick_occ_n.fetch_add(1, Ordering::Relaxed);
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
        let newly = {
            let i = writer / 64;
            if i < self.done_bits.len() {
                let bit = 1u64 << (writer % 64);
                let prev = self.done_bits[i].fetch_or(bit, Ordering::Release);
                prev & bit == 0
            } else {
                self.finished.insert(writer, ()).is_none()
            }
        };
        if newly && self.is_gated(writer) {
            let mut cur = self.pending_gated.load(Ordering::Relaxed);
            while cur > 0 {
                match self.pending_gated.compare_exchange_weak(
                    cur,
                    cur - 1,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(v) => cur = v,
                }
            }
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

    /// FullReplay / OrderedReplay: this writer is no longer published.
    /// Clear the done stamp so later plants are not no-ops. Do **not**
    /// walk waiters/consumers here — that raced DashMap (19469101 SEGV,
    /// 19807137 unaligned tcache).
    pub(crate) fn note_abort_reincarnate(&self, writer: TxIdx) {
        let i = writer / 64;
        let was_done = if i < self.done_bits.len() {
            let bit = 1u64 << (writer % 64);
            self.done_bits[i].fetch_and(!bit, Ordering::Release) & bit != 0
        } else {
            self.finished.remove(&writer).is_some()
        };
        if was_done && self.is_gated(writer) {
            self.pending_gated.fetch_add(1, Ordering::Relaxed);
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

    /// Product-path yield ns (P1 busy/idle). Never written into ĉ.
    #[inline]
    pub(crate) fn add_yield_ns(&self, ns: u64) {
        if ns > 0 {
            self.yield_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn yield_ns(&self) -> u64 {
        self.yield_ns.load(Ordering::Relaxed)
    }

    /// Skip a gated-not-ready hole once: lock-free sleeping + wall-clock stall start.
    /// No deferred mutex — wake is the waiter map on producer Done.
    pub(crate) fn note_skip_gate(&self, tx_idx: TxIdx) {
        if self.sleeping.contains(&tx_idx) {
            return;
        }
        // Bound the sleeper set so a slipped-through RAW fan cannot turn
        // every yield into an O(n) DashMap walk.
        if self.sleeping.len() < SLEEP_CAP {
            self.sleeping.insert(tx_idx);
        }
        self.refuse.fetch_add(1, Ordering::Relaxed);
        self.skip_gate_n.fetch_add(1, Ordering::Relaxed);
        self.stall_start.entry(tx_idx).or_insert_with(Instant::now);
    }

    #[inline]
    pub(crate) fn has_sleeping_waiters(&self) -> bool {
        !self.sleeping.is_empty()
    }

    /// Self-heal: producer already Done but wave miss — push sleepers that
    /// `may_execute` so dual-path pick cannot hang on a stale hole.
    pub(crate) fn wake_ready_sleepers(&self, wave: &WaveParkTable) -> usize {
        if self.sleeping.is_empty() {
            return 0;
        }
        // Producer waiters are the real wake path. This scan is a self-heal
        // for a small hole set — never a full-envelope walk.
        let ready: Vec<TxIdx> = self
            .sleeping
            .iter()
            .take(SLEEP_CAP)
            .filter_map(|t| {
                let t = *t;
                self.may_execute(t).then_some(t)
            })
            .collect();
        for &t in &ready {
            self.sleeping.remove(&t);
            self.note_stall_end(t);
            wave.push_ready(t);
        }
        ready.len()
    }

    fn note_stall_end(&self, tx_idx: TxIdx) {
        if let Some((_, t0)) = self.stall_start.remove(&tx_idx) {
            let ns = t0.elapsed().as_nanos() as u64;
            if ns > 0 {
                // Wall prepaid is the **max** hole stall (makespan), not the
                // sum of overlapping Instants from block-start (S2).
                let mut cur = self.refuse_ns.load(Ordering::Relaxed);
                while ns > cur {
                    match self.refuse_ns.compare_exchange_weak(
                        cur,
                        ns,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(v) => cur = v,
                    }
                }
            }
        }
    }

    #[inline]
    pub(crate) fn pick_occ_n(&self) -> usize {
        self.pick_occ_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn pick_gate_n(&self) -> usize {
        self.pick_gate_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn skip_gate_n(&self) -> usize {
        self.skip_gate_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn ungated_occ_while_gated(&self) -> usize {
        self.ungated_occ_while_gated.load(Ordering::Relaxed)
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
        let cs = self
            .waiters
            .remove(&writer)
            .map(|(_, v)| v)
            .unwrap_or_default();
        let had_waiters = !cs.is_empty();
        // leftover_min is a claim token, not a Detect wait-set. Elect the
        // next leftover from leftover_claimed — waiters of leftover_min
        // used to be surplus Detect-stars (19807137 leftover_min=405).
        self.clear_leftover_claim(writer);
        let leftover_wake = if self.global_leftover_min.load(Ordering::Relaxed) == writer {
            let next = self.elect_next_leftover();
            let _ = self.global_leftover_min.compare_exchange(
                writer,
                next,
                Ordering::Release,
                Ordering::Relaxed,
            );
            if next != NONE {
                self.global_leftover_chain.store(next, Ordering::Relaxed);
            }
            next
        } else {
            self.live_leftover_min()
        };
        self.mark_done(writer);
        // Claim token: wake leftover_min even when no Detect gates exist.
        if leftover_wake != NONE && leftover_wake != writer && self.may_execute(leftover_wake) {
            wave.push_ready(leftover_wake);
        }
        if !self.has_any_gated() {
            return;
        }
        if !had_waiters && !self.was_queued(writer) && self.deferred_n.load(Ordering::Relaxed) == 0
        {
            return;
        }
        // Keep the waiter list so FullReplay can re-block them. Removing
        // it made Commit+revalidate abort a permanent may_execute stampede
        // (19807137 ~40 Released heads, live_wait=false).
        for c in cs {
            // Only the consumers still gated on *this* writer. Probe-star
            // leftovers that already rebased onto a later pred must stay.
            //
            // DashMap is not reentrant: drop the `consumers` shard
            // *before* `may_execute`, which also `consumers.get`. Nested
            // get on the same shard is the heap-abort after
            // `plant_observed_waw` increased waiter traffic
            // (`free(): invalid pointer` / `corrupted size vs. prev_size`).
            let still_mine = match self.consumers.get(&c) {
                Some(e) if e.load(Ordering::Relaxed) == writer => {
                    e.store(NONE, Ordering::Relaxed);
                    true
                }
                _ => false,
            };
            if !still_mine {
                continue;
            }
            self.sleeping.remove(&c);
            // Do not note_consumer_on here — DashMap re-entry on the wake
            // path SEGVd 19807137 after leftover_min started cycling.
            // First-wave already joined leftover election at plant time.
            if self.may_execute(c) {
                wave.push_ready(c);
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
        if self.leftover_surplus(tx_idx) {
            return false;
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
            // PC-5: leftover gated bit without a consumer map is not a
            // refuse. The mark_gated→insert race is over after admit_seed.
            None => true,
        }
    }

    #[inline]
    pub(crate) fn blocking_producer(&self, tx_idx: TxIdx) -> Option<TxIdx> {
        self.consumers
            .get(&tx_idx)
            .map(|e| e.load(Ordering::Relaxed))
            .filter(|&w| w < tx_idx && !self.is_writer_done(w))
    }

    /// Detect pred chain reaches `target`. Used to break add_dependency
    /// cycles: later leftover waits on us, we parked Aborting on them.
    pub(crate) fn detect_waits_on(&self, mut tx: TxIdx, target: TxIdx) -> bool {
        for _ in 0..8 {
            match self.blocking_producer(tx) {
                Some(p) if p == target => return true,
                Some(p) if p < tx => tx = p,
                _ => return false,
            }
        }
        false
    }

    /// Flush `consumer ← producer` without `ungate` (that DashMap walk
    /// double-freed 6196166 on the Blocked path).
    pub(crate) fn flush_pred_if(&self, consumer: TxIdx, producer: TxIdx) {
        if let Some(e) = self.consumers.get(&consumer)
            && e.load(Ordering::Relaxed) == producer
        {
            e.store(NONE, Ordering::Relaxed);
        }
    }

    /// Walk the Detect chain and flush the edge that names `target`.
    pub(crate) fn flush_wait_on(&self, mut tx: TxIdx, target: TxIdx) {
        for _ in 0..8 {
            match self.blocking_producer(tx) {
                Some(p) if p == target => {
                    self.flush_pred_if(tx, target);
                    return;
                }
                Some(p) if p < tx => tx = p,
                _ => return,
            }
        }
    }

    /// Defer a known consumer. Count once until the producer finishes —
    /// re-probing the same head must not spin `refuse_admit` (19606599 31k).
    /// PC-W1: mark sleeping so steal/index skip this head until pred Done.
    /// Instant-in-defer is **not** refuse_ns (S2: wall-clock stall only).
    #[inline]
    pub(crate) fn defer(&self, tx_idx: TxIdx) {
        self.note_skip_gate(tx_idx);
        let mut d = self.deferred.lock().unwrap();
        if d.iter().any(|&t| t == tx_idx) {
            return;
        }
        d.push(tx_idx);
        self.deferred_n.store(d.len(), Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn is_sleeping(&self, tx_idx: TxIdx) -> bool {
        self.sleeping.contains(&tx_idx) && !self.may_execute(tx_idx)
    }

    /// O6: true when some sleeper is blocked on a producer that is Executing.
    #[inline]
    pub(crate) fn sleeper_pred_busy(&self, mut is_executing: impl FnMut(TxIdx) -> bool) -> bool {
        self.sleeping
            .iter()
            .take(8)
            .any(|t| self.blocking_producer(*t).is_some_and(|w| is_executing(w)))
    }

    /// Consumers queued on `ℓ` (IntraPatch must not move strangers).
    pub(crate) fn consumers_queued_on(&self, location: MemoryLocationHash) -> Vec<TxIdx> {
        let mut out: Vec<TxIdx> = self
            .queued_on
            .iter()
            .filter_map(|e| (*e.value() == location).then_some(*e.key()))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Live Detect waiters bound to `ℓ` (no sort — plant-cap hot path).
    #[inline]
    pub(crate) fn consumer_count_on(&self, location: MemoryLocationHash) -> usize {
        self.queued_on
            .iter()
            .filter(|e| *e.value() == location)
            .count()
    }

    /// Hang-trace: leftover_min / loc_tip / overflow_tip occupancy.
    #[inline]
    pub(crate) fn hang_plant_n(&self) -> (usize, usize, usize) {
        (
            self.leftover_min.len(),
            self.loc_tip.len(),
            self.overflow_tip.len(),
        )
    }

    /// Hang-trace: global leftover election (`NONE` → `usize::MAX`).
    #[inline]
    pub(crate) fn hang_global_leftover(&self) -> (usize, usize) {
        (
            self.global_leftover_min.load(Ordering::Relaxed),
            self.global_leftover_chain.load(Ordering::Relaxed),
        )
    }

    /// Hang-trace: leftover-min done / may_execute / blocking pred.
    #[inline]
    pub(crate) fn hang_leftover_min_status(&self) -> (bool, bool, usize) {
        let min = self.global_leftover_min.load(Ordering::Relaxed);
        if min == NONE {
            return (true, true, NONE);
        }
        (
            self.is_writer_done(min),
            self.may_execute(min),
            self.blocking_producer(min).unwrap_or(NONE),
        )
    }

    /// Newest still-live waiter reachable from `start` with index `< before`.
    /// Overflow plants must chain, not star on one tip — a star wakes
    /// every leftover writer at once (19807137 ~40 Released mill).
    pub(crate) fn chain_tip_before(&self, start: TxIdx, before: TxIdx) -> TxIdx {
        let mut pred = start;
        for _ in 0..64 {
            let next = {
                let Some(w) = self.waiters.get(&pred) else {
                    break;
                };
                w.iter()
                    .copied()
                    .filter(|&c| c > pred && c < before && !self.is_writer_done(c))
                    .max()
            };
            let Some(n) = next else {
                break;
            };
            pred = n;
        }
        pred
    }

    /// Mid-block observed-WAW plant. Stops 2-writer Opt ping-pong without
    /// deepening a spine (producer itself waiting). Caller evicts surplus
    /// waiters on `ℓ` after insert so a token/storage fan stays ≤ `w_max`.
    pub(crate) fn should_plant_observed_waw(&self, consumer: TxIdx, producer: TxIdx) -> bool {
        if producer >= consumer || self.is_writer_done(producer) {
            return false;
        }
        if self
            .blocking_producer(consumer)
            .is_some_and(|w| w >= producer)
        {
            return false;
        }
        // Always plant if the producer is unfinished. Skipping when the
        // pred is itself waiting left leftover Opt writers ping-ponging
        // (~390% on 19469101). Depth is capped by w_max + overflow chain.
        true
    }

    /// Cap location waiters at `w_max` with an atomic slot, then chain
    /// surplus onto `overflow_tip`. After the original peer publishes,
    /// leftover writers elect one live tip **on this ℓ** and chain.
    ///
    /// A block-wide leftover chain (every FullReplay → `plant_global` +
    /// drain `gate_on_live_leftover`) serialized 6196166 onto leftover
    /// min=18 (`n_unf=75`, `live_wait=true`) and DashMap-rebound first-wave
    /// wakeups into a complete_arch heap abort. Cross-ℓ leftovers stay
    /// independent; shared storage still serializes via `plant_invalid_locs`.
    /// Does not walk `waiters` / `queued_on` (those munmap'd FullReplay).
    pub(crate) fn plant_observed_window(
        &self,
        consumer: TxIdx,
        producer: TxIdx,
        location: MemoryLocationHash,
        w_max: usize,
    ) -> bool {
        if consumer == 0 {
            return false;
        }
        let w_max = w_max.max(1);
        let producer_live = producer < consumer && !self.is_writer_done(producer);
        if producer_live {
            let got_slot = {
                let n = self
                    .loc_waiter_n
                    .entry(location)
                    .or_insert_with(|| AtomicUsize::new(0));
                loop {
                    let cur = n.load(Ordering::Relaxed);
                    if cur >= w_max {
                        break false;
                    }
                    match n.compare_exchange_weak(
                        cur,
                        cur + 1,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break true,
                        Err(_) => {}
                    }
                }
            };
            if got_slot {
                self.note_consumer_on(consumer, producer, Some(location));
                {
                    let e = self
                        .loc_tip
                        .entry(location)
                        .or_insert_with(|| AtomicUsize::new(consumer));
                    e.fetch_max(consumer, Ordering::Relaxed);
                }
                return true;
            }
            let loc_tip = self
                .loc_tip
                .get(&location)
                .map(|e| e.load(Ordering::Relaxed))
                .filter(|&t| t < consumer)
                .unwrap_or(producer);
            return self.chain_overflow(consumer, location, loc_tip, producer);
        }
        // Leftover: original peer already published. Prefer a live first-wave
        // loc_tip; else elect leftover_min on this ℓ only.
        let loc_tip = self
            .loc_tip
            .get(&location)
            .map(|e| e.load(Ordering::Relaxed))
            .filter(|&t| t < consumer)
            .unwrap_or(NONE);
        if loc_tip != NONE && !self.is_writer_done(loc_tip) {
            return self.chain_overflow(consumer, location, loc_tip, loc_tip);
        }
        self.plant_leftover_on_loc(consumer, location)
    }

    /// Overflow chain: wait on the newest live overflow / loc_tip / fallback.
    /// Snapshot preds before `overflow_tip.entry` — DashMap is not reentrant.
    fn chain_overflow(
        &self,
        consumer: TxIdx,
        location: MemoryLocationHash,
        loc_tip: TxIdx,
        fallback: TxIdx,
    ) -> bool {
        let pred = {
            let e = self
                .overflow_tip
                .entry(location)
                .or_insert_with(|| AtomicUsize::new(NONE));
            loop {
                let cur = e.load(Ordering::Relaxed);
                if cur == consumer {
                    break None;
                }
                let cand = if cur != NONE && cur < consumer && !self.is_writer_done(cur) {
                    Some(cur)
                } else if loc_tip < consumer && !self.is_writer_done(loc_tip) {
                    Some(loc_tip)
                } else if fallback < consumer && !self.is_writer_done(fallback) {
                    Some(fallback)
                } else {
                    None
                };
                match e.compare_exchange_weak(cur, consumer, Ordering::Release, Ordering::Relaxed) {
                    Ok(_) => break cand,
                    Err(_) => {}
                }
            }
        };
        match pred {
            Some(p) if self.should_plant_observed_waw(consumer, p) => {
                self.note_consumer_on(consumer, p, None);
                true
            }
            _ => false,
        }
    }

    /// Per-ℓ leftover election. Replaces a done min (fetch_min-only stayed
    /// sticky-done and re-armed the Released mill). Steal rebind happens
    /// after the leftover_min shard is dropped.
    fn plant_leftover_on_loc(&self, consumer: TxIdx, location: MemoryLocationHash) -> bool {
        if self.is_writer_done(consumer) {
            return false;
        }
        let (min, stolen) = {
            let e = self
                .leftover_min
                .entry(location)
                .or_insert_with(|| AtomicUsize::new(NONE));
            loop {
                let cur = e.load(Ordering::Relaxed);
                let cur_live = cur != NONE && !self.is_writer_done(cur);
                if cur_live && cur <= consumer {
                    break (cur, None);
                }
                match e.compare_exchange_weak(cur, consumer, Ordering::Release, Ordering::Relaxed) {
                    Ok(_) => {
                        let stolen = if cur != NONE && cur > consumer && !self.is_writer_done(cur) {
                            Some(cur)
                        } else {
                            None
                        };
                        break (consumer, stolen);
                    }
                    Err(_) => {}
                }
            }
        };
        if let Some(old) = stolen {
            self.rebind_stolen_leftover_head(old, consumer);
        }
        if min == consumer || min == NONE || self.is_writer_done(min) {
            return stolen.is_some();
        }
        self.chain_overflow(consumer, location, min, min)
    }

    /// Previous leftover head stays Indep after a lower writer steals `min`.
    /// Both then Opt-execute (19807137 leftover mill / nuclear 32-head mill).
    fn rebind_stolen_leftover_head(&self, old_head: TxIdx, new_min: TxIdx) {
        if old_head <= new_min || self.is_writer_done(new_min) {
            return;
        }
        self.note_consumer_on(old_head, new_min, None);
    }

    /// Live leftover head only. A sticky-done min makes every later plant
    /// a no-op (`should_plant` / wake-rebind skip) and re-arms the Released mill.
    /// After a producer Done wake, bind the waiter onto the live leftover
    /// head. Kept for leftover-election unit tests; the product drain path
    /// no longer rebinds first-wave onto a block-wide leftover min.
    #[allow(dead_code)]
    pub(crate) fn gate_on_live_leftover(&self, tx: TxIdx) -> bool {
        let min = self.live_leftover_min();
        if min == NONE || min >= tx {
            return false;
        }
        self.note_consumer_on(tx, min, None);
        !self.may_execute(tx)
    }

    fn live_leftover_min(&self) -> usize {
        let m = self.global_leftover_min.load(Ordering::Relaxed);
        if m == NONE || self.is_writer_done(m) {
            NONE
        } else {
            m
        }
    }

    fn leftover_claim_bit(tx: TxIdx) -> Option<(usize, u64)> {
        let i = tx / 64;
        (i < GATED_WORDS).then_some((i, 1u64 << (tx % 64)))
    }

    fn mark_leftover_claim(&self, tx: TxIdx) {
        if let Some((i, bit)) = Self::leftover_claim_bit(tx) {
            self.leftover_bits[i].fetch_or(bit, Ordering::Release);
        }
    }

    fn clear_leftover_claim(&self, tx: TxIdx) {
        if let Some((i, bit)) = Self::leftover_claim_bit(tx) {
            self.leftover_bits[i].fetch_and(!bit, Ordering::Release);
        }
    }

    fn leftover_claim_has(&self, tx: TxIdx) -> bool {
        Self::leftover_claim_bit(tx)
            .is_some_and(|(i, bit)| self.leftover_bits[i].load(Ordering::Acquire) & bit != 0)
    }

    fn elect_next_leftover(&self) -> usize {
        for (wi, word) in self.leftover_bits.iter().enumerate() {
            let mut bits = word.load(Ordering::Acquire);
            while bits != 0 {
                let b = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let c = wi * 64 + b;
                if !self.is_writer_done(c) {
                    return c;
                }
            }
        }
        NONE
    }

    /// leftover_min stayed sticky after Commit when the wake was lost
    /// (`19807137` glob_min=184 min_exec=true pending=0 n_unf=527).
    /// Advance the claim and return the next leftover to requeue.
    pub(crate) fn take_finished_leftover_min(
        &self,
        mut finished: impl FnMut(TxIdx) -> bool,
    ) -> Option<TxIdx> {
        let min = self.global_leftover_min.load(Ordering::Relaxed);
        if min == NONE {
            return None;
        }
        if !self.is_writer_done(min) && !finished(min) {
            return None;
        }
        self.clear_leftover_claim(min);
        if !self.is_writer_done(min) {
            self.mark_done(min);
        }
        let next = self.elect_next_leftover();
        let _ = self.global_leftover_min.compare_exchange(
            min,
            next,
            Ordering::Release,
            Ordering::Relaxed,
        );
        if next != NONE {
            self.global_leftover_chain.store(next, Ordering::Relaxed);
            Some(next)
        } else {
            None
        }
    }

    /// Live leftover claim that still may_execute (lost-queue recover).
    pub(crate) fn live_leftover_head(&self) -> Option<TxIdx> {
        let min = self.live_leftover_min();
        (min != NONE).then_some(min)
    }

    /// Surplus leftover: claimed after leftover_min, not Detect-starred.
    #[inline]
    pub(crate) fn leftover_surplus(&self, tx: TxIdx) -> bool {
        let min = self.live_leftover_min();
        min != NONE && min < tx && self.leftover_claim_has(tx)
    }

    #[inline]
    pub(crate) fn is_leftover_claimed(&self, tx: TxIdx) -> bool {
        self.leftover_claim_has(tx)
    }

    #[inline]
    pub(crate) fn is_live_leftover_min(&self, tx: TxIdx) -> bool {
        self.live_leftover_min() == tx
    }

    fn install_leftover_min(&self, consumer: TxIdx) {
        if self.is_writer_done(consumer) {
            return;
        }
        loop {
            let cur = self.global_leftover_min.load(Ordering::Relaxed);
            let cur_live = cur != NONE && !self.is_writer_done(cur);
            if cur_live && cur <= consumer {
                return;
            }
            match self.global_leftover_min.compare_exchange_weak(
                cur,
                consumer,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.global_leftover_chain
                        .store(consumer, Ordering::Relaxed);
                    return;
                }
                Err(_) => {}
            }
        }
    }

    /// One leftover writer executes at a time. Surplus is claim-refused.
    /// Detect-star **only the next leftover** onto leftover_min — not the
    /// whole wait-set (19807137 leftover_min=405 n_unf=307). Dropping
    /// Detect entirely heap-aborted 6196166. Returns true when surplus.
    pub(crate) fn plant_global_leftover(&self, consumer: TxIdx) -> bool {
        if consumer == 0 || self.is_writer_done(consumer) {
            return false;
        }
        self.mark_leftover_claim(consumer);
        self.install_leftover_min(consumer);
        let min = self.live_leftover_min();
        if min == NONE || min >= consumer {
            return false;
        }
        if self.next_leftover_after(min) == Some(consumer)
            && self.should_plant_observed_waw(consumer, min)
        {
            self.note_consumer_on(consumer, min, None);
        }
        true
    }

    fn next_leftover_after(&self, min: TxIdx) -> Option<TxIdx> {
        for (wi, word) in self.leftover_bits.iter().enumerate() {
            let mut bits = word.load(Ordering::Acquire);
            while bits != 0 {
                let b = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let c = wi * 64 + b;
                if c > min && !self.is_writer_done(c) {
                    return Some(c);
                }
            }
        }
        None
    }

    /// Keep at most `w_max` Detect waiters on `ℓ`. Evict oldest first,
    /// never `keep` (the aborting consumer that just planted). Returns
    /// ungated txs — caller must requeue them onto `Q_indep`.
    pub(crate) fn evict_surplus_waiters(
        &self,
        location: MemoryLocationHash,
        keep: TxIdx,
        w_max: usize,
    ) -> Vec<TxIdx> {
        let queued = self.consumers_queued_on(location);
        if queued.len() <= w_max {
            return Vec::new();
        }
        let surplus = queued.len() - w_max;
        let mut evicted = Vec::new();
        for &c in &queued {
            if evicted.len() >= surplus {
                break;
            }
            if c == keep {
                continue;
            }
            self.ungate(c);
            evicted.push(c);
        }
        evicted
    }

    /// PC-5: drop gates whose producer is gone or already published so
    /// `pending_gated>0` cannot exist with an empty RunnableSet.
    /// Returns the txs that became executable.
    pub(crate) fn collapse_false_gates(
        &self,
        mut producer_live: impl FnMut(TxIdx) -> bool,
    ) -> Vec<TxIdx> {
        let mut freed = Vec::new();
        let candidates: Vec<TxIdx> = self
            .consumers
            .iter()
            .map(|e| *e.key())
            .chain(self.sleeping.iter().map(|t| *t))
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        for tx in candidates {
            if !seen.insert(tx) {
                continue;
            }
            if !self.is_gated(tx) {
                continue;
            }
            let live = self.blocking_producer(tx).is_some_and(&mut producer_live);
            if live {
                continue;
            }
            self.ungate(tx);
            freed.push(tx);
        }
        freed
    }

    /// I1: stamp preds that the scheduler already published. A pick-quantum
    /// flush can insert `consumer←pred` after O5 skipped the Done bit —
    /// `may_execute` stays false and workers yield-spin (~400% CPU).
    pub(crate) fn heal_finished_preds(&self, mut finished: impl FnMut(TxIdx) -> bool) {
        let mut preds: Vec<TxIdx> = self
            .sleeping
            .iter()
            .take(SLEEP_CAP)
            .filter_map(|t| self.blocking_producer(*t))
            .filter(|&w| finished(w))
            .collect();
        // Detect-gated waiters are often ST_WAIT without a sleeping bit
        // (RunnableSet refuse does not always call `note_skip_gate`).
        // Snapshot first — do not hold `consumers.iter` across other maps.
        if preds.is_empty() && !self.consumers.is_empty() {
            let snapshot: Vec<(TxIdx, TxIdx)> = self
                .consumers
                .iter()
                .map(|e| (*e.key(), e.value().load(Ordering::Relaxed)))
                .collect();
            for (c, w) in snapshot {
                if w < c && !self.is_writer_done(w) && finished(w) {
                    preds.push(w);
                }
            }
        }
        preds.sort_unstable();
        preds.dedup();
        for w in preds {
            self.mark_done(w);
        }
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

    /// Drop a wait-set entry without a wave (pre-worker soft-cap).
    /// Clears the gated bit **and** ReadyEdge membership so `was_queued`
    /// is false — P3 cap-0 must not leave dropped txs on the SpecFence
    /// execute/validate path (13287210 19× after plant-then-drop).
    pub(crate) fn ungate(&self, tx: TxIdx) {
        let i = tx / 64;
        let was = if i < self.gated_bits.len() {
            let bit = 1u64 << (tx % 64);
            let prev = self.gated_bits[i].fetch_and(!bit, Ordering::Release);
            prev & bit != 0
        } else {
            self.consumers.contains_key(&tx)
        };
        if was {
            let mut g = self.gated_n.load(Ordering::Relaxed);
            while g > 0 {
                match self.gated_n.compare_exchange_weak(
                    g,
                    g - 1,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(v) => g = v,
                }
            }
            if !self.is_writer_done(tx) {
                let mut p = self.pending_gated.load(Ordering::Relaxed);
                while p > 0 {
                    match self.pending_gated.compare_exchange_weak(
                        p,
                        p - 1,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(v) => p = v,
                    }
                }
            }
        }
        let pred = self.consumers.remove(&tx).and_then(|(_, e)| {
            let p = e.load(Ordering::Relaxed);
            (p != NONE).then_some(p)
        });
        self.queued_on.remove(&tx);
        self.sleeping.remove(&tx);
        self.a0_force.remove(&tx);
        if let Some(p) = pred
            && let Some(mut w) = self.waiters.get_mut(&p)
        {
            w.retain(|&c| c != tx);
            let empty = w.is_empty();
            drop(w);
            if empty {
                self.waiters.remove(&p);
            }
        }
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
        assert!(t.has_pending_gated());
        assert!(!t.may_execute(3));
        t.note_producer_done_stamp(3);
        assert!(
            !t.has_pending_gated(),
            "S1: finished hole returns pick to OCC"
        );
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

    #[test]
    fn done_stamp_prevents_late_flush_refuse_forever() {
        let t = ReadyEdgeTable::new();
        t.note_producer_done_stamp(24);
        t.note_consumer_on(25, 24, Some(0xabc));
        assert!(
            t.may_execute(25),
            "I1: flush after success stamp must not gate the successor"
        );
        assert!(t.blocking_producer(25).is_none());
    }

    #[test]
    fn heal_finished_preds_unsticks_late_plant() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_consumer_on(25, 24, Some(0xabc));
        t.note_skip_gate(25);
        assert!(!t.may_execute(25));
        assert!(t.has_sleeping_waiters());
        t.heal_finished_preds(|w| w == 24);
        assert!(
            t.may_execute(25),
            "I1: scheduler-Done pred must heal the refuse-forever bit"
        );
        assert_eq!(t.wake_ready_sleepers(&wave), 1);
        assert_eq!(wave.pop_ready(), Some(25));
    }

    #[test]
    fn ungate_clears_wait_set_entry() {
        let t = ReadyEdgeTable::new();
        t.note_consumer(9, 1);
        assert!(t.is_gated(9));
        assert!(t.has_pending_gated());
        assert!(t.was_queued(9));
        assert!(
            !t.was_queued(1),
            "producer-with-waiters is not Q_ordered membership"
        );
        t.ungate(9);
        assert!(!t.is_gated(9), "ungate drops the wait-for constraint");
        assert!(t.may_execute(9));
        assert!(
            !t.has_pending_gated(),
            "ungate returns pick to ungated OCC task selection"
        );
        assert!(
            !t.was_queued(9) && !t.was_queued(1),
            "P3 cap-drop must clear ReadyEdge membership so ungated execute is OCC-equivalent"
        );
    }

    #[test]
    fn leftover_gated_bit_without_consumer_is_not_refuse() {
        let t = ReadyEdgeTable::new();
        t.mark_gated(7);
        assert!(t.is_gated(7));
        assert!(
            t.may_execute(7),
            "PC-5: leftover gated bit without a consumer map is not a refuse"
        );
        t.ungate(7);
        assert!(!t.is_gated(7));
        assert!(!t.has_pending_gated());
    }

    #[test]
    fn collapse_false_gates_frees_dead_producer() {
        let t = ReadyEdgeTable::new();
        t.note_consumer(3, 1);
        assert!(!t.may_execute(3));
        let freed = t.collapse_false_gates(|_| false);
        assert!(freed.contains(&3));
        assert!(t.may_execute(3));
        assert!(!t.is_gated(3));
    }

    #[test]
    fn consumers_queued_on_is_this_location_only() {
        let t = ReadyEdgeTable::new();
        t.note_consumer_on(3, 0, Some(11));
        t.note_consumer_on(5, 1, Some(22));
        assert_eq!(t.consumers_queued_on(11), vec![3]);
        assert_eq!(t.consumers_queued_on(22), vec![5]);
        assert!(t.consumers_queued_on(99).is_empty());
    }

    #[test]
    fn producer_done_many_waiters_then_may_execute() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        for c in 1..64 {
            t.note_consumer(c, 0);
            assert!(!t.may_execute(c));
        }
        t.note_producer_done(0, &wave);
        for c in 1..64 {
            assert!(
                t.may_execute(c),
                "waiter {c} must execute after producer done (no nested DashMap get)"
            );
        }
        let mut woke = 0;
        while wave.pop_ready().is_some() {
            woke += 1;
        }
        assert_eq!(woke, 63);
    }

    #[test]
    fn producer_done_concurrent_with_may_execute() {
        use std::sync::Arc;
        let t = Arc::new(ReadyEdgeTable::new());
        let wave = Arc::new(WaveParkTable::new());
        for c in 1..48 {
            t.note_consumer(c, 0);
        }
        let mut handles = Vec::new();
        for _ in 0..8 {
            let t = t.clone();
            handles.push(std::thread::spawn(move || {
                for c in 1..48 {
                    let _ = t.may_execute(c);
                    let _ = t.blocking_producer(c);
                    let _ = t.blocked_consumers();
                }
            }));
        }
        t.note_producer_done(0, &wave);
        for h in handles {
            h.join().expect("no DashMap re-entry panic");
        }
        for c in 1..48 {
            assert!(t.may_execute(c));
        }
    }

    #[test]
    fn abort_clears_done_stamp() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_consumer_on(3, 1, Some(0xabc));
        t.note_producer_done(1, &wave);
        assert!(t.is_writer_done(1));
        t.note_abort_reincarnate(1);
        assert!(
            !t.is_writer_done(1),
            "FullReplay must clear done so later plants are not no-ops"
        );
        assert!(
            t.should_plant_observed_waw(4, 1),
            "cleared done stamp must allow a new plant"
        );
    }

    #[test]
    fn overflow_waiters_chain_not_star() {
        let t = ReadyEdgeTable::new();
        t.note_consumer_on(2, 0, Some(0xabc));
        t.note_consumer_on(3, 2, None);
        t.note_consumer_on(4, 3, None);
        t.note_consumer_on(5, 4, None);
        assert_eq!(t.chain_tip_before(0, 10), 5);
        assert_eq!(t.blocking_producer(3), Some(2));
        assert_eq!(t.blocking_producer(4), Some(3));
        assert_eq!(t.blocking_producer(5), Some(4));
        assert!(
            !t.may_execute(5),
            "chain tail must wait — not wake with the window tip"
        );
    }

    #[test]
    fn plant_window_caps_location_and_chains_overflow() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let loc = 0xabc;
        assert!(t.plant_observed_window(1, 0, loc, 2));
        assert!(t.plant_observed_window(2, 0, loc, 2));
        assert!(t.admitted_on_location(1));
        assert!(t.admitted_on_location(2));
        assert!(t.plant_observed_window(3, 0, loc, 2));
        assert!(t.plant_observed_window(4, 0, loc, 2));
        assert!(t.plant_observed_window(5, 0, loc, 2));
        assert!(!t.admitted_on_location(3));
        assert_eq!(t.consumer_count_on(loc), 2, "atomic cap, not racy walk");
        assert_eq!(t.blocking_producer(3), Some(2));
        assert_eq!(t.blocking_producer(4), Some(3));
        assert_eq!(t.blocking_producer(5), Some(4));
        t.note_producer_done(0, &wave);
        assert!(t.may_execute(1), "window waiter of 0 wakes");
        assert!(t.may_execute(2), "window waiter of 0 wakes");
        assert!(
            !t.may_execute(3) && !t.may_execute(5),
            "overflow must not stampede with the window tip"
        );
        t.note_producer_done(2, &wave);
        assert!(t.may_execute(3));
        assert!(!t.may_execute(4), "chain continues after loc-tip Commit");
    }

    #[test]
    fn plant_window_rebinds_leftovers_after_producer_done() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let loc = 0xabc;
        assert!(t.plant_observed_window(1, 0, loc, 1));
        t.note_producer_done(0, &wave);
        t.note_producer_done(1, &wave);
        assert!(
            !t.plant_observed_window(3, 0, loc, 1),
            "first leftover is elected tip and must execute"
        );
        assert!(
            t.plant_observed_window(5, 0, loc, 1),
            "later leftover must wait on the elected tip — not Released-mill"
        );
        assert_eq!(t.blocking_producer(5), Some(3));
        assert!(t.may_execute(3));
        assert!(!t.may_execute(5));
    }

    #[test]
    fn plant_window_later_tip_only_lowest_leftover_steals() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let loc = 0xabc;
        assert!(t.plant_observed_window(1, 0, loc, 1));
        t.note_producer_done(0, &wave);
        t.note_producer_done(1, &wave);
        assert!(
            !t.plant_observed_window(50, 0, loc, 1),
            "later leftover may elect first"
        );
        assert!(
            t.plant_observed_window(10, 0, loc, 1),
            "lowest leftover steals and rebinds the previous head"
        );
        assert!(
            t.plant_observed_window(30, 0, loc, 1),
            "mid leftover waits on leftover_min — not a third tip"
        );
        assert_eq!(t.blocking_producer(30), Some(10));
        assert_eq!(
            t.blocking_producer(50),
            Some(10),
            "stolen head must not stay Indep"
        );
        assert!(t.may_execute(10));
        assert!(!t.may_execute(30));
        assert!(!t.may_execute(50));
    }

    #[test]
    fn plant_window_leftover_elections_are_per_location() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_producer_done(0, &wave);
        assert!(
            !t.plant_observed_window(20, 0, 0xaaa, 1),
            "first leftover on ℓA is that loc's tip"
        );
        assert!(
            !t.plant_observed_window(40, 0, 0xbbb, 1),
            "other-loc leftover is a separate tip — not a block-wide chain"
        );
        assert_eq!(t.blocking_producer(40), None);
        assert!(t.may_execute(20));
        assert!(t.may_execute(40));
        assert!(
            t.plant_observed_window(60, 0, 0xaaa, 1),
            "same-ℓ leftover still waits on leftover_min"
        );
        assert_eq!(t.blocking_producer(60), Some(20));
        assert!(!t.may_execute(60));
    }

    #[test]
    fn first_wave_stays_on_original_producer() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_producer_done(0, &wave);
        assert!(!t.plant_observed_window(24, 0, 0xdef, 1));
        assert!(t.plant_observed_window(100, 50, 0xabc, 1));
        assert_eq!(
            t.blocking_producer(100),
            Some(50),
            "live original producer still gates first-wave"
        );
        t.note_producer_done(50, &wave);
        assert!(
            t.may_execute(100),
            "after original Done, first-wave is this ℓ's leftover tip — not rebound onto another loc"
        );
        assert!(t.may_execute(24));
        assert_eq!(t.blocking_producer(100), None);
    }

    #[test]
    fn plant_global_leftover_replaces_done_min() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_producer_done(0, &wave);
        assert!(!t.plant_global_leftover(20));
        t.note_producer_done(20, &wave);
        assert!(t.is_writer_done(20));
        assert!(
            !t.plant_global_leftover(40),
            "next leftover must become the new live min, not wait on a done head"
        );
        assert!(t.may_execute(40));
        assert!(t.plant_global_leftover(60));
        assert_eq!(
            t.blocking_producer(60),
            Some(40),
            "immediate next leftover may Detect-wait leftover_min"
        );
        assert!(!t.may_execute(60));
        assert!(t.leftover_surplus(60));
    }

    #[test]
    fn plant_global_leftover_claim_refuses_previous_tip() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_producer_done(0, &wave);
        assert!(
            !t.plant_global_leftover(40),
            "first leftover is the global tip"
        );
        assert!(t.may_execute(40));
        assert!(
            !t.plant_global_leftover(10),
            "lower leftover steals leftover_min without Detect-starring 40"
        );
        assert_eq!(t.blocking_producer(40), None);
        assert!(t.may_execute(10));
        assert!(
            !t.may_execute(40),
            "two leftover heads must not both execute"
        );
        assert!(t.leftover_surplus(40));
    }

    #[test]
    fn plant_global_leftover_does_not_detect_star_surplus() {
        let t = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        t.note_producer_done(0, &wave);
        assert!(!t.plant_global_leftover(20));
        assert!(t.plant_global_leftover(40));
        assert!(t.plant_global_leftover(60));
        assert_eq!(
            t.blocking_producer(40),
            Some(20),
            "next leftover Detect-waits leftover_min"
        );
        assert_eq!(
            t.blocking_producer(60),
            None,
            "surplus beyond next is claim-only, not Detect-starred"
        );
        assert!(t.may_execute(20));
        assert!(!t.may_execute(40));
        assert!(!t.may_execute(60));
        t.note_producer_done(20, &wave);
        assert!(
            t.may_execute(40),
            "next leftover_min executes after the claim head commits"
        );
        assert!(t.leftover_surplus(60));
    }

    #[test]
    fn take_finished_leftover_min_advances_sticky_claim() {
        let t = ReadyEdgeTable::new();
        assert!(!t.plant_global_leftover(20));
        assert!(t.plant_global_leftover(40));
        t.mark_done(20);
        let next = t.take_finished_leftover_min(|_| false);
        assert_eq!(next, Some(40));
        assert!(t.may_execute(40));
        assert!(!t.leftover_surplus(40));
    }

    #[test]
    fn detect_waits_on_follows_pred_chain() {
        let t = ReadyEdgeTable::new();
        t.note_consumer_on(15, 10, Some(0xaaa));
        t.note_consumer_on(20, 15, Some(0xbbb));
        assert!(t.detect_waits_on(20, 10));
        assert!(t.detect_waits_on(15, 10));
        assert!(!t.detect_waits_on(10, 20));
        assert!(!t.detect_waits_on(20, 3));
    }

    #[test]
    fn plant_observed_waw_allows_two_writer_ping_pong() {
        let t = ReadyEdgeTable::new();
        assert!(
            t.should_plant_observed_waw(5, 3),
            "thin hops=0 pair must plant"
        );
        t.note_consumer_on(5, 3, Some(0x32be));
        assert!(!t.may_execute(5), "planted waiter must not Opt-ping-pong");
    }

    #[test]
    fn plant_observed_waw_still_plants_when_producer_waiting() {
        let t = ReadyEdgeTable::new();
        t.note_consumer_on(5, 1, Some(0x32be));
        assert!(!t.may_execute(5));
        assert!(
            t.should_plant_observed_waw(8, 5),
            "must plant leftover Opt writers even if pred is waiting"
        );
    }

    #[test]
    fn evict_surplus_waiters_keeps_newest_and_w_max() {
        let t = ReadyEdgeTable::new();
        t.note_consumer_on(3, 0, Some(0xabc));
        t.note_consumer_on(5, 1, Some(0xabc));
        t.note_consumer_on(7, 2, Some(0xabc));
        assert_eq!(t.consumer_count_on(0xabc), 3);
        let evicted = t.evict_surplus_waiters(0xabc, 7, 2);
        assert_eq!(evicted, vec![3]);
        assert_eq!(t.consumers_queued_on(0xabc), vec![5, 7]);
        assert!(t.may_execute(3), "evicted waiter returns to Opt");
        assert!(!t.may_execute(5));
        assert!(!t.may_execute(7));
    }

    #[test]
    fn anonymous_fan_in_does_not_grow_loc_wait_set() {
        let t = ReadyEdgeTable::new();
        t.note_consumer_on(3, 0, Some(0xabc));
        t.note_consumer_on(5, 1, Some(0xabc));
        assert_eq!(t.consumer_count_on(0xabc), 2);
        t.note_consumer_on(7, 5, None);
        assert_eq!(
            t.consumer_count_on(0xabc),
            2,
            "anonymous fan-in must not grow loc wait-set"
        );
        assert!(!t.may_execute(7));
        assert_eq!(t.blocking_producer(7), Some(5));
    }
}
