//! Adaptive CC Redesign v1 — default LeanOCC engagement (replaces M4 prove-quiet).
//!
//! # Exact trigger (R1)
//!
//! ## Block start
//! Always start **LeanOCC**. Never require proving quiet (`last_abort_rate`,
//! `conflict_mass`, multi-writer hints) to lean — that was the M4 failure mode
//! (`lean_mode_txs=0` on mainnet).
//!
//! ## Location policy
//! HotLocal Bind/WaitHard/park applies only for `ℓ ∈ HotSet` (see `hotset.rs`).
//! Cold locations always SpecRead (OCC-style). WaitHard is forbidden off HotSet.
//!
//! ## Mid-block
//! On abort_rate ≥ τ_abort_mid (0.08), ensure abort locations land in HotSet
//! (via `HotSet::note_abort` / `insert`). Engagement stays LeanOCC for execute
//! (Handler::run) unless `SPECFENCE_ENABLE_INSPECT=1`.
//!
//! ## Research inspect
//! `SPECFENCE_ENABLE_INSPECT=1` re-enables inspect_run / jump / CallOutcome SC
//! (M1* plant). Default production path never inspects.
//!
//! OCC / PCC modes never consult this module.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::TxIdx;

/// Mid-block abort rate that escalates writers into HotSet (still Lean execute).
pub(crate) const TAU_ABORT_MID: f64 = 0.08;

/// `SPECFENCE_ENABLE_INSPECT=1` (or `true`/`yes`) enables research inspect/jump.
pub(crate) fn research_inspect_enabled() -> bool {
    match std::env::var_os("SPECFENCE_ENABLE_INSPECT") {
        None => false,
        Some(v) => {
            let s = v.to_string_lossy();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        }
    }
}

/// Per-block adaptive engagement controller (SpecFence only).
#[derive(Debug)]
pub(crate) struct AdaptiveEngagement {
    /// Execute-path lean (Handler::run). False only under research inspect.
    lean: AtomicBool,
    lean_txs: AtomicUsize,
    full_txs: AtomicUsize,
    switches: AtomicUsize,
    aborts: AtomicUsize,
    /// Last execute of `tx_idx` used the lean execute path.
    tx_was_lean: Vec<AtomicBool>,
}

impl AdaptiveEngagement {
    /// Redesign: always start LeanOCC (ignore old M4 prove-quiet gates).
    pub(crate) fn should_start_lean() -> bool {
        !research_inspect_enabled()
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
    /// Returns whether this incarnation should take the lean OCC-fast execute path.
    pub(crate) fn begin_tx(&self, tx_idx: TxIdx) -> bool {
        // Research inspect forces full execute for the whole process once set.
        let lean = if research_inspect_enabled() {
            self.lean.store(false, Ordering::Relaxed);
            false
        } else {
            true
        };
        // Keep AtomicBool in sync when research flag flips mid-process.
        self.lean.store(lean, Ordering::Relaxed);
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

    /// Record a validation abort. Returns true when mid-block abort rate crossed
    /// τ_abort_mid (caller should ensure HotSet membership for abort locs).
    pub(crate) fn note_abort(&self) -> bool {
        let aborts = self.aborts.fetch_add(1, Ordering::Relaxed) + 1;
        let started = self
            .lean_txs
            .load(Ordering::Relaxed)
            .saturating_add(self.full_txs.load(Ordering::Relaxed))
            .max(1);
        let rate = aborts as f64 / started as f64;
        if rate >= TAU_ABORT_MID {
            // Count an "engagement switch" once when abort storm starts — not lean→full
            // execute flip (execute stays lean). Signals HotSet escalate pressure.
            if self.switches.load(Ordering::Relaxed) == 0 {
                self.switches.store(1, Ordering::Relaxed);
            }
            return true;
        }
        false
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

    #[test]
    fn default_start_is_lean_without_inspect_flag() {
        // Ensure flag off for this unit test.
        unsafe {
            std::env::remove_var("SPECFENCE_ENABLE_INSPECT");
        }
        assert!(AdaptiveEngagement::should_start_lean());
        let eng = AdaptiveEngagement::new(4, true);
        assert!(eng.begin_tx(0));
        assert_eq!(eng.lean_mode_txs(), 1);
        assert_eq!(eng.full_mode_txs(), 0);
    }

    #[test]
    fn mid_block_abort_rate_signals_hotset_escalate() {
        unsafe {
            std::env::remove_var("SPECFENCE_ENABLE_INSPECT");
        }
        let eng = AdaptiveEngagement::new(10, true);
        assert!(eng.begin_tx(0));
        assert!(eng.note_abort()); // 1/1 ≥ 0.08
        assert_eq!(eng.engagement_switches(), 1);
        // Execute path stays lean.
        assert!(eng.begin_tx(1));
        assert!(eng.is_lean());
    }
}
