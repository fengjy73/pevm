//! Cost-aware admit policy — B3 hot-path EV + B2 cross-block estimator.
//!
//! Soft=0 actions: **A0** OptimisticRead vs **A1** OrderedAdmit (refuse +
//! wave-admit pred). Beta posteriors are **features**, not `decide()` authority.
//! Plant SoT: PC-adaptive design v1 (3356896).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use rustc_hash::FxBuildHasher;

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
    /// Cohorts forced A0 by the Lean / EV gate (PC-5).
    pub lean_a0_cohorts: usize,
    /// Mean estimated cost of A1 decisions (feature units).
    pub mean_c_a1: f64,
    /// Mean estimated cost of the A0 counterfactual at those decisions.
    pub mean_c_a0_cf: f64,
    /// A0 executions that later paid reexec (incarnation>0).
    pub a0_reexec: usize,
    /// Off-edge incarnation>0 — miss-Detect (D4).
    pub miss_detect: usize,
    /// Sampled ready-set width mean (PC-4).
    pub ready_width_mean: f64,
    /// Idle-core nanoseconds accumulated on yield / empty refuse (PC-4).
    pub idle_core_ns: u64,
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
}

/// Per-address B2 context: p_effWAW + refuse-cost EMA.
#[derive(Debug, Clone, Copy)]
struct CohortStat {
    p_eff: OnlineStat,
    refuse: OnlineStat,
}

/// Process-persistent cost calibrator (B2) + per-block B3 decide.
#[derive(Debug)]
pub(crate) struct CostPolicy {
    block_n: AtomicUsize,
    refuse_global: Mutex<OnlineStat>,
    reexec_global: Mutex<OnlineStat>,
    idle_global: Mutex<OnlineStat>,
    /// Linear weights for p_eff ≈ σ(w·x). x = [1, contract, logn, storage, small, beta].
    w_p: Mutex<[f64; 6]>,
    cohorts: DashMap<(u8, Address), CohortStat, FxBuildHasher>,
    a1_decisions: AtomicUsize,
    lean_a0: AtomicUsize,
    c_a1_sum_bits: AtomicU64,
    c_a0_sum_bits: AtomicU64,
    ev_samples: AtomicUsize,
    a0_reexec: AtomicUsize,
    miss_detect: AtomicUsize,
}

