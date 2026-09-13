//! SpecFence first-class executor ticks — schedule / validate / resolve.
//!
//! Shares **revm + `MvMemory`** with OCC. Does **not** share access intercepts.
//! Unfenced reads are dispatched to OCC helpers from `access_policy`, not here.
//!
//! Authoritative plant: `lab/notes/specfence-clean-slate-architecture.md`.

use super::ConcurrencyMode;
use super::dag::FenceGraph;
use super::rem::WaveParkTable;
use crate::TxIdx;
use crate::mv_memory::MvMemory;

/// OCC schedule / execute / validate never take wave or fence handles.
#[inline]
pub(crate) fn wave_for_mode(mode: ConcurrencyMode, wave: &WaveParkTable) -> Option<&WaveParkTable> {
    matches!(mode, ConcurrencyMode::SpecFence).then_some(wave)
}

/// `FenceGraph` wake is SpecFence-only (OCC has no SoftWait / Data-wake).
#[inline]
pub(crate) fn fence_for_mode(mode: ConcurrencyMode, dag: &FenceGraph) -> Option<&FenceGraph> {
    matches!(mode, ConcurrencyMode::SpecFence).then_some(dag)
}

/// OCC validate is a single read-set walk. SpecFence may overlay Resolve.
#[inline]
pub(crate) fn occ_read_set_valid(mv_memory: &MvMemory, tx_idx: TxIdx) -> bool {
    mv_memory.validate_read_locations(tx_idx)
}

/// Hinted account Wait is **PCC-legacy only**. SpecFence π is access-grain.
#[inline]
pub(crate) fn hinted_wait_enabled(mode: ConcurrencyMode) -> bool {
    mode == ConcurrencyMode::Pcc
}

/// SpecFence Resolve overlay (R1a / PrefixSkip / B0) runs only in this mode.
#[inline]
pub(crate) fn uses_specfence_resolve(mode: ConcurrencyMode) -> bool {
    mode == ConcurrencyMode::SpecFence
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occ_never_takes_wave_or_fence() {
        assert!(wave_for_mode(ConcurrencyMode::Occ, &WaveParkTable::new()).is_none());
        assert!(fence_for_mode(ConcurrencyMode::Occ, &FenceGraph::new()).is_none());
        assert!(!hinted_wait_enabled(ConcurrencyMode::Occ));
        assert!(!hinted_wait_enabled(ConcurrencyMode::SpecFence));
        assert!(hinted_wait_enabled(ConcurrencyMode::Pcc));
        assert!(!uses_specfence_resolve(ConcurrencyMode::Occ));
        assert!(uses_specfence_resolve(ConcurrencyMode::SpecFence));
    }
}
