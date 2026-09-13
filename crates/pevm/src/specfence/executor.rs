//! SpecFence parallel computer — schedule / OccKernel validate / steal.
//!
//! Owns SpecFence **stages**. OCC ticks never enter here.
//! Unfenced incarnations use the shared OCC validate kernel (bool walk + B0).
//!
//! Plant SoT: `lab/notes/specfence-parallel-compute-architecture.md`.

use super::ConcurrencyMode;
use super::SpecFenceCtx;
use super::dag::FenceGraph;
use super::kernel::KernelTable;
use super::learner::LiveLearner;
use super::rem::WaveParkTable;
use crate::mv_memory::MvMemory;
use crate::scheduler::Scheduler;
use crate::{Task, TxIdx, TxVersion};

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

/// Shared OCC validate kernel — first-mismatch bool, no Vec, no rem.
#[inline]
pub(crate) fn occ_read_set_valid(mv_memory: &MvMemory, tx_idx: TxIdx) -> bool {
    mv_memory.validate_read_locations(tx_idx)
}

/// Hinted account Wait is **PCC-legacy only**. SpecFence π is access-grain.
#[inline]
pub(crate) fn hinted_wait_enabled(mode: ConcurrencyMode) -> bool {
    mode == ConcurrencyMode::Pcc
}

/// SpecFence Resolve overlay runs only for **PccKernel** incarnations.
#[inline]
pub(crate) fn uses_specfence_resolve(
    mode: ConcurrencyMode,
    kernel: &KernelTable,
    tx_idx: TxIdx,
) -> bool {
    mode == ConcurrencyMode::SpecFence && kernel.is_pcc(tx_idx)
}

/// Empty PE table: access gate skips bump_k (still OccKernel unless Repair armed).
#[inline]
pub(crate) fn specfence_plant_is_occ(mode: ConcurrencyMode, learner: &LiveLearner) -> bool {
    mode != ConcurrencyMode::SpecFence || !learner.has_any_predicted()
}

/// SpecFence ready-set + steal + pipeline (wave deque + Block-STM indices).
#[inline]
pub(crate) fn next_sf_task(scheduler: &Scheduler, wave: &WaveParkTable) -> Option<Task> {
    scheduler.next_task_with_wave(Some(wave))
}

/// OCC schedule — zero SpecFence symbols.
#[inline]
pub(crate) fn next_occ_task(scheduler: &Scheduler) -> Option<Task> {
    scheduler.next_task()
}

/// OCC validate stage: bool walk + B0 estimates. No rem, no SF metrics.
pub(crate) fn validate_occ_stage(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
) -> Option<Task> {
    let valid = occ_read_set_valid(mv_memory, tx_version.tx_idx);
    let aborted = !valid && scheduler.try_validation_abort(tx_version);
    if aborted {
        mv_memory.convert_writes_to_estimates(tx_version.tx_idx);
    }
    scheduler.finish_validation(tx_version, aborted)
}

/// SpecFence OccKernel validate: same OCC kernel. Fail ⇒ B0 + learn PE (repair stage).
/// Never RebindThis / PrefixSkip — journal-less repair is a protocol bug.
pub(crate) fn validate_occ_kernel(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
) -> Option<Task> {
    specfence.metrics.record_occ_kernel_validate();
    let valid = occ_read_set_valid(mv_memory, tx_version.tx_idx);
    let aborted = !valid && scheduler.try_validation_abort(tx_version);
    if !aborted {
        return scheduler.finish_validation(tx_version, false);
    }

    let invalid = mv_memory.collect_invalid_reads(tx_version.tx_idx);
    let write_locations = mv_memory.write_locations(tx_version.tx_idx);
    mv_memory.convert_writes_to_estimates(tx_version.tx_idx);
    specfence.metrics.record_occ_abort();
    specfence.metrics.record_full_restart();
    specfence.partial_retry.clear_force_bind(tx_version.tx_idx);
    specfence
        .partial_retry
        .clear_force_writers(tx_version.tx_idx);
    specfence.partial_retry.clear_repair(tx_version.tx_idx);
    specfence.partial_retry.clear_ff_head(tx_version.tx_idx);
    specfence
        .partial_retry
        .clear_suffix_repair_depth(tx_version.tx_idx);

    let cascade_hint = invalid.len().max(1);
    for location in &invalid {
        specfence.bayes.observe_conflict_location_always(*location);
        specfence.metrics.record_bayes_conflict();
        specfence.hotset.note_abort(*location);
        let loc_k = specfence
            .edges
            .min_k_of_location(tx_version.tx_idx, *location)
            .or_else(|| {
                specfence
                    .partial_retry
                    .first_k(tx_version.tx_idx, *location)
                    .map(|k| k as u32)
            });
        specfence
            .learner
            .note_abort_access(*location, cascade_hint, loc_k);
        if let Some(k) = loc_k.filter(|&k| k > 0) {
            specfence.sketch.mark_access_class(*location, k);
        }
    }

    let rewind_to = mv_memory.min_higher_reader_of(tx_version.tx_idx, &write_locations);
    scheduler.finish_validation_fenced(tx_version, true, rewind_to, Some(specfence.wave))
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
        let k = KernelTable::new(2);
        assert!(!uses_specfence_resolve(ConcurrencyMode::Occ, &k, 0));
        assert!(!uses_specfence_resolve(ConcurrencyMode::SpecFence, &k, 0));
        k.mark_pcc(0);
        assert!(uses_specfence_resolve(ConcurrencyMode::SpecFence, &k, 0));
    }
}
