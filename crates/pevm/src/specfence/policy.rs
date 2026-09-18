//! Cost-aware admit policy — L1 cost-EV + A0-majority block + CC-D1 routing.
//!
//! Soft=0 actions: **A0** OptimisticRead vs **A1** OrderedAdmit (refuse +
//! wave-admit pred). A0 is the OCC-effect path on this spine — not a hand-off
//! to a second OCC runtime. Beta posteriors are **features**, not `decide()`
//! authority. Decide is measured-ns EMA: keep A1 iff ĉ_A1 + δ < ĉ_A0.
//!
//! O1: short WAW (≤`ORDER_WINDOW_K` hops) is fully ordered; long thin spines
//! stay A0 at begin (leftover-aware EV) — not a prefix-window serialize.
//! O2/L1: if ĉ_ordered_spine loses to abort EMA, demote that ℓ to A0.
//!
//! **L1 (thin / a0_majority_block):** default A0. Cold start may be A1=0.
//! Promote a short edge only when a per-ℓ / address-pair prior or observed
//! effective-WAW cost proves ordered cheaper. Do not freeze A1=K from the
//! optimistic full-shell prior (ĉ_A1=8µs < ĉ_A0=25µs) on a small block.
//! `THIN_A1_K` is a **cap**, not a frozen set of exactly 3.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use rustc_hash::FxBuildHasher;

use crate::{MemoryLocationHash, TxIdx};

use super::collateral::ConflictClass;

/// Small-block serial estimate below this → thin SpecFence shell (PC-S1).
const META_FLOOR_NS: f64 = 400_000.0;
/// ~2µs/tx cold serial hat (21k transfer class).
const SERIAL_NS_PER_TX: f64 = 2_000.0;
/// Thin-shell A1 spine **cap** (not “always exactly 3”). Promote/ignore may
/// change the set; ns-EV may keep fewer. 3356896 proven spines stay under K.
pub(crate) const THIN_A1_K: usize = 3;
/// O1: long Basic WAW plants at most this many hops (partial order). Short
/// chains (storage 14→16→17) stay fully ordered. Never full-serialize a
/// 16-writer spine — that prepaid wall lost to OCC abort on 3356896.
pub(crate) const ORDER_WINDOW_K: usize = 2;
const THIN_N_MAX: usize = 256;
/// ns-EV hysteresis: keep A1 only if ĉ_A1 + δ < ĉ_A0.
const NS_DELTA: f64 = 2_000.0;
/// Full-shell cold-start EMA priors (ns). Large blocks may keep A1.
const PRIOR_C_A1_NS: f64 = 8_000.0;
const PRIOR_C_A0_NS: f64 = 25_000.0;
/// Thin-block cold priors: OCC abort is cheaper than refuse/wave prepaid.
const PRIOR_C_A1_THIN_NS: f64 = 18_000.0;
const PRIOR_C_A0_THIN_NS: f64 = 6_000.0;
/// Consecutive blocks where prepaid A1 loses to the abort counterfactual.
const PREPAID_LOSE_N: u32 = 2;
const EMA_ALPHA: f64 = 0.20;

/// Soft=0 action on a candidate edge / cohort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmitAction {
    /// OptimisticRead — execute now; conflict pays reexec.
    A0OptimisticRead,
    /// OrderedAdmit — refuse while pred unfinished; steal independent work.
    A1OrderedAdmit,
}

/// Envelope / location cohort used as the B2 context key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum CohortKind {
    SameFrom = 0,
    EmptyTo = 1,
    CallWaw = 2,
    RawFan = 3,
}

impl CohortKind {
    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// End-of-block learning report (the four questions in the design note).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LearnReport {
    /// Cohorts that chose A1 this block.
    pub a1_cohorts: usize,
    /// Optimistic-read cohorts (A0 / cost-EV / K-cap / empty-to EOA).
    pub a0_cohorts: usize,
    /// Mean estimated cost of A1 decisions (ns EMA).
    pub mean_c_a1: f64,
    /// Mean estimated cost of the A0 counterfactual at those decisions (ns EMA).
    pub mean_c_a0_cf: f64,
    /// A0 executions that later paid reexec (incarnation>0).
    pub a0_reexec: usize,
    /// Incarnation>0 txs that were never dependency-admitted (A0 conflict reexec).
    pub unfenced_reexec: usize,
    /// Sampled ready-set width mean (PC-W4 |Ready| bag).
    pub ready_width_mean: f64,
    /// Idle-core nanoseconds accumulated on yield / empty refuse (PC-4).
    pub idle_core_ns: u64,
    /// A0-majority / low-meta block (n small; ReadyEdge only on ordered-admit).
    pub a0_majority_block: bool,
    /// Measured refuse-path nanoseconds.
    pub refuse_ns: u64,
    /// Measured incarnation>0 execute nanoseconds.
    pub reexec_ns: u64,
    /// Cost-EV evaluations that kept ordered admit (ĉ_A1+δ < ĉ_A0).
    pub cost_ev_keep_ordered: usize,
    /// Cost-EV evaluations that demoted ordered admit → optimistic read.
    pub cost_ev_demote_optimistic: usize,
    /// Thin-shell K-cap demotions (eligible A1 beyond K).
    pub k_cap_demote: usize,
    /// CC-X1: 21k commute accepts (no abort).
    pub commute_skip: usize,
    /// CC-R3: off-edge aborts parked as a batch behind a writer.
    pub batch_repair: usize,
    /// CC-D1: effective non-lazy conflict → learn/promote.
    pub conflict_promote: usize,
    /// CC-D1: lazy-noise conflict → ignorable (not unfenced_reexec→A1).
    pub conflict_ignore: usize,
    /// L3: A1 prepaid ns this block (refuse + width loss).
    pub prepaid_ns: u64,
    /// L3: counterfactual OCC abort ns (measured reexec).
    pub abort_cf_ns: u64,
    /// L3: prior decay steps applied because prepaid lost.
    pub prior_decay: usize,
    /// P2: end-block HotSet / prior / D1 / learn wall (one Instant).
    pub end_block_ns: u64,
}

#[derive(Debug, Clone, Copy)]
struct OnlineStat {
    n: f64,
    mean: f64,
}

impl OnlineStat {
    const fn new(prior: f64) -> Self {
        Self {
            n: 1.0,
            mean: prior,
        }
    }

    fn update(&mut self, x: f64) {
        self.n += 1.0;
        self.mean += (x - self.mean) / self.n;
    }

    fn ema(&mut self, x: f64, alpha: f64) {
        self.mean = (1.0 - alpha) * self.mean + alpha * x.clamp(0.0, 8.0);
        self.n += 1.0;
    }

    fn ema_ns(&mut self, x: f64, alpha: f64) {
        self.mean = (1.0 - alpha) * self.mean + alpha * x.clamp(0.0, 10_000_000.0);
        self.n += 1.0;
    }
}

/// CC-D1 first-conflict note for an off-edge incarnation>0 tx.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConflictNote {
    pub location: MemoryLocationHash,
    pub peer: Option<TxIdx>,
    pub class: ConflictClass,
    pub lazy: bool,
}

/// Cross-block short-edge promote (location, not a whole lazy spine).
#[derive(Debug, Clone, Copy)]
struct PromotedLoc {
    hits: u32,
    reexec_ns_ema: f64,
    /// True after an EffectiveWAW observation (abort hat if `reexec_ns` was 0).
    measured: bool,
    /// Immediate predecessor on this ℓ (reuse / mid-block seed).
    pred: TxIdx,
    /// Immediate successor on this ℓ (reuse / mid-block seed).
    succ: TxIdx,
    /// O2/L1: ĉ_ordered_spine lost to abort EMA — next begin plants 0 hops.
    demoted: bool,
}

