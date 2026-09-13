//! SpecFence parallel computer — Spec validate / OCC helpers.
//!
//! Owns SpecFence **validate**. Ready/steal lives in `computer.rs`.
//! Spec-only incarnations use the shared OCC validate kernel (bool walk + B0).
//! R1 Resolve runs only when a certificate **strip** covers fail locations.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v6-essence.md`.

use super::ConcurrencyMode;
use super::SpecFenceCtx;
use super::certificate::CertificateTable;
use super::dag::FenceGraph;
use super::kernel::KernelTable;
use super::learner::LiveLearner;
use super::rem::WaveParkTable;
use super::repair::{RepairGrain, repair_grain};
use crate::mv_memory::MvMemory;
use crate::scheduler::Scheduler;
use crate::{MemoryLocationHash, Task, TxIdx, TxVersion};

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

/// SpecFence rem overlay: any successful Fence strip or repair prefix.
#[inline]
pub(crate) fn uses_specfence_resolve(
    mode: ConcurrencyMode,
    kernel: &KernelTable,
    tx_idx: TxIdx,
) -> bool {
    mode == ConcurrencyMode::SpecFence && kernel.may_resolve(tx_idx)
}

/// R1 museum only when the strip covers **every** invalid read (v6 §5).
#[inline]
pub(crate) fn specfence_r1_validate(
    mode: ConcurrencyMode,
    cert: &CertificateTable,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> bool {
    mode == ConcurrencyMode::SpecFence
        && !invalid.is_empty()
        && repair_grain(cert, tx_idx, invalid) == RepairGrain::R1
}

/// Empty PE table: quiet path is byte-identical OCC (no ordinal / PE probe).
#[inline]
pub(crate) fn specfence_plant_is_occ(mode: ConcurrencyMode, learner: &LiveLearner) -> bool {
    mode != ConcurrencyMode::SpecFence || !learner.has_any_predicted()
}

/// OCC schedule — zero SpecFence symbols.
#[inline]
pub(crate) fn next_occ_task(scheduler: &Scheduler) -> Option<Task> {
    scheduler.next_task()
}

/// OCC validate stage: bool walk + B0 estimates. Abort counters only (no rem).
pub(crate) fn validate_occ_stage(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    metrics: Option<&super::MetricsInner>,
) -> Option<Task> {
    let valid = occ_read_set_valid(mv_memory, tx_version.tx_idx);
    let aborted = !valid && scheduler.try_validation_abort(tx_version);
    if aborted {
        mv_memory.convert_writes_to_estimates(tx_version.tx_idx);
        if let Some(m) = metrics {
            m.record_occ_abort();
            m.record_full_restart();
        }
    }
    scheduler.finish_validation(tx_version, aborted)
}

/// Spec-only validate: same OCC kernel. Fail ⇒ B0 + learn PE at **true \(k\)**.
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
    if !invalid.is_empty() {
        specfence.metrics.record_region_validate_fail(invalid.len());
    }
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
        // True k from AccessOrdinalLog / rem first_k / EdgeKey — never residual 1.
        let loc_k = specfence
            .access_log
            .first_k(tx_version.tx_idx, *location)
            .or_else(|| {
                specfence
                    .partial_retry
                    .first_k(tx_version.tx_idx, *location)
                    .map(|k| k as u32)
            })
            .or_else(|| {
                specfence
                    .edges
                    .min_k_of_location(tx_version.tx_idx, *location)
            })
            .filter(|&k| k > 0);
        specfence
            .learner
            .note_abort_access(*location, cascade_hint, loc_k);
        if let Some(k) = loc_k {
            specfence.sketch.mark_access_class(*location, k);
        }
        if let Some(w) = mv_memory.last_writer_before(*location, tx_version.tx_idx)
            && w < tx_version.tx_idx
            && !scheduler.is_done(w)
        {
            specfence.ready_edges.note_unpublished(*location, w);
            specfence.ready_edges.note_consumer(tx_version.tx_idx, w);
            specfence.sketch.push_spine(*location, w);
        }
    }

    let rewind_to = mv_memory.min_higher_reader_of(tx_version.tx_idx, &write_locations);
    let block_size = scheduler.block_size();
    let cascade_from = tx_version.tx_idx + 1;
    let (cascade, skipped) = match rewind_to {
        Some(to) => {
            let to = to.min(block_size);
            (
                block_size.saturating_sub(to),
                to.saturating_sub(cascade_from),
            )
        }
        None => (0, block_size.saturating_sub(cascade_from)),
    };
    specfence.metrics.record_fence_cascade(cascade, skipped);
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
        k.note_fence(0);
        assert!(uses_specfence_resolve(ConcurrencyMode::SpecFence, &k, 0));
        let cert = CertificateTable::new(1);
        cert.begin_execute(0, false);
        assert!(!specfence_r1_validate(
            ConcurrencyMode::SpecFence,
            &cert,
            0,
            &[7]
        ));
        cert.note_success(0, 7);
        assert!(specfence_r1_validate(
            ConcurrencyMode::SpecFence,
            &cert,
            0,
            &[7]
        ));
        assert!(!specfence_r1_validate(
            ConcurrencyMode::SpecFence,
            &cert,
            0,
            &[7, 9]
        ));
    }
}
