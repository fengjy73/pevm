//! Plant v2 M4 — adaptive SpecFence meta engagement (iron-law C).
//!
//! # Exact trigger
//!
//! ## Block start (`AdaptiveEngagement::should_start_lean`)
//! Start **lean** (SpecFence meta off / OCC-fast) when **all** hold:
//! 1. `last_abort_rate < τ_abort` with `τ_abort = 0.05`
//!    (`last_abort_rate = occ_aborts / max(1, n_tx)` from the previous SpecFence
//!    block; cold start / `reset_heat` → `0.0` → lean-eligible).
//! 2. `bayes.conflict_mass() < τ_mass` with `τ_mass = 0.12`
//!    (`conflict_mass` = mean over tracked locations of `max(0, P_ℓ − prior_mean)`).
//! 3. No location/account would seed Wait at `DEFAULT_TAU` (`hot_conflict_count(τ)=0`).
//! 4. No hinted account (excl. beneficiary) has `writer_count ≥ 2` — multi-writer
//!    schedules start **full** so M1 RewindTo / selective invalidate still engage.
//!
//! Otherwise start **full** (today's Bind/Wait/repair/inspect plant).
//!
//! ## Mid-block escalate (lean → full)
//! When lean and `occ_aborts_so_far / max(1, txs_started) ≥ τ_abort_mid` (`0.08`),
//! flip to full and bump `engagement_switches`. Subsequent txs use the full plant.
//!
//! ## Lean path (still `ConcurrencyMode::SpecFence`)
//! - Skip `inspect_run` → `Handler::run` like OCC.
//! - `maybe_wait`: SpecRead-only (no Bayes π / WaitHard / Bind / rem cps).
//! - No hinted Wait admission; abort uses OCC full ESTIMATE + full cascade.
//! - Still validate; bayes/rw_prior still learn on abort/success so the next
//!   block (or mid-block switch) can engage full.
//!
//! OCC / PCC modes never consult this module.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloy_primitives::Address;

use crate::TxIdx;

use super::bayes::BayesMap;
use super::AccountHints;

/// Abort rate from the previous SpecFence block above which we refuse lean start.
pub(crate) const TAU_ABORT: f64 = 0.05;
/// Mean excess conflict mass above the Beta prior that refuses lean start.
pub(crate) const TAU_MASS: f64 = 0.12;
/// Mid-block abort rate that escalates lean → full.
pub(crate) const TAU_ABORT_MID: f64 = 0.08;

/// Per-block adaptive engagement controller (SpecFence only).
#[derive(Debug)]
pub(crate) struct AdaptiveEngagement {
    lean: AtomicBool,
    lean_txs: AtomicUsize,
    full_txs: AtomicUsize,
    switches: AtomicUsize,
    /// Aborts observed while this block has been running (lean or full).
    aborts: AtomicUsize,
    /// Last execute of `tx_idx` used the lean path (for OCC-like abort).
    tx_was_lean: Vec<AtomicBool>,
}

impl AdaptiveEngagement {
    /// Block-start decision (documented trigger).
    pub(crate) fn should_start_lean(
        last_abort_rate: f64,
        bayes: &BayesMap,
        seed_tau: f64,
        hints: &AccountHints,
        beneficiary: Address,
    ) -> bool {
        if last_abort_rate >= TAU_ABORT {
            return false;
        }
        if bayes.conflict_mass() >= TAU_MASS {
            return false;
        }
        if bayes.hot_conflict_count(seed_tau) > 0 {
            return false;
        }
        // Multi-writer from/to hints → likely WW; keep full plant (RewindTo / fence).
        for address in hints.accounts() {
            if address != beneficiary && hints.writer_count(&address) >= 2 {
                return false;
            }
        }
        true
    }

    pub(crate) fn new(block_size: usize, start_lean: bool) -> Self {
        let mut tx_was_lean = Vec::with_capacity(block_size);
        for _ in 0..block_size {
            tx_was_lean.push(AtomicBool::new(false));
        }
        Self {
            lean: AtomicBool::new(start_lean),
            lean_txs: AtomicUsize::new(0),
            full_txs: AtomicUsize::new(0),
            switches: AtomicUsize::new(0),
            aborts: AtomicUsize::new(0),
            tx_was_lean,
        }
    }

    /// Disabled engagement (OCC/PCC): always "full" counters unused.
    pub(crate) fn disabled(block_size: usize) -> Self {
        Self::new(block_size, false)
    }

    #[inline]
    pub(crate) fn is_lean(&self) -> bool {
        self.lean.load(Ordering::Relaxed)
    }

    /// Call at the start of each `Vm::execute` under SpecFence.
    /// Returns whether this incarnation should take the lean OCC-fast path.
    pub(crate) fn begin_tx(&self, tx_idx: TxIdx) -> bool {
        let lean = self.is_lean();
        if lean {
            self.lean_txs.fetch_add(1, Ordering::Relaxed);
        } else {
            self.full_txs.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(slot) = self.tx_was_lean.get(tx_idx) {
            slot.store(lean, Ordering::Relaxed);
        }
        lean
    }

    pub(crate) fn tx_was_lean(&self, tx_idx: TxIdx) -> bool {
        self.tx_was_lean
            .get(tx_idx)
            .is_some_and(|b| b.load(Ordering::Relaxed))
    }

    /// Record a validation abort; may escalate lean → full.
    pub(crate) fn note_abort(&self) {
        let aborts = self.aborts.fetch_add(1, Ordering::Relaxed) + 1;
        if !self.is_lean() {
            return;
        }
        let started = self
            .lean_txs
            .load(Ordering::Relaxed)
            .saturating_add(self.full_txs.load(Ordering::Relaxed))
            .max(1);
        let rate = aborts as f64 / started as f64;
        if rate >= TAU_ABORT_MID {
            self.escalate();
        }
    }

    fn escalate(&self) {
        // Only count a switch when we actually flip lean → full.
        if self
            .lean
            .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            self.switches.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn lean_mode_txs(&self) -> usize {
        self.lean_txs.load(Ordering::Relaxed)
    }

    pub(crate) fn full_mode_txs(&self) -> usize {
        self.full_txs.load(Ordering::Relaxed)
    }

    pub(crate) fn engagement_switches(&self) -> usize {
        self.switches.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specfence::DEFAULT_TAU;

    #[test]
    fn cold_start_is_lean() {
        let bayes = BayesMap::new();
        let hints = AccountHints::default();
        assert!(AdaptiveEngagement::should_start_lean(
            0.0,
            &bayes,
            DEFAULT_TAU,
            &hints,
            Address::ZERO,
        ));
    }

    #[test]
    fn high_abort_rate_refuses_lean() {
        let bayes = BayesMap::new();
        let hints = AccountHints::default();
        assert!(!AdaptiveEngagement::should_start_lean(
            0.10,
            &bayes,
            DEFAULT_TAU,
            &hints,
            Address::ZERO,
        ));
    }

    #[test]
    fn mid_block_escalates_on_abort_rate() {
        let eng = AdaptiveEngagement::new(10, true);
        assert!(eng.begin_tx(0));
        // One abort out of one started → rate 1.0 ≥ 0.08.
        eng.note_abort();
        assert!(!eng.is_lean());
        assert_eq!(eng.engagement_switches(), 1);
        assert!(!eng.begin_tx(1));
        assert_eq!(eng.full_mode_txs(), 1);
    }
}
