//! SpecFence engagement — Lean execute + **quiet/storm block mode** (A+B+C).
//!
//! Authoritative: `lab/notes/specfence-abc-unified-protocol.md`.
//!
//! - Execute path: always LeanOCC for `Handler::run` unless research inspect.
//! - Block engagement mode (C): `Quiet` (598-like OCC-lite) vs `Storm`
//!   (597-like Await-ready on hot ℓ). Selected from inter morph prior; may flip
//!   mid-block on live morph evidence (598→599 style). Mode actuates Await set
//!   in `maybe_wait` — not SoftWait Soft arms.
//! - `note_abort` is **metrics-only** — does not escalate HotSet or flip π.
//! - `SPECFENCE_ENABLE_INSPECT=1` re-enables inspect_run / jump (research only).
//!
//! OCC / PCC modes never consult this module.


use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::TxIdx;

/// Legacy abort-rate cut (unused by π after V5-P0; retained for metrics docs).
#[allow(dead_code)]
pub(crate) const TAU_ABORT_MID: f64 = 0.08;
/// Legacy escalate window (unused after V5-P0 shovel).
#[allow(dead_code)]
pub(crate) const MIN_STARTED_FOR_ABORT_ESCALATE: usize = 32;
#[allow(dead_code)]
pub(crate) const MIN_ABORTS_FOR_ESCALATE: usize = 4;

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

/// Dig A/B: `SPECFENCE_DISABLE_SOFTWAIT=1` forces π SpecRead-only (never arm SoftWait).
/// Lean default SoftWait stays scarce; this is for makespan comparison only.
pub(crate) fn softwait_disabled() -> bool {
    match std::env::var_os("SPECFENCE_DISABLE_SOFTWAIT") {
        None => false,
        Some(v) => {
            let s = v.to_string_lossy();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        }
    }
}

/// Dig escape: `SPECFENCE_DISABLE_AWAIT_AT_A=1` turns off storm hot-ℓ Await@a
/// (production default = on). SoftWait Soft stays independent and ~0.
pub(crate) fn await_at_a_disabled() -> bool {
    match std::env::var_os("SPECFENCE_DISABLE_AWAIT_AT_A") {
        None => false,
        Some(v) => {
            let s = v.to_string_lossy();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        }
    }
}

/// Block-level engagement mode (inter/intra morph actuation — not SoftWait Soft).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum BlockEngagementMode {
    /// 598-like: OCC-lite discovery; narrow Await (force_prefix/sticky/prior only).
    Quiet = 0,
    /// 597-like: Await-ready on hot program ℓ (live fanout / inter top-ℓ / sticky).
    Storm = 1,
}

impl BlockEngagementMode {
    #[inline]
    pub(crate) fn from_u8(v: u8) -> Self {
        if v == BlockEngagementMode::Storm as u8 {
            BlockEngagementMode::Storm
        } else {
            BlockEngagementMode::Quiet
        }
    }

    /// Map morph weights → engagement mode.
    /// Storm when fan-out dominates or quiet mass collapses (mixed/storm evidence).
    pub(crate) fn from_morph(fan_out: f64, mixed: f64, quiet: f64) -> Self {
        if fan_out >= 0.35 && fan_out >= mixed && fan_out >= quiet {
            return BlockEngagementMode::Storm;
        }
        // Mixed with collapsed quiet (599 flip from 598): Await-ready.
        if quiet < 0.40 && (fan_out + mixed) >= 0.55 {
            return BlockEngagementMode::Storm;
        }
        BlockEngagementMode::Quiet
    }
}

/// `SPECFENCE_PROFILE=1` enables ns Instant buckets (handler/maybe_wait/validate/sched).
/// Default off — Instant tax on every SpecRead biases wall vs OCC.
pub(crate) fn profile_timing_enabled() -> bool {
    match std::env::var_os("SPECFENCE_PROFILE") {
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
    /// Quiet vs Storm (C): actuates hot-ℓ Await in maybe_wait.
    mode: AtomicUsize,
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
            mode: AtomicUsize::new(BlockEngagementMode::Quiet as usize),
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

    /// Record a validation abort (metrics only).
    ///
    /// V5-P0: always returns `false`. Abort-rate HotSet escalate / mode ladders
    /// are deleted from SpecFence control — π does not consult engagement.
    pub(crate) fn note_abort(&self) -> bool {
        self.aborts.fetch_add(1, Ordering::Relaxed);
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

    /// Set block engagement mode from inter-block morph prior (block start).
    pub(crate) fn set_mode_from_morph(&self, fan_out: f64, mixed: f64, quiet: f64) {
        let m = BlockEngagementMode::from_morph(fan_out, mixed, quiet);
        self.mode.store(m as usize, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn mode(&self) -> BlockEngagementMode {
        BlockEngagementMode::from_u8(self.mode.load(Ordering::Relaxed) as u8)
    }

    #[inline]
    pub(crate) fn is_storm(&self) -> bool {
        self.mode() == BlockEngagementMode::Storm
    }

    /// Mid-block / inter flip: update mode from live morph; count switches.
    pub(crate) fn maybe_flip_mode(&self, fan_out: f64, mixed: f64, quiet: f64) -> bool {
        let next = BlockEngagementMode::from_morph(fan_out, mixed, quiet);
        let cur = self.mode();
        if next != cur {
            self.mode.store(next as usize, Ordering::Relaxed);
            self.switches.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_start_is_lean_without_inspect_flag() {
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
    fn note_abort_never_escalates_hotset_v5() {
        unsafe {
            std::env::remove_var("SPECFENCE_ENABLE_INSPECT");
        }
        let eng = AdaptiveEngagement::new(64, true);
        for i in 0..40 {
            assert!(eng.begin_tx(i));
        }
        for _ in 0..20 {
            assert!(
                !eng.note_abort(),
                "V5-P0: engagement must not escalate HotSet / flip π"
            );
        }
        assert_eq!(eng.engagement_switches(), 0);
        assert!(eng.is_lean());
    }

    #[test]
    fn first_abort_does_not_escalate_hotset() {
        unsafe {
            std::env::remove_var("SPECFENCE_ENABLE_INSPECT");
        }
        let eng = AdaptiveEngagement::new(4, true);
        assert!(eng.begin_tx(0));
        assert!(!eng.note_abort());
        assert_eq!(eng.engagement_switches(), 0);
    }

    #[test]
    fn morph_selects_quiet_vs_storm() {
        assert_eq!(
            BlockEngagementMode::from_morph(0.10, 0.20, 0.60),
            BlockEngagementMode::Quiet
        );
        assert_eq!(
            BlockEngagementMode::from_morph(0.50, 0.20, 0.20),
            BlockEngagementMode::Storm
        );
        // 598→599 style: quiet collapses, mixed+fan_out rise
        assert_eq!(
            BlockEngagementMode::from_morph(0.25, 0.40, 0.25),
            BlockEngagementMode::Storm
        );
    }

    #[test]
    fn maybe_flip_counts_switch() {
        let eng = AdaptiveEngagement::new(4, true);
        assert!(!eng.is_storm());
        assert!(eng.maybe_flip_mode(0.50, 0.20, 0.20));
        assert!(eng.is_storm());
        assert_eq!(eng.engagement_switches(), 1);
        assert!(!eng.maybe_flip_mode(0.50, 0.20, 0.20));
        assert_eq!(eng.engagement_switches(), 1);
    }
}