/// Per-address B2 context: p_effWAW + refuse-cost EMA + per-pair cost-EV.
#[derive(Debug, Clone, Copy)]
struct CohortStat {
    p_eff: OnlineStat,
    refuse: OnlineStat,
    c_a0: OnlineStat,
    c_a1: OnlineStat,
    prepaid_lose: u32,
}

/// Process-persistent cost calibrator (B2) + per-block ns-EV decide.
#[derive(Debug)]
pub(crate) struct CostPolicy {
    block_n: AtomicUsize,
    refuse_global: Mutex<OnlineStat>,
    reexec_global: Mutex<OnlineStat>,
    idle_global: Mutex<OnlineStat>,
    /// L1: EMA of measured refuse_ns + width_loss_ns.
    c_a1_ns: Mutex<OnlineStat>,
    /// L1: EMA of measured reexec_ns.
    c_a0_ns: Mutex<OnlineStat>,
    /// Linear weights for p_eff ≈ σ(w·x) — **feature only**, not decide().
    w_p: Mutex<[f64; 6]>,
    cohorts: DashMap<(u8, Address), CohortStat, FxBuildHasher>,
    a1_decisions: AtomicUsize,
    a0_cohorts: AtomicUsize,
    c_a1_sum_bits: AtomicU64,
    c_a0_sum_bits: AtomicU64,
    ev_samples: AtomicUsize,
    a0_reexec: AtomicUsize,
    unfenced_reexec: AtomicUsize,
    a0_majority_block: AtomicBool,
    cost_ev_keep_ordered: AtomicUsize,
    cost_ev_demote_optimistic: AtomicUsize,
    k_cap_demote: AtomicUsize,
    refuse_ns: AtomicU64,
    reexec_ns: AtomicU64,
    width_loss_ns: AtomicU64,
    commute_skip: AtomicUsize,
    batch_repair: AtomicUsize,
    conflict_promote: AtomicUsize,
    conflict_ignore: AtomicUsize,
    /// CC-D1: first conflict ℓ per off-edge tx.
    conflicts: DashMap<TxIdx, ConflictNote, FxBuildHasher>,
    /// C4: EffectiveWAW locations eligible for a short-edge A1 (cap-limited).
    promoted: DashMap<MemoryLocationHash, PromotedLoc, FxBuildHasher>,
    /// L4: consecutive (pred, succ) pairs on a hot ℓ (beyond one stored pair).
    short_chain: DashMap<MemoryLocationHash, Vec<(TxIdx, TxIdx)>, FxBuildHasher>,
    /// L2: reexec_ns EMA keyed by location (feeds U3 / per-ℓ EV).
    loc_reexec_ns: DashMap<MemoryLocationHash, OnlineStat, FxBuildHasher>,
    /// L2: refuse/prepaid EMA keyed by location.
    loc_c_a1: DashMap<MemoryLocationHash, OnlineStat, FxBuildHasher>,
    /// Hot-path read-only EV snapshot (written at begin/end-block).
    c_a0_snap_bits: AtomicU64,
    c_a1_snap_bits: AtomicU64,
    /// L3: consecutive blocks where prepaid lost to abort_cf (process-persistent).
    prepaid_lose_streak: AtomicUsize,
    prior_decay: AtomicUsize,
    prepaid_ns: AtomicU64,
    abort_cf_ns: AtomicU64,
    /// C3: off-edge reexecs in this block (wave).
    wave_off_edge: AtomicUsize,
    wave_batch_noted: AtomicBool,
    /// L4: previous block size — reuse may seed stored (pred, succ) idx pairs.
    last_block_n: AtomicUsize,
    /// O3: (ℓ, pred, succ) recorded during abort — flush only when idle.
    pending_idle: Mutex<Vec<(MemoryLocationHash, TxIdx, TxIdx)>>,
}

impl Default for CostPolicy {
    fn default() -> Self {
        Self {
            block_n: AtomicUsize::new(0),
            refuse_global: Mutex::new(OnlineStat::new(0.22)),
            reexec_global: Mutex::new(OnlineStat::new(1.0)),
            idle_global: Mutex::new(OnlineStat::new(0.12)),
            c_a1_ns: Mutex::new(OnlineStat::new(PRIOR_C_A1_NS)),
            c_a0_ns: Mutex::new(OnlineStat::new(PRIOR_C_A0_NS)),
            // Cold-start: intercept 0.05, contract +0.9, logn mild, storage +1.1.
            w_p: Mutex::new([0.05, 0.90, 0.12, 1.10, -0.15, 0.80]),
            cohorts: DashMap::default(),
            a1_decisions: AtomicUsize::new(0),
            a0_cohorts: AtomicUsize::new(0),
            c_a1_sum_bits: AtomicU64::new(0.0f64.to_bits()),
            c_a0_sum_bits: AtomicU64::new(0.0f64.to_bits()),
            ev_samples: AtomicUsize::new(0),
            a0_reexec: AtomicUsize::new(0),
            unfenced_reexec: AtomicUsize::new(0),
            a0_majority_block: AtomicBool::new(false),
            cost_ev_keep_ordered: AtomicUsize::new(0),
            cost_ev_demote_optimistic: AtomicUsize::new(0),
            k_cap_demote: AtomicUsize::new(0),
            refuse_ns: AtomicU64::new(0),
            reexec_ns: AtomicU64::new(0),
            width_loss_ns: AtomicU64::new(0),
            commute_skip: AtomicUsize::new(0),
            batch_repair: AtomicUsize::new(0),
            conflict_promote: AtomicUsize::new(0),
            conflict_ignore: AtomicUsize::new(0),
            conflicts: DashMap::default(),
            promoted: DashMap::default(),
            short_chain: DashMap::default(),
            loc_reexec_ns: DashMap::default(),
            loc_c_a1: DashMap::default(),
            c_a0_snap_bits: AtomicU64::new(PRIOR_C_A0_NS.to_bits()),
            c_a1_snap_bits: AtomicU64::new(PRIOR_C_A1_NS.to_bits()),
            prepaid_lose_streak: AtomicUsize::new(0),
            prior_decay: AtomicUsize::new(0),
            prepaid_ns: AtomicU64::new(0),
            abort_cf_ns: AtomicU64::new(0),
            wave_off_edge: AtomicUsize::new(0),
            wave_batch_noted: AtomicBool::new(false),
            last_block_n: AtomicUsize::new(0),
            pending_idle: Mutex::new(Vec::new()),
        }
    }
}

impl CostPolicy {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reset(&self) {
        self.block_n.store(0, Ordering::Relaxed);
        *self.refuse_global.lock().unwrap() = OnlineStat::new(0.22);
        *self.reexec_global.lock().unwrap() = OnlineStat::new(1.0);
        *self.idle_global.lock().unwrap() = OnlineStat::new(0.12);
        *self.c_a1_ns.lock().unwrap() = OnlineStat::new(PRIOR_C_A1_NS);
        *self.c_a0_ns.lock().unwrap() = OnlineStat::new(PRIOR_C_A0_NS);
        *self.w_p.lock().unwrap() = [0.05, 0.90, 0.12, 1.10, -0.15, 0.80];
        self.cohorts.clear();
        self.conflicts.clear();
        self.promoted.clear();
        self.short_chain.clear();
        self.loc_reexec_ns.clear();
        self.loc_c_a1.clear();
        self.pending_idle.lock().unwrap().clear();
        self.prepaid_lose_streak.store(0, Ordering::Relaxed);
        self.last_block_n.store(0, Ordering::Relaxed);
        self.c_a0_snap_bits
            .store(PRIOR_C_A0_NS.to_bits(), Ordering::Relaxed);
        self.c_a1_snap_bits
            .store(PRIOR_C_A1_NS.to_bits(), Ordering::Relaxed);
        self.reset_block_counters();
    }

