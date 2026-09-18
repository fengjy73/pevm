//! SpecFence parallel computer — Spec validate / OCC helpers.
//!
//! Owns SpecFence **validate** (CC Resolve). Ready/steal lives in `computer.rs` (PC).
//! Spec-only incarnations use the shared OCC validate kernel (bool walk + full_abort_reexecute).
//! partial_abort Resolve runs only when a certificate **strip** covers fail locations.
//!
//! Protocol: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.

use super::ConcurrencyMode;
use super::LeanAbortRepair;
use super::SpecFenceCtx;
use super::certificate::CertificateTable;
use super::collateral::{ConflictClass, classify_first_conflict, commute_ok, location_is_lazy};
use super::dag::FenceGraph;
use super::learner::LiveLearner;
use super::repair::{RepairGrain, repair_grain};
use super::wave::WaveParkTable;
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
    cert: &CertificateTable,
    tx_idx: TxIdx,
) -> bool {
    mode == ConcurrencyMode::SpecFence && cert.may_resolve(tx_idx)
}

/// partial_abort museum only when the strip covers **every** invalid read (v6 §5).
#[inline]
pub(crate) fn specfence_partial_abort_validate(
    mode: ConcurrencyMode,
    cert: &CertificateTable,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> bool {
    mode == ConcurrencyMode::SpecFence
        && !invalid.is_empty()
        && repair_grain(cert, tx_idx, invalid) == RepairGrain::PartialAbort
}

/// **Deprecated name** (`specfence_cost_class_spec`). v9.3: this is **not**
/// a computer switch. Cold Spec cost-class (empty PE). Quiet-off alone must
/// **not** switch the scheduler to `next_occ_task` (no second OCC engine).
#[inline]
pub(crate) fn specfence_plant_is_occ(mode: ConcurrencyMode, learner: &LiveLearner) -> bool {
    specfence_cost_class_spec(mode, learner)
}

/// SpecFence cold = Mode(a)=A0 OptimisticRead on the **same** spine (zero Fence meta).
/// Not a flip to the Occ computer.
#[inline]
pub(crate) fn specfence_cost_class_spec(mode: ConcurrencyMode, learner: &LiveLearner) -> bool {
    mode != ConcurrencyMode::SpecFence || !learner.has_any_predicted()
}

/// Per-access optimistic_read cost class: empty PE **or** this \(\ell\) has no PE class.
#[inline]
pub(crate) fn specfence_access_is_occ(
    mode: ConcurrencyMode,
    learner: &LiveLearner,
    location: crate::MemoryLocationHash,
) -> bool {
    specfence_cost_class_spec(mode, learner) || !learner.location_predicted(location)
}

/// OCC schedule — zero SpecFence symbols.
#[inline]
pub(crate) fn next_occ_task(scheduler: &Scheduler) -> Option<Task> {
    scheduler.next_task()
}

/// OCC validate stage: bool walk + full_abort_reexecute estimates. Abort counters only (no rem).
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
            m.record_full_abort_reexecute();
        }
    }
    scheduler.finish_validation(tx_version, aborted)
}

