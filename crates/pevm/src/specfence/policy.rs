//! Cost-aware admit policy — L1 cost-EV + A0-majority block + CC-D1 routing.
//!
//! Soft=0 actions: **A0** OptimisticRead vs **A1** OrderedAdmit (refuse +
//! wave-admit pred). A0 is the OCC-effect path on this spine — not a hand-off
//! to a second OCC runtime. Beta posteriors are **features**, not `decide()`
//! authority. Decide is measured-ns EMA: keep A1 iff ĉ_A1 + δ < ĉ_A0.
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
const THIN_N_MAX: usize = 256;
/// ns-EV hysteresis: keep A1 only if ĉ_A1 + δ < ĉ_A0.
const NS_DELTA: f64 = 2_000.0;
/// Cold-start EMA priors (ns). Keep proven contract WAW / empty-to A1.
const PRIOR_C_A1_NS: f64 = 8_000.0;
const PRIOR_C_A0_NS: f64 = 25_000.0;
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
}

/// Per-address B2 context: p_effWAW + refuse-cost EMA.
#[derive(Debug, Clone, Copy)]
struct CohortStat {
    p_eff: OnlineStat,
    refuse: OnlineStat,
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
    /// C4: reexec_ns EMA keyed by location (feeds U3).
    loc_reexec_ns: DashMap<MemoryLocationHash, OnlineStat, FxBuildHasher>,
    /// C3: off-edge reexecs in this block (wave).
    wave_off_edge: AtomicUsize,
    wave_batch_noted: AtomicBool,
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
            loc_reexec_ns: DashMap::default(),
            wave_off_edge: AtomicUsize::new(0),
            wave_batch_noted: AtomicBool::new(false),
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
        self.loc_reexec_ns.clear();
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
        self.wave_off_edge.store(0, Ordering::Relaxed);
        self.wave_batch_noted.store(false, Ordering::Relaxed);
    }

    pub(crate) fn begin_block(&self, n: usize) {
        self.block_n.store(n, Ordering::Relaxed);
        self.reset_block_counters();
        let serial_hat = n as f64 * SERIAL_NS_PER_TX;
        let thin = n > 0 && n <= THIN_N_MAX && serial_hat < META_FLOOR_NS;
        self.a0_majority_block.store(thin, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn is_a0_majority_block(&self) -> bool {
        self.a0_majority_block.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn thin_a1_k(&self) -> usize {
        if self.is_a0_majority_block() {
            THIN_A1_K
        } else {
            usize::MAX
        }
    }

    #[inline]
    pub(crate) fn block_n(&self) -> usize {
        self.block_n.load(Ordering::Relaxed)
    }

    /// L1 ns-EV: keep A1 iff ĉ_A1 + δ < ĉ_A0. Structural vetoes stay (PC-2).
    /// Beta/`p_eff` are features only — not decide() authority.
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
        let (c_a0, c_a1) = self.ns_ev();
        self.record_ev(c_a0, c_a1);
        // Proven effective WAW (B2) stays A1 even if this-block EMA is noisy.
        let proven = self
            .cohorts
            .get(&(kind.as_u8(), addr))
            .is_some_and(|s| s.p_eff.mean >= 0.40 && s.p_eff.n >= 2.0);
        let keep = proven || c_a1 + NS_DELTA < c_a0;
        let _ = p_beta; // feature only — used in p_eff for reports, not decide().
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

    fn ns_ev(&self) -> (f64, f64) {
        let c_a1 = self.c_a1_ns.lock().unwrap().mean.max(1.0);
        let c_a0 = self.c_a0_ns.lock().unwrap().mean.max(1.0);
        (c_a0, c_a1)
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
        let mut e = self
            .cohorts
            .entry((kind.as_u8(), addr))
            .or_insert(CohortStat {
                p_eff: OnlineStat::new(if effective { 0.55 } else { 0.12 }),
                refuse: OnlineStat::new(0.22),
            });
        e.p_eff.ema(if effective { 1.0 } else { 0.0 }, 0.25);
        // Contextual update of w_p (one SGD step).
        let n = self.block_n().max(1);
        let is_contract = matches!(
            kind,
            CohortKind::CallWaw | CohortKind::RawFan | CohortKind::EmptyTo
        ) && effective;
        let x = self.features(kind, 8, is_contract, if effective { 0.4 } else { 0.1 });
        let y = if effective { 1.0 } else { 0.0 };
        let mut w = self.w_p.lock().unwrap();
        let pred = sigmoid(w.iter().zip(x.iter()).map(|(a, b)| a * b).sum());
        let err = y - pred;
        let lr = 0.08;
        for i in 0..6 {
            w[i] += lr * err * x[i];
        }
        let _ = n;
    }

    /// L1: measured refuse-path ns (hot-path).
    pub(crate) fn note_refuse_ns(&self, ns: u64) {
        if ns == 0 {
            return;
        }
        self.refuse_ns.fetch_add(ns, Ordering::Relaxed);
        self.c_a1_ns.lock().unwrap().ema_ns(ns as f64, EMA_ALPHA);
    }

    /// L1: measured incarnation>0 execute ns (hot-path).
    pub(crate) fn note_reexec_ns(&self, ns: u64) {
        self.note_reexec_ns_at(None, ns);
    }

    /// C4: reexec_ns EMA on a conflict ℓ (or global if `loc` is None).
    pub(crate) fn note_reexec_ns_at(&self, loc: Option<MemoryLocationHash>, ns: u64) {
        if ns == 0 {
            return;
        }
        self.reexec_ns.fetch_add(ns, Ordering::Relaxed);
        self.c_a0_ns.lock().unwrap().ema_ns(ns as f64, EMA_ALPHA);
        self.a0_reexec.fetch_add(1, Ordering::Relaxed);
        if let Some(loc) = loc {
            self.loc_reexec_ns
                .entry(loc)
                .or_insert(OnlineStat::new(PRIOR_C_A0_NS))
                .ema_ns(ns as f64, EMA_ALPHA);
        }
    }

    /// L1: idle-core ns attributed as A1 width loss.
    pub(crate) fn note_width_loss_ns(&self, ns: u64) {
        if ns == 0 {
            return;
        }
        self.width_loss_ns.fetch_add(ns, Ordering::Relaxed);
        let c_a1 = self.refuse_ns.load(Ordering::Relaxed) as f64 + ns as f64;
        self.c_a1_ns.lock().unwrap().ema_ns(c_a1, EMA_ALPHA);
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
    pub(crate) fn promote_short_edge(&self, location: MemoryLocationHash, reexec_ns: u64) {
        let mut e = self.promoted.entry(location).or_insert(PromotedLoc {
            hits: 0,
            reexec_ns_ema: PRIOR_C_A0_NS,
        });
        e.hits = e.hits.saturating_add(1);
        if reexec_ns > 0 {
            e.reexec_ns_ema = (1.0 - EMA_ALPHA) * e.reexec_ns_ema + EMA_ALPHA * (reexec_ns as f64);
        }
        self.note_conflict_promote();
        self.note_reexec_ns_at(Some(location), reexec_ns);
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
        self.promoted.get(&location).is_some_and(|s| s.hits >= 1)
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
        }
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
    fn contract_empty_to_hot_payee_is_a1() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        assert!(
            p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.30),
            "contract empty-to n=16 (0x209c class) must stay A1"
        );
    }

    #[test]
    fn calldata_short_waw_is_a1() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0xed);
        assert!(
            p.choose_a1(CohortKind::CallWaw, addr, 3, true, 0.25),
            "storage 14-17 class must stay A1"
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
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        assert!(
            p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "after effective WAW observations A1 must remain available"
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
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        for _ in 0..8 {
            p.note_refuse_ns(80_000);
        }
        assert!(
            p.choose_a1(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "proven effective WAW stays A1 (do not drop 0x209c class)"
        );
    }
}