    fn reset_block_counters(&self) {
        self.a1_decisions.store(0, Ordering::Relaxed);
        self.a0_cohorts.store(0, Ordering::Relaxed);
        self.c_a1_sum_bits
            .store(0.0f64.to_bits(), Ordering::Relaxed);
        self.c_a0_sum_bits
            .store(0.0f64.to_bits(), Ordering::Relaxed);
        self.ev_samples.store(0, Ordering::Relaxed);
        self.a0_reexec.store(0, Ordering::Relaxed);
        self.unfenced_reexec.store(0, Ordering::Relaxed);
        self.a0_majority_block.store(false, Ordering::Relaxed);
        self.cost_ev_keep_ordered.store(0, Ordering::Relaxed);
        self.cost_ev_demote_optimistic.store(0, Ordering::Relaxed);
        self.k_cap_demote.store(0, Ordering::Relaxed);
        self.refuse_ns.store(0, Ordering::Relaxed);
        self.reexec_ns.store(0, Ordering::Relaxed);
        self.width_loss_ns.store(0, Ordering::Relaxed);
        self.commute_skip.store(0, Ordering::Relaxed);
        self.batch_repair.store(0, Ordering::Relaxed);
        self.conflict_promote.store(0, Ordering::Relaxed);
        self.conflict_ignore.store(0, Ordering::Relaxed);
        self.conflicts.clear();
        self.prior_decay.store(0, Ordering::Relaxed);
        self.prepaid_ns.store(0, Ordering::Relaxed);
        self.abort_cf_ns.store(0, Ordering::Relaxed);
        self.wave_off_edge.store(0, Ordering::Relaxed);
        self.wave_batch_noted.store(false, Ordering::Relaxed);
        self.pending_idle.lock().unwrap().clear();
    }