/// C1+C2: record first conflict ℓ and accept a commute without incarnation++.
fn note_and_try_commute(
    specfence: SpecFenceCtx<'_>,
    mv_memory: &MvMemory,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> bool {
    // Commute first — classify/DashMap only when the accept path misses.
    if commute_ok(
        specfence.hints,
        mv_memory,
        specfence.beneficiary,
        tx_idx,
        invalid,
    ) {
        let _ = mv_memory.try_rebind_invalid_reads_value_stable(tx_idx, invalid)
            || mv_memory.try_rebind_invalid_reads(tx_idx, invalid);
        specfence.metrics.record_commute_skip();
        if let Some(p) = specfence.policy {
            p.note_commute_skip();
            p.ignore_conflict(None);
        }
        return true;
    }
    let first = classify_first_conflict(
        specfence.hints,
        mv_memory,
        specfence.beneficiary,
        tx_idx,
        invalid,
    );
    if let Some(f) = first
        && let Some(p) = specfence.policy
    {
        p.note_conflict_ell(tx_idx, f.location, f.peer, f.class, f.lazy);
    }
    false
}

/// CC-R3: park an off-edge abort behind the unfinished writer (no suffix storm).
/// Gated / non-thin validate still owns this; A1=0 uses `occ_abort_ungated`.
#[allow(dead_code)]
fn batch_park_abort(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
    invalid: &[MemoryLocationHash],
) -> Option<Task> {
    let writer = invalid.iter().find_map(|&loc| {
        mv_memory
            .last_writer_before(loc, tx_version.tx_idx)
            .filter(|&w| w < tx_version.tx_idx && !scheduler.is_done(w))
    });
    if !scheduler.try_validation_abort(tx_version) {
        return scheduler.finish_validation(tx_version, false);
    }
    mv_memory.convert_writes_to_estimates(tx_version.tx_idx);
    specfence.metrics.record_occ_abort();
    specfence.metrics.record_full_abort_reexecute();
    if let Some(p) = specfence.policy {
        p.note_wave_off_edge_reexec();
        if let Some(f) = classify_first_conflict(
            specfence.hints,
            mv_memory,
            specfence.beneficiary,
            tx_version.tx_idx,
            invalid,
        ) {
            match f.class {
                ConflictClass::EffectiveWAW => {
                    promote_and_seed_short_edge(specfence, p, tx_version.tx_idx, &f);
                }
                ConflictClass::LazyNoise | ConflictClass::CommuteCandidate => {
                    p.ignore_conflict(Some(f.location));
                }
            }
        }
    }
    if let Some(w) = writer
        && scheduler.add_dependency_from_aborting(tx_version.tx_idx, w)
    {
        specfence.metrics.record_batch_repair();
        if let Some(p) = specfence.policy {
            p.note_batch_repair();
        }
        return scheduler.finish_validation_fenced_barrier_park(tx_version, None);
    }
    scheduler.finish_validation(tx_version, true)
}

/// L2: first EffectiveWAW abort → record ℓ and OrderedAdmit idle successors.
fn promote_and_seed_short_edge(
    specfence: SpecFenceCtx<'_>,
    policy: &super::policy::CostPolicy,
    tx_idx: TxIdx,
    f: &super::collateral::FirstConflict,
) {
    policy.promote_short_edge(f.location, 0);
    let producer = f.peer.filter(|&w| w < tx_idx).unwrap_or(tx_idx);
    crate::specfence::admit::admit_seed_after_effective_abort(
        specfence.ready_edges,
        specfence.hints,
        Some(policy),
        tx_idx,
        producer,
        f.location,
    );
}

/// A0 / ungated: OCC abort after a failed commute. L2 still promotes the ℓ.
fn occ_abort_ungated(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
    invalid: &[MemoryLocationHash],
) -> Option<Task> {
    let first = specfence.policy.and_then(|_| {
        classify_first_conflict(
            specfence.hints,
            mv_memory,
            specfence.beneficiary,
            tx_version.tx_idx,
            invalid,
        )
    });
    let aborted = scheduler.try_validation_abort(tx_version);
    if aborted {
        mv_memory.convert_writes_to_estimates(tx_version.tx_idx);
        specfence.metrics.record_occ_abort();
        specfence.metrics.record_full_abort_reexecute();
        if let Some(p) = specfence.policy {
            p.note_wave_off_edge_reexec();
            if let Some(f) = first {
                match f.class {
                    ConflictClass::EffectiveWAW => {
                        promote_and_seed_short_edge(specfence, p, tx_version.tx_idx, &f);
                    }
                    ConflictClass::LazyNoise | ConflictClass::CommuteCandidate => {
                        p.ignore_conflict(Some(f.location));
                    }
                }
            }
        }
    }
    scheduler.finish_validation(tx_version, aborted)
}

/// P1: A0 validate — OCC-identical on the no-conflict path; commute only on miss.
pub(crate) fn validate_a0_fast(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
) -> Option<Task> {
    if occ_read_set_valid(mv_memory, tx_version.tx_idx) {
        return scheduler.finish_validation(tx_version, false);
    }
    let invalid = mv_memory.collect_invalid_reads(tx_version.tx_idx);
    if note_and_try_commute(specfence, mv_memory, tx_version.tx_idx, &invalid) {
        return scheduler.finish_validation(tx_version, false);
    }
    occ_abort_ungated(mv_memory, scheduler, tx_version, specfence, &invalid)
}

/// Spec-only validate: same OCC kernel. Fail ⇒ full_abort_reexecute + learn PE at **true \(k\)**.
/// Never RebindThis / PrefixSkip — journal-less repair is a protocol bug.
pub(crate) fn validate_occ_kernel(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
) -> Option<Task> {
    let valid = occ_read_set_valid(mv_memory, tx_version.tx_idx);
    if valid {
        return scheduler.finish_validation(tx_version, false);
    }
    specfence.metrics.record_occ_kernel_validate();
    let invalid = mv_memory.collect_invalid_reads(tx_version.tx_idx);
    if note_and_try_commute(specfence, mv_memory, tx_version.tx_idx, &invalid) {
        return scheduler.finish_validation(tx_version, false);
    }
    let a0_ungated = specfence.policy.is_some_and(|p| p.is_a0_majority_block())
        && !specfence.ready_edges.is_gated(tx_version.tx_idx);
    if a0_ungated {
        return occ_abort_ungated(mv_memory, scheduler, tx_version, specfence, &invalid);
    }
    let aborted = scheduler.try_validation_abort(tx_version);
    if !aborted {
        return scheduler.finish_validation(tx_version, false);
    }
    let write_locations = mv_memory.write_locations(tx_version.tx_idx);
    mv_memory.convert_writes_to_estimates(tx_version.tx_idx);
    specfence.metrics.record_occ_abort();
    specfence.metrics.record_full_abort_reexecute();
    if !invalid.is_empty() {
        specfence.metrics.record_region_validate_fail(invalid.len());
    }
    specfence
        .partial_retry
        .clear_force_ordered_admit(tx_version.tx_idx);
    specfence
        .partial_retry
        .clear_force_writers(tx_version.tx_idx);
    specfence.partial_retry.clear_repair(tx_version.tx_idx);
    specfence.partial_retry.clear_ff_head(tx_version.tx_idx);
    specfence
        .partial_retry
        .clear_suffix_repair_depth(tx_version.tx_idx);

    let cascade_hint = invalid.len().max(1);
    let queued = specfence.ready_edges.was_queued(tx_version.tx_idx);
    let first = classify_first_conflict(
        specfence.hints,
        mv_memory,
        specfence.beneficiary,
        tx_version.tx_idx,
        &invalid,
    );
    if let Some(p) = specfence.policy {
        if !queued {
            p.note_wave_off_edge_reexec();
        }
        if let Some(f) = first {
            match f.class {
                ConflictClass::EffectiveWAW => {
                    promote_and_seed_short_edge(specfence, p, tx_version.tx_idx, &f);
                }
                ConflictClass::LazyNoise | ConflictClass::CommuteCandidate => {
                    p.ignore_conflict(Some(f.location));
                }
            }
        }
    }
    for location in &invalid {
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
        let lazy = location_is_lazy(mv_memory, tx_version.tx_idx, *location);
        let promote = first
            .is_some_and(|f| f.location == *location && f.class == ConflictClass::EffectiveWAW);
        // LazyNoise / commute must not seed PE or re-A1 a same-from spine.
        if queued || promote {
            crate::specfence::feeder::observe_abort(
                specfence.learner,
                specfence.bayes,
                *location,
                cascade_hint,
                loc_k,
            );
        } else if !lazy {
            crate::specfence::feeder::observe_abort(
                specfence.learner,
                specfence.bayes,
                *location,
                cascade_hint,
                loc_k,
            );
        }
        if let Some(k) = loc_k {
            specfence.sketch.mark_access_class(*location, k);
        }
        if promote
            && let Some(w) = mv_memory.last_writer_before(*location, tx_version.tx_idx)
            && w < tx_version.tx_idx
            && !scheduler.is_done(w)
        {
            crate::specfence::admit::admit_seed_on_abort(
                specfence.ready_edges,
                specfence.producer_stages,
                tx_version.tx_idx,
                w,
                *location,
            );
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
    // OCC-identical suffix cascade + wave ready for park steal.
    // min_higher_reader skip left later txs Validated against ESTIMATE.
    scheduler.finish_validation_fenced(
        tx_version,
        true,
        Some(tx_version.tx_idx + 1),
        Some(specfence.wave),
    )
}

/// CC Resolve: split RS_spec / RS_fence. Never always-full_abort_reexecute while certs exist.
///
/// No strip → OCC full_abort_reexecute + PE(true k). `covers_all` → PartialAbortRebind rebind; else full_abort_reexecute.
pub(crate) fn validate_specfence(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
) -> Option<Task> {
    let has_cert = specfence.certificates.has_any(tx_version.tx_idx)
        || specfence.certificates.may_resolve(tx_version.tx_idx)
        || specfence.certificates.may_resolve(tx_version.tx_idx);
    if !has_cert {
        return validate_occ_kernel(mv_memory, scheduler, tx_version, specfence);
    }

    specfence.metrics.record_occ_kernel_validate();
    if occ_read_set_valid(mv_memory, tx_version.tx_idx) {
        return scheduler.finish_validation(tx_version, false);
    }

    let invalid = mv_memory.collect_invalid_reads(tx_version.tx_idx);
    if note_and_try_commute(specfence, mv_memory, tx_version.tx_idx, &invalid) {
        return scheduler.finish_validation(tx_version, false);
    }
    // Thin-shell A0: commute already tried; failed commute ≡ OCC abort.
    let a0_ungated = specfence.policy.is_some_and(|p| p.is_a0_majority_block())
        && !specfence.ready_edges.is_gated(tx_version.tx_idx);
    if a0_ungated {
        return occ_abort_ungated(mv_memory, scheduler, tx_version, specfence, &invalid);
    }
    if !invalid.is_empty() {
        let grain = repair_grain(specfence.certificates, tx_version.tx_idx, &invalid);
        let covers = grain == RepairGrain::PartialAbort;
        let selective: Vec<_> = if covers {
            Vec::new()
        } else {
            invalid
                .iter()
                .copied()
                .filter(|&loc| specfence.certificates.covers(tx_version.tx_idx, loc))
                .collect()
        };
        let fenced: &[MemoryLocationHash] = if covers { &invalid } else { &selective };
        let bayes_q = invalid.first().copied().map(|loc| {
            let w_exec = mv_memory
                .last_writer_before(loc, tx_version.tx_idx)
                .is_some_and(|w| scheduler.is_executing(w));
            specfence
                .bayes
                .query_validate(loc, !fenced.is_empty(), w_exec)
        });
        if !fenced.is_empty() {
            let estimate_cleared = fenced.iter().all(|&loc| {
                mv_memory
                    .current_data_value(tx_version.tx_idx, loc)
                    .is_some()
            });
            // M3: snap / FF identity **or** incarnation-strict same-output.
            // Never rebind on identity_held alone (seq≠par).
            let identity_held = specfence
                .partial_retry
                .identity_held(tx_version.tx_idx, fenced);
            let value_stable = estimate_cleared
                && fenced.iter().all(|&loc| {
                    let Some(cur) = mv_memory.current_data_value(tx_version.tx_idx, loc) else {
                        return false;
                    };
                    specfence
                        .partial_retry
                        .identity_stable_match(tx_version.tx_idx, loc, &cur)
                        || mv_memory.prior_read_value_stable(tx_version.tx_idx, loc)
                });
            if identity_held {
                specfence.learner.note_identity_hit();
            }
            let partial_abort_rebind = value_stable
                && mv_memory.try_rebind_invalid_reads_value_stable(tx_version.tx_idx, fenced)
                && (fenced.len() == invalid.len()
                    || occ_read_set_valid(mv_memory, tx_version.tx_idx));
            if partial_abort_rebind {
                specfence.learner.note_resolve_partial_abort();
                specfence.metrics.record_rebind_only();
                specfence.metrics.record_partial_abort_win();
                specfence.metrics.record_partial_retry();
                specfence
                    .partial_retry
                    .clear_force_ordered_admit(tx_version.tx_idx);
                specfence
                    .partial_retry
                    .clear_force_writers(tx_version.tx_idx);
                specfence.partial_retry.clear_repair(tx_version.tx_idx);
                specfence.partial_retry.clear_ff_head(tx_version.tx_idx);
                specfence
                    .partial_retry
                    .clear_suffix_repair_depth(tx_version.tx_idx);
                specfence.learner.note_reexec_cost(0.1);
                return scheduler.finish_validation(tx_version, false);
            }
            // PartialAbortRewind: **strip**-covered fail → RewindTo. repair_armed covers_all
            // must not skip sibling optimistic_read (Iter26 seq≠par). Soft=0.
            let strip_covers = specfence
                .certificates
                .covers_strips_all(tx_version.tx_idx, &invalid);
            // Validate port: PartialAbortRewind only when Bayes EV says cover beats full abort.
            let rewind_ev =
                bayes_q.is_none_or(|q| q.depth_frac >= 0.50 || q.ev_ordered_admit_beats_full_abort);
            if strip_covers && rewind_ev {
                let read_locations = mv_memory.read_locations(tx_version.tx_idx);
                let write_locations = mv_memory.write_locations(tx_version.tx_idx);
                if let Some(LeanAbortRepair::SuffixRepair {
                    suffix_writes,
                    reexec_cost,
                    ..
                }) = specfence.partial_retry.try_arm_partial_abort_rewind(
                    tx_version.tx_idx,
                    &read_locations,
                    &invalid,
                    &write_locations,
                ) {
                    specfence.metrics.record_partial_abort_attempt();
                    if scheduler.try_validation_abort(tx_version) {
                        let estimated =
                            mv_memory.invalidate_partial_suffix(tx_version.tx_idx, &suffix_writes);
                        if !estimated.is_empty() {
                            specfence
                                .metrics
                                .record_selective_invalidate(estimated.len());
                        }
                        specfence.learner.note_resolve_partial_abort();
                        specfence.metrics.record_partial_abort_win();
                        specfence.metrics.record_rewind_to_cp();
                        specfence.metrics.record_partial_retry();
                        specfence.learner.note_reexec_cost(reexec_cost);
                        specfence
                            .partial_retry
                            .mark_needs_live_capture(tx_version.tx_idx);
                        return scheduler.finish_validation_fenced(
                            tx_version,
                            true,
                            Some(tx_version.tx_idx + 1),
                            Some(specfence.wave),
                        );
                    }
                }
                // Strips cover but PartialAbortRewind cannot arm (empty prefix / already
                // rewound once): honest OCC full_abort_reexecute. Never-full_abort_reexecute ForceOrderedAdmit livelocked
                // 19807137. Do not increment partial_abort_attempt (that was the theater).
            }
        }
    }

    validate_occ_kernel(mv_memory, scheduler, tx_version, specfence)
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
        let k = CertificateTable::new(2);
        assert!(!uses_specfence_resolve(ConcurrencyMode::Occ, &k, 0));
        assert!(!uses_specfence_resolve(ConcurrencyMode::SpecFence, &k, 0));
        k.note_success(0, 1);
        assert!(uses_specfence_resolve(ConcurrencyMode::SpecFence, &k, 0));
        let cert = CertificateTable::new(1);
        cert.begin_execute(0, false, 0);
        assert!(!specfence_partial_abort_validate(
            ConcurrencyMode::SpecFence,
            &cert,
            0,
            &[7]
        ));
        cert.note_success(0, 7);
        assert!(specfence_partial_abort_validate(
            ConcurrencyMode::SpecFence,
            &cert,
            0,
            &[7]
        ));
        assert!(!specfence_partial_abort_validate(
            ConcurrencyMode::SpecFence,
            &cert,
            0,
            &[7, 9]
        ));
    }

    #[test]
    fn quiet_lone_pe_stays_on_specfence_spine() {
        let live = LiveLearner::new();
        live.begin_block(crate::specfence::learner::MorphWeights::default());
        live.note_abort_access(7, 2, Some(6));
        assert!(live.has_any_predicted(), "intra abort may arm PE");
        assert!(
            !specfence_cost_class_spec(ConcurrencyMode::SpecFence, &live),
            "v9.3: PE-on must not retreat to an OCC computer"
        );
        assert!(
            live.quiet_pessimistic_off(),
            "quiet morph still holds Fence verbs (2179522)"
        );
    }
}