impl Default for CostPolicy {
    fn default() -> Self {
        Self {
            block_n: AtomicUsize::new(0),
            refuse_global: Mutex::new(OnlineStat::new(0.22)),
            reexec_global: Mutex::new(OnlineStat::new(1.0)),
            idle_global: Mutex::new(OnlineStat::new(0.12)),
            // Cold-start: intercept 0.05, contract +0.9, logn mild, storage +1.1.
            w_p: Mutex::new([0.05, 0.90, 0.12, 1.10, -0.15, 0.80]),
            cohorts: DashMap::default(),
            a1_decisions: AtomicUsize::new(0),
            lean_a0: AtomicUsize::new(0),
            c_a1_sum_bits: AtomicU64::new(0.0f64.to_bits()),
            c_a0_sum_bits: AtomicU64::new(0.0f64.to_bits()),
            ev_samples: AtomicUsize::new(0),
            a0_reexec: AtomicUsize::new(0),
            miss_detect: AtomicUsize::new(0),
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
        *self.w_p.lock().unwrap() = [0.05, 0.90, 0.12, 1.10, -0.15, 0.80];
        self.cohorts.clear();
        self.reset_block_counters();
    }

    fn reset_block_counters(&self) {
        self.a1_decisions.store(0, Ordering::Relaxed);
        self.lean_a0.store(0, Ordering::Relaxed);
        self.c_a1_sum_bits
            .store(0.0f64.to_bits(), Ordering::Relaxed);
        self.c_a0_sum_bits
            .store(0.0f64.to_bits(), Ordering::Relaxed);
        self.ev_samples.store(0, Ordering::Relaxed);
        self.a0_reexec.store(0, Ordering::Relaxed);
        self.miss_detect.store(0, Ordering::Relaxed);
    }

    pub(crate) fn begin_block(&self, n: usize) {
        self.block_n.store(n, Ordering::Relaxed);
        self.reset_block_counters();
    }

    #[inline]
    pub(crate) fn block_n(&self) -> usize {
        self.block_n.load(Ordering::Relaxed)
    }

    /// B3: E[c|A0] ≈ p_effWAW · reexec; E[c|A1] ≈ refuse + width_loss.
    /// PC-5: if E[meta_A1] > E[abort_A0] → A0 (OCC-cost).
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
            self.lean_a0.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::A0OptimisticRead;
        }
        // 2-tx same-from pairs are OCC-cost (3356896 has ~60; refuse is pure meta).
        if kind == CohortKind::SameFrom && cohort_len < 3 {
            self.lean_a0.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::A0OptimisticRead;
        }
        let (e_a0, e_a1) = self.ev(kind, addr, cohort_len, is_contract, p_beta);
        self.record_ev(e_a0, e_a1);
        if e_a1 > e_a0 {
            self.lean_a0.fetch_add(1, Ordering::Relaxed);
            AdmitAction::A0OptimisticRead
        } else {
            self.a1_decisions.fetch_add(1, Ordering::Relaxed);
            AdmitAction::A1OrderedAdmit
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

    fn ev(
        &self,
        kind: CohortKind,
        addr: Address,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> (f64, f64) {
        let p = self.p_eff(kind, addr, cohort_len, is_contract, p_beta);
        let reexec = self.reexec_global.lock().unwrap().mean.max(0.35);
        let refuse = self.refuse_cost(kind, addr, cohort_len);
        let width = self.width_loss(cohort_len);
        let e_a0 = p * reexec;
        let e_a1 = refuse + width;
        (e_a0, e_a1)
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
        // PC-5 small-block: inflate A1 meta so lazy cohorts retreat to OCC-cost.
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

    /// B2: observed refuse / idle / reexec cost sample (c = αΔwall + βreexec + γidle).
    pub(crate) fn note_cost_sample(&self, refuse_unit: f64, reexec: bool, idle_ns: u64) {
        if refuse_unit > 0.0 {
            self.refuse_global.lock().unwrap().ema(refuse_unit, 0.15);
        }
        if reexec {
            self.reexec_global.lock().unwrap().ema(1.0, 0.20);
            self.a0_reexec.fetch_add(1, Ordering::Relaxed);
        }
        if idle_ns > 0 {
            // Normalize ns into the same unit as refuse (~1.0 ≈ 50µs of idle).
            let idle_u = (idle_ns as f64 / 50_000.0).min(4.0);
            self.idle_global.lock().unwrap().ema(idle_u, 0.10);
        }
    }

    /// D4: off-edge incarnation>0 — miss-Detect, raise p_eff on the location class.
    pub(crate) fn note_miss_detect(&self, kind: CohortKind, addr: Address) {
        self.bump_miss_detect();
        self.note_eff_waw(kind, addr, true);
    }

    pub(crate) fn bump_miss_detect(&self) {
        self.miss_detect.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_a0_reexec(&self) {
        self.a0_reexec.fetch_add(1, Ordering::Relaxed);
        self.reexec_global.lock().unwrap().ema(1.0, 0.20);
    }

    pub(crate) fn take_report(&self, ready_width_mean: f64, idle_core_ns: u64) -> LearnReport {
        let n = self.ev_samples.load(Ordering::Relaxed).max(1) as f64;
        LearnReport {
            a1_cohorts: self.a1_decisions.load(Ordering::Relaxed),
            lean_a0_cohorts: self.lean_a0.load(Ordering::Relaxed),
            mean_c_a1: f64::from_bits(self.c_a1_sum_bits.load(Ordering::Relaxed)) / n,
            mean_c_a0_cf: f64::from_bits(self.c_a0_sum_bits.load(Ordering::Relaxed)) / n,
            a0_reexec: self.a0_reexec.load(Ordering::Relaxed),
            miss_detect: self.miss_detect.load(Ordering::Relaxed),
            ready_width_mean,
            idle_core_ns,
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
            "2-tx same-from must stay OCC-cost"
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
}