    pub(crate) fn begin_block(&self, n: usize) {
        let prev = self.block_n.load(Ordering::Relaxed);
        if prev > 0 {
            self.last_block_n.store(prev, Ordering::Relaxed);
        }
        self.block_n.store(n, Ordering::Relaxed);
        self.reset_block_counters();
        let serial_hat = n as f64 * SERIAL_NS_PER_TX;
        let thin = n > 0 && n <= THIN_N_MAX && serial_hat < META_FLOOR_NS;
        self.a0_majority_block.store(thin, Ordering::Relaxed);
        // Hot-path snapshot: thin cold defaults A0 (abort cheaper than prepaid).
        let (c_a0, c_a1) = if thin {
            (PRIOR_C_A0_THIN_NS, PRIOR_C_A1_THIN_NS)
        } else {
            (
                self.c_a0_ns.lock().unwrap().mean.max(1.0),
                self.c_a1_ns.lock().unwrap().mean.max(1.0),
            )
        };
        self.c_a0_snap_bits.store(c_a0.to_bits(), Ordering::Relaxed);
        self.c_a1_snap_bits.store(c_a1.to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn is_a0_majority_block(&self) -> bool {
        self.a0_majority_block.load(Ordering::Relaxed)
    }

    /// Thin / a0_majority: plant A1 only when a measured pair or ℓ EV says
    /// prepaid is cheaper. Cold start stays A1=0 (no hint walk / no contracts).
    pub(crate) fn should_seed_thin_a1(&self) -> bool {
        if !self.is_a0_majority_block() {
            return true;
        }
        for e in self.promoted.iter() {
            if self.is_promoted(*e.key()) {
                return true;
            }
        }
        for e in self.cohorts.iter() {
            let s = e.value();
            if (s.c_a0.n >= 2.0 || s.c_a1.n >= 2.0) && s.c_a1.mean + NS_DELTA < s.c_a0.mean.max(1.0)
            {
                return true;
            }
        }
        false
    }

    #[inline]
    pub(crate) fn thin_a1_k(&self) -> usize {
        if self.is_a0_majority_block() {
            THIN_A1_K
        } else {
            usize::MAX
        }
    }

    /// O1/O2: how many consecutive hops to plant on `ℓ`.
    ///
    /// Short chains (≤`ORDER_WINDOW_K`) plant all (storage 14→16→17).
    /// Long thin spines: leftover-aware EV. Prefix-window + tail abort is
    /// the worst of both worlds (PR22 PRIMARY miss). Plant 0 at begin when
    /// A0-all wins; O3 may add one idle hop after an abort.
    pub(crate) fn hops_to_plant(&self, location: MemoryLocationHash, n_pairs: usize) -> usize {
        if n_pairs == 0 {
            return 0;
        }
        if self.loc_demoted(location) {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
            return 0;
        }
        if !self.is_promoted(location) {
            return 0;
        }
        if n_pairs <= ORDER_WINDOW_K {
            return n_pairs;
        }
        if !self.is_a0_majority_block() {
            return n_pairs;
        }
        if self.long_spine_window_loses(location, n_pairs) {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
            return 0;
        }
        self.cost_ev_keep_ordered.fetch_add(1, Ordering::Relaxed);
        ORDER_WINDOW_K
    }

    /// Leftover-aware: cost(W) = W·stall + (n−W)·abort vs cost(A0) = n·abort.
    /// Prefix plant loses iff stall ≥ abort. Thin stall is the A1 prior — a
    /// 16-writer wait-for is not 18µs of refuse; it is pred-execute overlap
    /// loss, so thin long spines default to A0 (OCC abort).
    fn long_spine_window_loses(&self, location: MemoryLocationHash, n_pairs: usize) -> bool {
        if n_pairs <= ORDER_WINDOW_K {
            return false;
        }
        self.spine_ordered_loses(location, n_pairs)
            || self.spine_ordered_loses(location, ORDER_WINDOW_K)
    }

    /// ĉ_ordered_spine = hops × ĉ_A1 vs loc abort EMA (not hops×abort).
    /// Full 16-writer prepaid is hops×18µs; one abort sample is tens of µs.
    pub(crate) fn spine_ordered_loses(&self, location: MemoryLocationHash, hops: usize) -> bool {
        if hops == 0 {
            return false;
        }
        let c_ord = self
            .loc_c_a1
            .get(&location)
            .map(|s| s.mean)
            .unwrap_or_else(|| f64::from_bits(self.c_a1_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        // Thin: refuse prior understates wait-for-pred stall (31 blocked on 4).
        let stall = if self.is_a0_majority_block() {
            c_ord.max(PRIOR_C_A1_THIN_NS)
        } else {
            c_ord
        };
        let c_abort = self
            .promoted
            .get(&location)
            .map(|s| s.reexec_ns_ema)
            .or_else(|| self.loc_reexec_ns.get(&location).map(|s| s.mean))
            .unwrap_or_else(|| f64::from_bits(self.c_a0_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        let leftover = hops.saturating_sub(1) as f64 * c_abort.max(PRIOR_C_A0_THIN_NS);
        let ordered_spine = hops as f64 * stall + leftover;
        ordered_spine + NS_DELTA >= c_abort * hops.max(1) as f64
    }

    #[inline]
    pub(crate) fn loc_demoted(&self, location: MemoryLocationHash) -> bool {
        self.promoted.get(&location).is_some_and(|s| s.demoted)
    }

    pub(crate) fn promoted_locations(&self) -> Vec<MemoryLocationHash> {
        self.promoted.iter().map(|e| *e.key()).collect()
    }

    /// D1-shaped writer lists from stored short-edge pairs (edge_4_31 without MV walk).
    pub(crate) fn writer_orders_from_pairs(&self) -> Vec<(MemoryLocationHash, Vec<TxIdx>)> {
        let mut map: hashbrown::HashMap<MemoryLocationHash, Vec<TxIdx>> = hashbrown::HashMap::new();
        for (loc, pred, succ) in self.promoted_short_pairs() {
            let v = map.entry(loc).or_default();
            v.push(pred);
            v.push(succ);
        }
        let mut out: Vec<_> = map
            .into_iter()
            .map(|(loc, mut w)| {
                w.sort_unstable();
                w.dedup();
                (loc, w)
            })
            .collect();
        out.sort_by_key(|(loc, _)| *loc);
        out
    }

    /// Stored consecutive pairs on `ℓ` (consensus order).
    pub(crate) fn pairs_of(&self, location: MemoryLocationHash) -> Vec<(TxIdx, TxIdx)> {
        let mut out = Vec::new();
        if let Some(s) = self.promoted.get(&location)
            && s.pred < s.succ
        {
            out.push((s.pred, s.succ));
        }
        if let Some(e) = self.short_chain.get(&location) {
            for &(pred, succ) in e.value() {
                if pred < succ {
                    out.push((pred, succ));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// O3: record a pair to plant only if the successor is still idle.
    pub(crate) fn queue_idle_edge(&self, location: MemoryLocationHash, pred: TxIdx, succ: TxIdx) {
        if pred >= succ {
            return;
        }
        self.pending_idle
            .lock()
            .unwrap()
            .push((location, pred, succ));
    }

    pub(crate) fn take_pending_idle(&self) -> Vec<(MemoryLocationHash, TxIdx, TxIdx)> {
        std::mem::take(&mut *self.pending_idle.lock().unwrap())
    }

    fn mark_loc_demoted(&self, location: MemoryLocationHash) {
        if let Some(mut e) = self.promoted.get_mut(&location) {
            e.demoted = true;
        }
    }

    #[inline]
    pub(crate) fn block_n(&self) -> usize {
        self.block_n.load(Ordering::Relaxed)
    }

    /// L1 ns-EV: keep A1 iff ĉ_A1 + δ < ĉ_A0. Structural vetoes stay (PC-2).
    /// Beta/`p_eff` are features only — not decide() authority.
    ///
    /// Thin / a0_majority_block: default A0. Cold start may be A1=0. Keep
    /// ordered only when a **measured** per-pair (or per-ℓ) prior proves
    /// prepaid cheaper than abort — not the full-shell 8µs-vs-25µs prior.
    pub(crate) fn choose(
        &self,
        kind: CohortKind,
        addr: Address,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> AdmitAction {
        if cohort_len < 2 {
            return AdmitAction::A0OptimisticRead;
        }
        // PC-2: envelope empty-to on an EOA is LazyRecipient — never A1.
        if kind == CohortKind::EmptyTo && !is_contract {
            self.a0_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::A0OptimisticRead;
        }
        // 2-tx same-from pairs stay A0 (3356896 has ~60; refuse is pure meta).
        if kind == CohortKind::SameFrom && cohort_len < 3 {
            self.a0_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::A0OptimisticRead;
        }
        // A0-majority: 2-tx calldata pairs stay A0 (K reserved for ≥3 spines).
        if self.is_a0_majority_block() && kind == CohortKind::CallWaw && cohort_len < 3 {
            self.a0_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::A0OptimisticRead;
        }
        let _ = p_beta; // feature only — used in p_eff for reports, not decide().
        let keep = if self.is_a0_majority_block() {
            self.pair_ev_prefers_ordered(kind, addr)
        } else {
            let (c_a0, c_a1) = self.ns_ev_snap();
            self.record_ev(c_a0, c_a1);
            let proven = self
                .cohorts
                .get(&(kind.as_u8(), addr))
                .is_some_and(|s| s.p_eff.mean >= 0.40 && s.p_eff.n >= 2.0);
            proven || c_a1 + NS_DELTA < c_a0
        };
        if keep {
            self.cost_ev_keep_ordered.fetch_add(1, Ordering::Relaxed);
            self.a1_decisions.fetch_add(1, Ordering::Relaxed);
            AdmitAction::A1OrderedAdmit
        } else {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
            self.a0_cohorts.fetch_add(1, Ordering::Relaxed);
            AdmitAction::A0OptimisticRead
        }
    }

    #[inline]
    pub(crate) fn choose_a1(
        &self,
        kind: CohortKind,
        addr: Address,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> bool {
        self.choose(kind, addr, cohort_len, is_contract, p_beta) == AdmitAction::A1OrderedAdmit
    }

    #[inline]
    fn ns_ev_snap(&self) -> (f64, f64) {
        let c_a0 = f64::from_bits(self.c_a0_snap_bits.load(Ordering::Relaxed)).max(1.0);
        let c_a1 = f64::from_bits(self.c_a1_snap_bits.load(Ordering::Relaxed)).max(1.0);
        (c_a0, c_a1)
    }

    /// L2: per address-pair EV. Thin cold (no measured samples) → A0.
    fn pair_ev_prefers_ordered(&self, kind: CohortKind, addr: Address) -> bool {
        let Some(s) = self.cohorts.get(&(kind.as_u8(), addr)) else {
            return false;
        };
        // Need measured abort/prepaid samples — p_eff alone does not freeze A1.
        let measured = s.c_a0.n >= 2.0 || s.c_a1.n >= 2.0;
        if !measured {
            return false;
        }
        let c_a0 = s.c_a0.mean.max(1.0);
        let c_a1 = s.c_a1.mean.max(1.0);
        self.record_ev(c_a0, c_a1);
        c_a1 + NS_DELTA < c_a0
    }

    /// Score for A0-majority K-cap (storage/call WAW > contract empty-to).
    pub(crate) fn a1_score(kind: CohortKind, cohort_len: usize, is_contract: bool) -> i64 {
        let class = match kind {
            CohortKind::RawFan => 4000,
            CohortKind::CallWaw => 3000,
            CohortKind::EmptyTo if is_contract => 2000,
            CohortKind::SameFrom => 1000,
            CohortKind::EmptyTo => 0,
        };
        class + cohort_len as i64
    }

    pub(crate) fn note_k_cap_demote(&self) {
        let a1 = self.a1_decisions.load(Ordering::Relaxed);
        if a1 > 0 {
            self.a1_decisions.fetch_sub(1, Ordering::Relaxed);
        }
        self.a0_cohorts.fetch_add(1, Ordering::Relaxed);
        self.k_cap_demote.fetch_add(1, Ordering::Relaxed);
    }

    fn p_eff(
        &self,
        kind: CohortKind,
        addr: Address,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> f64 {
        if let Some(st) = self.cohorts.get(&(kind.as_u8(), addr))
            && st.p_eff.n >= 3.0
        {
            return st.p_eff.mean.clamp(0.02, 0.95);
        }
        let x = self.features(kind, cohort_len, is_contract, p_beta);
        let w = *self.w_p.lock().unwrap();
        let z: f64 = w.iter().zip(x.iter()).map(|(a, b)| a * b).sum();
        sigmoid(z).clamp(0.02, 0.95)
    }

    fn features(
        &self,
        kind: CohortKind,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> [f64; 6] {
        let n = self.block_n() as f64;
        let logn = ((cohort_len as f64).max(1.0)).ln();
        let storage = matches!(kind, CohortKind::CallWaw | CohortKind::RawFan) as u8 as f64;
        let small = if n > 0.0 && n <= 256.0 { 1.0 } else { 0.0 };
        let contract = if is_contract || matches!(kind, CohortKind::CallWaw | CohortKind::RawFan) {
            1.0
        } else {
            0.0
        };
        [1.0, contract, logn, storage, small, p_beta.clamp(0.0, 1.0)]
    }

    fn refuse_cost(&self, kind: CohortKind, addr: Address, cohort_len: usize) -> f64 {
        let base = self
            .cohorts
            .get(&(kind.as_u8(), addr))
            .map(|s| s.refuse.mean)
            .unwrap_or_else(|| self.refuse_global.lock().unwrap().mean);
        // Short chains: refuse + width almost always lose to one cheap abort.
        let short = if cohort_len < 3 && !matches!(kind, CohortKind::CallWaw | CohortKind::RawFan) {
            0.18
        } else {
            0.0
        };
        // PC-5 small-block: inflate A1 meta so lazy cohorts stay A0.
        let n = self.block_n() as f64;
        let lean = if n > 0.0 && n <= 256.0 { 1.15 } else { 1.0 };
        (base * lean + short).clamp(0.08, 1.5)
    }

    fn width_loss(&self, cohort_len: usize) -> f64 {
        let idle = self.idle_global.lock().unwrap().mean;
        let cores = 8.0;
        idle * ((cohort_len.saturating_sub(1) as f64) / cores).min(1.0)
    }

    fn record_ev(&self, e_a0: f64, e_a1: f64) {
        add_f64(&self.c_a0_sum_bits, e_a0);
        add_f64(&self.c_a1_sum_bits, e_a1);
        self.ev_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// B2: observe effective vs lazy WAW on a cohort (write-set Detect).
    pub(crate) fn note_eff_waw(&self, kind: CohortKind, addr: Address, effective: bool) {
        let thin = self.is_a0_majority_block();
        let mut e = self
            .cohorts
            .entry((kind.as_u8(), addr))
            .or_insert(CohortStat {
                p_eff: OnlineStat::new(if effective { 0.55 } else { 0.12 }),
                refuse: OnlineStat::new(0.22),
                c_a0: OnlineStat::new(if thin {
                    PRIOR_C_A0_THIN_NS
                } else {
                    PRIOR_C_A0_NS
                }),
                c_a1: OnlineStat::new(if thin {
                    PRIOR_C_A1_THIN_NS
                } else {
                    PRIOR_C_A1_NS
                }),
                prepaid_lose: 0,
            });
        e.p_eff.ema(if effective { 1.0 } else { 0.0 }, 0.25);
        // L4: no w_p SGD on the Detect hot path — pair EV is enough.
        let _ = effective;
    }

    /// L4: refuse ns is an atomic add; EMA flush is end-block.
    pub(crate) fn note_refuse_ns(&self, ns: u64) {
        if ns == 0 {
            return;
        }
        self.refuse_ns.fetch_add(ns, Ordering::Relaxed);
    }

    /// L1: measured incarnation>0 execute ns (atomic; EMA at end-block / loc).
    pub(crate) fn note_reexec_ns(&self, ns: u64) {
        self.note_reexec_ns_at(None, ns);
    }

    /// L2: reexec_ns EMA on a conflict ℓ (or global if `loc` is None).
    pub(crate) fn note_reexec_ns_at(&self, loc: Option<MemoryLocationHash>, ns: u64) {
        if ns == 0 {
            return;
        }
        self.reexec_ns.fetch_add(ns, Ordering::Relaxed);
        self.a0_reexec.fetch_add(1, Ordering::Relaxed);
        if let Some(loc) = loc {
            self.loc_reexec_ns
                .entry(loc)
                .or_insert(OnlineStat::new(PRIOR_C_A0_NS))
                .ema_ns(ns as f64, EMA_ALPHA);
        }
    }

    /// L4: idle-core ns attributed as A1 width loss (atomic; EMA at end-block).
    pub(crate) fn note_width_loss_ns(&self, ns: u64) {
        if ns == 0 {
            return;
        }
        self.width_loss_ns.fetch_add(ns, Ordering::Relaxed);
    }

    /// B2: observed refuse / idle / reexec cost sample (legacy units + ns feed).
    pub(crate) fn note_cost_sample(&self, refuse_unit: f64, reexec: bool, idle_ns: u64) {
        if refuse_unit > 0.0 {
            self.refuse_global.lock().unwrap().ema(refuse_unit, 0.15);
        }
        if reexec {
            self.reexec_global.lock().unwrap().ema(1.0, 0.20);
        }
        if idle_ns > 0 {
            let idle_u = (idle_ns as f64 / 50_000.0).min(4.0);
            self.idle_global.lock().unwrap().ema(idle_u, 0.10);
            self.note_width_loss_ns(idle_ns);
        }
    }

    pub(crate) fn note_commute_skip(&self) {
        self.commute_skip.fetch_add(1, Ordering::Relaxed);
    }

    /// Probe-star early release: commute absorbed and no measured reexec —
    /// cost-EV demotes remaining EmptyTo ordered-admit waiters.
    #[inline]
    pub(crate) fn should_release_probe_star(&self) -> bool {
        self.is_a0_majority_block()
            && self.commute_skip.load(Ordering::Relaxed) >= 8
            && self.reexec_ns.load(Ordering::Relaxed) == 0
    }

    pub(crate) fn note_batch_repair(&self) {
        self.batch_repair.fetch_add(1, Ordering::Relaxed);
    }

    /// CC-D1: first conflict ℓ + peer + class on an off-edge inc>0 tx.
    pub(crate) fn note_conflict_ell(
        &self,
        tx: TxIdx,
        location: MemoryLocationHash,
        peer: Option<TxIdx>,
        class: ConflictClass,
        lazy: bool,
    ) {
        self.conflicts.entry(tx).or_insert(ConflictNote {
            location,
            peer,
            class,
            lazy,
        });
    }

    /// C3: one off-edge reexec in this wave. First time count ≥ 2 → batch_repair.
    pub(crate) fn note_wave_off_edge_reexec(&self) {
        let n = self.wave_off_edge.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= 2 && !self.wave_batch_noted.swap(true, Ordering::Relaxed) {
            self.note_batch_repair();
        }
    }

    /// C4: EffectiveWAW → short-edge promote (location only, not a lazy spine).
    ///
    /// L1: `reexec_ns == 0` still sets `measured=true` using the location EMA or
    /// the abort hat. Passing 0 must not lock the edge unpromoted for the block.
    pub(crate) fn promote_short_edge(&self, location: MemoryLocationHash, reexec_ns: u64) {
        let measured_ns = if reexec_ns > 0 {
            reexec_ns as f64
        } else {
            self.loc_reexec_ns
                .get(&location)
                .map(|s| s.mean)
                .filter(|m| *m > 1.0)
                .unwrap_or(PRIOR_C_A0_NS)
        };
        let mut e = self.promoted.entry(location).or_insert(PromotedLoc {
            hits: 0,
            reexec_ns_ema: PRIOR_C_A0_NS,
            measured: false,
            pred: usize::MAX,
            succ: usize::MAX,
            demoted: false,
        });
        e.hits = e.hits.saturating_add(1);
        e.reexec_ns_ema = (1.0 - EMA_ALPHA) * e.reexec_ns_ema + EMA_ALPHA * measured_ns;
        e.measured = true;
        drop(e);
        self.note_conflict_promote();
        self.note_reexec_ns_at(Some(location), reexec_ns);
    }

    /// L2/L4: remember the immediate (pred, succ) pair on a hot ℓ.
    pub(crate) fn note_short_pair(&self, location: MemoryLocationHash, pred: TxIdx, succ: TxIdx) {
        if pred >= succ {
            return;
        }
        let mut e = self.promoted.entry(location).or_insert(PromotedLoc {
            hits: 0,
            reexec_ns_ema: PRIOR_C_A0_NS,
            measured: false,
            pred: usize::MAX,
            succ: usize::MAX,
            demoted: false,
        });
        if e.pred == usize::MAX || pred < e.pred {
            e.pred = pred;
            e.succ = succ;
        }
        drop(e);
        let mut chain = self.short_chain.entry(location).or_default();
        if !chain.iter().any(|&(p, s)| p == pred && s == succ) {
            chain.push((pred, succ));
        }
    }

    /// Post-publish (end-block): persist consecutive D1 pairs on already
    /// promoted ℓ. Completes 4→31→66→… after the first block without
    /// inserting ReadyEdges mid-execute.
    pub(crate) fn note_promoted_writer_orders(&self, orders: &[(MemoryLocationHash, Vec<TxIdx>)]) {
        for (loc, writers) in orders {
            if writers.len() < 2 || !self.is_promoted(*loc) {
                continue;
            }
            for pair in writers.windows(2) {
                self.note_short_pair(*loc, pair[0], pair[1]);
            }
        }
    }

    /// L4: stored short-edge pairs whose idx still match this block size.
    pub(crate) fn promoted_short_pairs(&self) -> Vec<(MemoryLocationHash, TxIdx, TxIdx)> {
        let n = self.block_n();
        let prev = self.last_block_n.load(Ordering::Relaxed);
        let same_shape = n > 0 && (prev == 0 || prev == n);
        if !same_shape {
            return Vec::new();
        }
        let mut out = Vec::new();
        for e in self.promoted.iter() {
            let s = *e.value();
            if s.hits >= 1 && s.measured && s.pred < s.succ && s.succ < n {
                out.push((*e.key(), s.pred, s.succ));
            }
        }
        for e in self.short_chain.iter() {
            let loc = *e.key();
            if !self.is_promoted(loc) {
                continue;
            }
            for &(pred, succ) in e.value() {
                if pred < succ && succ < n {
                    out.push((loc, pred, succ));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// C3: keep a short edge when ĉ_reexec (ℓ EMA) > ĉ_ordered.
    /// First EffectiveWAW / stored pair is enough proof on a thin block.
    pub(crate) fn loc_ev_prefers_ordered(&self, location: MemoryLocationHash) -> bool {
        let c_reexec = self
            .promoted
            .get(&location)
            .map(|s| s.reexec_ns_ema)
            .or_else(|| self.loc_reexec_ns.get(&location).map(|s| s.mean))
            .unwrap_or(PRIOR_C_A0_NS)
            .max(1.0);
        let c_ord = self
            .loc_c_a1
            .get(&location)
            .map(|s| s.mean)
            .unwrap_or_else(|| f64::from_bits(self.c_a1_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        c_ord + NS_DELTA < c_reexec
    }

    /// C1/C2/C3: gate a short ReadyEdge after an effective publish.
    ///
    /// `has_earlier` — D1 already saw a prior writer (4→31).
    /// `hint_later` — envelope successors still unpublished (14→16, 31→66).
    pub(crate) fn should_gate_short_after_write(
        &self,
        location: MemoryLocationHash,
        has_earlier: bool,
        hint_later: usize,
    ) -> bool {
        if self.is_promoted(location) {
            let n_pairs = self
                .short_chain
                .get(&location)
                .map(|c| c.len())
                .unwrap_or(0);
            // Promoted long thin spine: begin already demoted to A0. Do not
            // re-gate mid-block (that rebuilt the 16-writer prepaid wall).
            if n_pairs > ORDER_WINDOW_K && self.is_a0_majority_block() {
                return self.hops_to_plant(location, n_pairs) > 0;
            }
            return true;
        }
        if !has_earlier && hint_later < 2 {
            return false;
        }
        if self.is_a0_majority_block() && !self.can_add_short_loc(location) {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
            return false;
        }
        // C2: first effective write with a known successor, or 2nd writer on ℓ.
        // C3: loc EV / abort hat must still beat prepaid.
        let keep = has_earlier || hint_later >= 2 || self.loc_ev_prefers_ordered(location);
        if keep {
            self.cost_ev_keep_ordered.fetch_add(1, Ordering::Relaxed);
            self.a1_decisions.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
        }
        keep
    }

    fn can_add_short_loc(&self, location: MemoryLocationHash) -> bool {
        if self.promoted.contains_key(&location) {
            return true;
        }
        let n = self
            .promoted
            .iter()
            .filter(|e| e.measured && e.hits >= 1)
            .count();
        n < THIN_A1_K
    }

    /// C5: one real short edge (not an envelope cohort).
    pub(crate) fn note_short_edge_admit(&self) {
        self.a1_decisions.fetch_add(1, Ordering::Relaxed);
        self.cost_ev_keep_ordered.fetch_add(1, Ordering::Relaxed);
    }

    /// C4: commute / LazyNoise → ignore (do not raise unfenced_reexec→A1).
    pub(crate) fn ignore_conflict(&self, location: Option<MemoryLocationHash>) {
        self.note_conflict_ignore();
        if let Some(loc) = location {
            // Successful commute lowers the promote weight on this ℓ.
            if let Some(mut e) = self.promoted.get_mut(&loc)
                && e.hits > 0
            {
                e.hits -= 1;
            }
        }
    }

    #[inline]
    pub(crate) fn is_promoted(&self, location: MemoryLocationHash) -> bool {
        let Some(s) = self.promoted.get(&location) else {
            return false;
        };
        if s.hits < 1 {
            return false;
        }
        if s.demoted && self.is_a0_majority_block() {
            return false;
        }
        if !self.is_a0_majority_block() {
            return true;
        }
        if !s.measured {
            return false;
        }
        // C3: per-ℓ abort EMA vs prepaid snap (not the thin global 6µs A0 prior).
        let c_ord = self
            .loc_c_a1
            .get(&location)
            .map(|e| e.mean)
            .unwrap_or_else(|| f64::from_bits(self.c_a1_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        c_ord + NS_DELTA < s.reexec_ns_ema
    }

    pub(crate) fn conflict_of(&self, tx: TxIdx) -> Option<ConflictNote> {
        self.conflicts.get(&tx).map(|e| *e)
    }

    pub(crate) fn note_conflict_promote(&self) {
        self.conflict_promote.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_conflict_ignore(&self) {
        self.conflict_ignore.fetch_add(1, Ordering::Relaxed);
    }

    /// D4: off-edge incarnation>0 — miss-Detect, raise p_eff on the location class.
    pub(crate) fn note_unfenced_reexec(&self, kind: CohortKind, addr: Address) {
        self.bump_unfenced_reexec();
        self.note_eff_waw(kind, addr, true);
    }

    pub(crate) fn bump_unfenced_reexec(&self) {
        self.unfenced_reexec.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_a0_reexec(&self) {
        self.a0_reexec.fetch_add(1, Ordering::Relaxed);
        self.reexec_global.lock().unwrap().ema(1.0, 0.20);
    }

    /// L2 test / end-block helper: write a measured address-pair EV sample.
    pub(crate) fn note_pair_measured_ev(
        &self,
        kind: CohortKind,
        addr: Address,
        c_a0: f64,
        c_a1: f64,
    ) {
        let thin = self.is_a0_majority_block();
        let mut e = self
            .cohorts
            .entry((kind.as_u8(), addr))
            .or_insert(CohortStat {
                p_eff: OnlineStat::new(0.55),
                refuse: OnlineStat::new(0.22),
                c_a0: OnlineStat::new(if thin {
                    PRIOR_C_A0_THIN_NS
                } else {
                    PRIOR_C_A0_NS
                }),
                c_a1: OnlineStat::new(if thin {
                    PRIOR_C_A1_THIN_NS
                } else {
                    PRIOR_C_A1_NS
                }),
                prepaid_lose: 0,
            });
        e.c_a0 = OnlineStat {
            n: 3.0,
            mean: c_a0.max(1.0),
        };
        e.c_a1 = OnlineStat {
            n: 3.0,
            mean: c_a1.max(1.0),
        };
    }

    /// L3: flush EMAs, compare prepaid vs abort counterfactual, decay if prepaid loses.
    pub(crate) fn end_block_learn(&self) {
        let refuse = self.refuse_ns.load(Ordering::Relaxed);
        let reexec = self.reexec_ns.load(Ordering::Relaxed);
        let width = self.width_loss_ns.load(Ordering::Relaxed);
        let prepaid = refuse.saturating_add(width);
        let abort_cf = reexec;
        self.prepaid_ns.store(prepaid, Ordering::Relaxed);
        self.abort_cf_ns.store(abort_cf, Ordering::Relaxed);
        if refuse > 0 {
            self.c_a1_ns
                .lock()
                .unwrap()
                .ema_ns(refuse as f64, EMA_ALPHA);
        }
        if reexec > 0 {
            self.c_a0_ns
                .lock()
                .unwrap()
                .ema_ns(reexec as f64, EMA_ALPHA);
        }
        let prepaid_lost = prepaid > abort_cf && prepaid > 0;
        if prepaid_lost {
            let n = self.prepaid_lose_streak.fetch_add(1, Ordering::Relaxed) + 1;
            if n >= PREPAID_LOSE_N as usize {
                self.decay_ordered_priors();
            }
            // O2/L1: long spines whose ordered EV lost → next begin plants 0.
            self.demote_long_spines();
        } else {
            self.prepaid_lose_streak.store(0, Ordering::Relaxed);
        }
        // L-E: abort_cf without prepaid → raise *short* edges. Long spines
        // that O2 demoted stay A0 — those aborts are the cheaper path.
        if abort_cf > prepaid {
            let long: hashbrown::HashSet<MemoryLocationHash> = self
                .short_chain
                .iter()
                .filter(|e| e.value().len() > ORDER_WINDOW_K)
                .map(|e| *e.key())
                .collect();
            for mut e in self.promoted.iter_mut() {
                if e.measured {
                    e.hits = e.hits.saturating_add(1);
                    if !long.contains(e.key()) {
                        e.demoted = false;
                    }
                }
            }
        }
        // L-D: refresh thin snap from flushed EMAs so next begin can seed.
        // Use per-tx abort hat, not the whole-block reexec sum.
        let c_a0 = if abort_cf > 0 {
            self.c_a0_ns.lock().unwrap().mean.max(PRIOR_C_A0_NS)
        } else {
            self.c_a0_ns.lock().unwrap().mean.max(1.0)
        };
        let c_a1 = self.c_a1_ns.lock().unwrap().mean.max(1.0);
        if !self.is_a0_majority_block() || abort_cf > 0 {
            self.c_a0_snap_bits.store(c_a0.to_bits(), Ordering::Relaxed);
            self.c_a1_snap_bits.store(c_a1.to_bits(), Ordering::Relaxed);
        }
    }

    fn decay_ordered_priors(&self) {
        self.prior_decay.fetch_add(1, Ordering::Relaxed);
        for mut e in self.cohorts.iter_mut() {
            e.p_eff.ema(0.0, 0.35);
            let raised = e.c_a1.mean * 1.25 + PRIOR_C_A1_THIN_NS * 0.15;
            e.c_a1.ema_ns(raised, EMA_ALPHA);
            e.prepaid_lose = e.prepaid_lose.saturating_add(1);
        }
        for mut e in self.promoted.iter_mut() {
            if e.hits > 0 {
                e.hits -= 1;
            }
        }
    }

    /// O2: when prepaid lost, demote locations whose stored chain is longer
    /// than the window (16-writer Basic WAW). Storage trio (2 hops) stays.
    fn demote_long_spines(&self) {
        let long: Vec<MemoryLocationHash> = self
            .short_chain
            .iter()
            .filter(|e| e.value().len() > ORDER_WINDOW_K)
            .filter(|e| self.spine_ordered_loses(*e.key(), e.value().len()))
            .map(|e| *e.key())
            .collect();
        for loc in long {
            self.mark_loc_demoted(loc);
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn take_report(&self, ready_width_mean: f64, idle_core_ns: u64) -> LearnReport {
        let n = self.ev_samples.load(Ordering::Relaxed).max(1) as f64;
        LearnReport {
            a1_cohorts: self.a1_decisions.load(Ordering::Relaxed),
            a0_cohorts: self.a0_cohorts.load(Ordering::Relaxed),
            mean_c_a1: {
                let ema = self.c_a1_ns.lock().unwrap().mean;
                if ema > 0.0 {
                    ema
                } else {
                    f64::from_bits(self.c_a1_sum_bits.load(Ordering::Relaxed)) / n
                }
            },
            mean_c_a0_cf: {
                let ema = self.c_a0_ns.lock().unwrap().mean;
                if ema > 0.0 {
                    ema
                } else {
                    f64::from_bits(self.c_a0_sum_bits.load(Ordering::Relaxed)) / n
                }
            },
            a0_reexec: self.a0_reexec.load(Ordering::Relaxed),
            unfenced_reexec: self.unfenced_reexec.load(Ordering::Relaxed),
            ready_width_mean,
            idle_core_ns,
            a0_majority_block: self.is_a0_majority_block(),
            refuse_ns: self.refuse_ns.load(Ordering::Relaxed),
            reexec_ns: self.reexec_ns.load(Ordering::Relaxed),
            cost_ev_keep_ordered: self.cost_ev_keep_ordered.load(Ordering::Relaxed),
            cost_ev_demote_optimistic: self.cost_ev_demote_optimistic.load(Ordering::Relaxed),
            k_cap_demote: self.k_cap_demote.load(Ordering::Relaxed),
            commute_skip: self.commute_skip.load(Ordering::Relaxed),
            batch_repair: self.batch_repair.load(Ordering::Relaxed),
            conflict_promote: self.conflict_promote.load(Ordering::Relaxed),
            conflict_ignore: self.conflict_ignore.load(Ordering::Relaxed),
            prepaid_ns: self.prepaid_ns.load(Ordering::Relaxed),
            abort_cf_ns: self.abort_cf_ns.load(Ordering::Relaxed),
            prior_decay: self.prior_decay.load(Ordering::Relaxed),
            end_block_ns: 0,
        }
    }

    pub(crate) fn set_end_block_ns(&self, report: &mut LearnReport, ns: u64) {
        report.end_block_ns = ns;
    }
}

fn sigmoid(z: f64) -> f64 {
    let z = z.clamp(-12.0, 12.0);
    1.0 / (1.0 + (-z).exp())
}

fn add_f64(slot: &AtomicU64, x: f64) {
    let mut cur = slot.load(Ordering::Relaxed);
    loop {
        let next = (f64::from_bits(cur) + x).to_bits();
        match slot.compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(v) => cur = v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_empty_to_eoa_is_a0() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x9e);
        assert!(
            !p.choose_a1(CohortKind::EmptyTo, addr, 12, false, 0.10),
            "lazy EOA empty-to must be A0 (PC-2/PC-5)"
        );
    }

    #[test]
    fn thin_cold_start_defaults_a0() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        assert!(
            !p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.30),
            "L1: thin cold start must not freeze A1 on contract empty-to"
        );
        let storage = Address::repeat_byte(0xed);
        assert!(
            !p.choose_a1(CohortKind::CallWaw, storage, 3, true, 0.25),
            "L1: thin cold start must not freeze A1 on short calldata WAW"
        );
    }

    #[test]
    fn full_shell_contract_empty_to_hot_payee_is_a1() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0x20);
        assert!(
            p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.30),
            "full-shell contract empty-to n=16 stays A1"
        );
    }

    #[test]
    fn full_shell_calldata_short_waw_is_a1() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0xed);
        assert!(
            p.choose_a1(CohortKind::CallWaw, addr, 3, true, 0.25),
            "full-shell storage 14-17 class stays A1"
        );
    }

    #[test]
    fn two_tx_same_from_stays_a0() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x56);
        assert!(
            !p.choose_a1(CohortKind::SameFrom, addr, 2, false, 0.20),
            "2-tx same-from must stay A0"
        );
    }

    #[test]
    fn thin_two_tx_callwaw_stays_a0() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0xab);
        assert!(
            !p.choose_a1(CohortKind::CallWaw, addr, 2, true, 0.25),
            "thin-shell 2-tx calldata stays A0; K reserved for ≥3 spines"
        );
    }

    #[test]
    fn b2_raises_p_after_effective_waw() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0x20);
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        assert!(
            p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "full-shell: after effective WAW observations A1 remains available"
        );
    }

    #[test]
    fn thin_promotes_only_when_pair_ev_wins() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        assert!(
            !p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "thin: p_eff alone must not freeze A1"
        );
        p.note_pair_measured_ev(CohortKind::EmptyTo, addr, 80_000.0, 8_000.0);
        assert!(
            p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "thin: measured ĉ_A1+δ < ĉ_A0 promotes a short edge"
        );
    }

    #[test]
    fn a0_majority_block_on_small_block() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(p.is_a0_majority_block(), "n=176 serial_hat < meta floor");
        assert_eq!(p.thin_a1_k(), THIN_A1_K, "K is a cap on thin-shell");
        p.begin_block(4096);
        assert!(!p.is_a0_majority_block(), "large n keeps full shell");
        assert_eq!(p.thin_a1_k(), usize::MAX);
    }

    #[test]
    fn thin_a1_k_is_cap_not_exact_set() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert_eq!(p.thin_a1_k(), 3);
        // Promote does not freeze A1 at exactly 3 — it can add a short edge
        // that later admit ranks under the same cap.
        p.promote_short_edge(0x32be, 4_000);
        assert!(p.is_promoted(0x32be));
        p.ignore_conflict(Some(0x32be));
        assert!(!p.is_promoted(0x32be), "ignore can drop a promoted ℓ");
    }

    #[test]
    fn batch_repair_fires_on_second_off_edge_reexec() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.note_wave_off_edge_reexec();
        assert_eq!(p.take_report(0.0, 0).batch_repair, 0);
        p.note_wave_off_edge_reexec();
        assert_eq!(
            p.take_report(0.0, 0).batch_repair,
            1,
            "C3: second off-edge abort in the wave is one batch_repair"
        );
    }

    #[test]
    fn cost_ev_demote_optimistics_when_refuse_dominates() {
        let p = CostPolicy::new();
        p.begin_block(176);
        for _ in 0..8 {
            p.note_refuse_ns(80_000);
            p.note_width_loss_ns(20_000);
        }
        for _ in 0..2 {
            p.note_reexec_ns(4_000);
        }
        let addr = Address::repeat_byte(0x77);
        assert!(
            !p.choose_a1(CohortKind::EmptyTo, addr, 8, true, 0.10),
            "ĉ_A1+δ >= ĉ_A0 must demote an unproven cohort"
        );
    }

    #[test]
    fn probe_star_releases_when_commute_absorbs() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(!p.should_release_probe_star(), "cold block keeps Detect");
        for _ in 0..8 {
            p.note_commute_skip();
        }
        assert!(
            p.should_release_probe_star(),
            "commute-absorbed EmptyTo with reexec_ns=0 demotes leftover waiters"
        );
    }

    #[test]
    fn cost_ev_keeps_proven_contract_spine() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0x20);
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        for _ in 0..8 {
            p.note_refuse_ns(80_000);
        }
        assert!(
            p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "full-shell proven effective WAW stays A1 (do not drop 0x209c class)"
        );
    }

    #[test]
    fn thin_prepaid_loss_decays_prior() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        p.note_pair_measured_ev(CohortKind::EmptyTo, addr, 80_000.0, 8_000.0);
        assert!(p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.20));
        p.note_refuse_ns(50_000);
        p.note_width_loss_ns(10_000);
        // abort_cf = 0 → prepaid loses
        p.end_block_learn();
        p.begin_block(176);
        p.note_refuse_ns(50_000);
        p.end_block_learn();
        let r = p.take_report(0.0, 0);
        assert!(
            r.prior_decay >= 1,
            "L3: two prepaid-losing blocks must decay prior: {r:?}"
        );
        assert!(r.prepaid_ns > r.abort_cf_ns);
    }

    #[test]
    fn thin_cold_should_not_seed_a1() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(
            !p.should_seed_thin_a1(),
            "L1: thin cold start must not seed A1"
        );
        p.promote_short_edge(0x32be, 80_000);
        assert!(
            p.should_seed_thin_a1(),
            "L1: measured expensive abort may seed a short edge"
        );
    }

    #[test]
    fn thin_zero_ns_promote_uses_abort_hat() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 0);
        assert!(
            p.is_promoted(0x32be),
            "L1: promote_short_edge(ℓ, 0) must set measured and gate when abort hat wins"
        );
        p.promote_short_edge(0x32be, 80_000);
        assert!(
            p.is_promoted(0x32be),
            "thin: measured expensive abort must keep the short edge"
        );
    }

    #[test]
    fn thin_write_set_gates_known_successors() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(
            p.should_gate_short_after_write(0xedba, false, 2),
            "C2: first effective write with ≥2 later hint txs must promote"
        );
        assert!(
            p.should_gate_short_after_write(0x32be, true, 0),
            "C1: earlier writer on ℓ must raise the immediate successor"
        );
        assert!(
            !p.should_gate_short_after_write(0xabc, false, 0),
            "unique writer without a successor stays A0"
        );
    }

    #[test]
    fn reuse_promoted_pair_survives_next_begin() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        p.note_short_pair(0x32be, 4, 31);
        p.end_block_learn();
        p.begin_block(176);
        assert!(
            p.should_seed_thin_a1(),
            "L4: reuse must raise short-edge A1"
        );
        let pairs = p.promoted_short_pairs();
        assert!(
            pairs
                .iter()
                .any(|&(loc, pred, succ)| loc == 0x32be && pred == 4 && succ == 31),
            "L4: stored 4→31 pair must survive begin: {pairs:?}"
        );
    }

    #[test]
    fn end_block_persists_consecutive_writer_order() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        p.note_promoted_writer_orders(&[(0x32be, vec![4, 31, 66, 67]), (0xabc, vec![1, 2])]);
        p.end_block_learn();
        p.begin_block(176);
        let pairs = p.promoted_short_pairs();
        assert!(
            pairs
                .iter()
                .any(|&(l, a, b)| l == 0x32be && a == 4 && b == 31)
                && pairs
                    .iter()
                    .any(|&(l, a, b)| l == 0x32be && a == 31 && b == 66)
                && pairs
                    .iter()
                    .any(|&(l, a, b)| l == 0x32be && a == 66 && b == 67),
            "post-publish D1 must persist the full main-chain short edges: {pairs:?}"
        );
        assert!(
            !pairs.iter().any(|&(l, _, _)| l == 0xabc),
            "unpromoted ERC-20 slots must not enter short_chain"
        );
    }

    #[test]
    fn hops_to_plant_windows_long_spine_keeps_storage() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.promote_short_edge(0xedba, 20_000);
        p.note_short_pair(0xedba, 14, 16);
        p.note_short_pair(0xedba, 16, 17);
        assert_eq!(
            p.hops_to_plant(0x32be, 4),
            0,
            "O1/O2: leftover-aware EV demotes long thin spine to A0"
        );
        assert_eq!(
            p.hops_to_plant(0xedba, 2),
            2,
            "O1: storage trio stays fully ordered"
        );
        assert!(
            p.spine_ordered_loses(0x32be, 16),
            "O2: 16-writer prepaid must lose to one abort sample"
        );
        assert!(
            !p.should_gate_short_after_write(0x32be, true, 0),
            "O2: write-set must not re-gate a leftover-lose long spine"
        );
        assert!(
            p.should_gate_short_after_write(0xedba, true, 0),
            "O1: storage write-set still raises the short edge"
        );
    }

    #[test]
    fn prepaid_loss_demotes_long_spine_not_storage() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.promote_short_edge(0xedba, 20_000);
        p.note_short_pair(0xedba, 14, 16);
        p.note_short_pair(0xedba, 16, 17);
        p.note_refuse_ns(80_000);
        p.note_width_loss_ns(20_000);
        // abort_cf = 0 → prepaid loses
        p.end_block_learn();
        p.begin_block(176);
        assert!(
            p.loc_demoted(0x32be),
            "O2: long spine demotes after prepaid lose"
        );
        assert_eq!(p.hops_to_plant(0x32be, 4), 0);
        assert!(
            !p.loc_demoted(0xedba),
            "O2: storage trio must not demote with the long spine"
        );
        assert_eq!(p.hops_to_plant(0xedba, 2), 2);
    }
}
