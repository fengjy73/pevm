//! SpecFence validate — Validate.to_resolve → Resolve.apply (SF-PS §C).
//!
//! Edged paths produce a [`ResolvePlan`] (rebind / rewind / ordered replay /
//! full replay). Independent / Opt is Avoid=noop: optimistic validate then
//! Commit or FullReplay. That is **not** `ConcurrencyMode::Occ`.
//!
//! OCC helpers (`next_occ_task`, `validate_occ_stage`) are the contrast
//! engine only.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::ConcurrencyMode;
use super::LeanAbortRepair;
use super::SpecFenceCtx;
use super::VisibilityPolicy;
use super::certificate::CertificateTable;
use super::collateral::{
    ConflictClass, classify_first_conflict, commute_location_ok, commute_ok, is_value_transfer,
    location_is_lazy,
};
use super::dag::FenceGraph;
use super::learner::LiveLearner;
use super::repair::{RepairGrain, repair_grain};
use super::resolve_plan::{ResolvePlan, try_early_waw_rewind};
use super::wave::WaveParkTable;
use crate::mv_memory::MvMemory;
use crate::scheduler::Scheduler;
use crate::{MemoryEntry, MemoryLocationHash, MemoryValue, Task, TxIdx, TxVersion};

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

/// Process-wide OCC pick counter. SpecFence Schedule.pick must not increment this.
static OCC_PICK_CALLS: AtomicUsize = AtomicUsize::new(0);

/// OCC schedule — zero SpecFence symbols. Contrast engine only.
#[inline]
pub(crate) fn next_occ_task(scheduler: &Scheduler) -> Option<Task> {
    OCC_PICK_CALLS.fetch_add(1, Ordering::Relaxed);
    scheduler.next_task()
}

/// Snapshot of `next_occ_task` calls since last reset.
#[inline]
pub(crate) fn occ_pick_calls() -> usize {
    OCC_PICK_CALLS.load(Ordering::Relaxed)
}

/// Reset before a SpecFence block so tests can prove SF pick never entered OCC.
#[inline]
pub(crate) fn reset_occ_pick_calls() {
    OCC_PICK_CALLS.store(0, Ordering::Relaxed);
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
/// Accept path is a single `last_locations` lock (no collect Vec + rebind).
/// A first-touch absolute Basic on a shared ℓ resets later lazy evaluation
/// (higher-idx Basic after lower LazyRecipient). Commute must not keep it.
fn wrote_shared_absolute_basic(mv_memory: &MvMemory, tx_idx: TxIdx) -> bool {
    mv_memory.write_locations(tx_idx).iter().any(|&loc| {
        let Some(written) = mv_memory.data.get(&loc) else {
            return false;
        };
        matches!(
            written.get(&tx_idx),
            Some(MemoryEntry::Data(_, MemoryValue::Basic(_)))
        ) && written.keys().any(|&w| w != tx_idx)
    })
}

fn note_and_try_commute(
    specfence: SpecFenceCtx<'_>,
    mv_memory: &MvMemory,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> bool {
    if wrote_shared_absolute_basic(mv_memory, tx_idx) {
        return false;
    }
    if is_value_transfer(specfence.hints, tx_idx)
        && mv_memory.try_commute_rebind_invalid(tx_idx, |loc| {
            commute_location_ok(
                specfence.hints,
                mv_memory,
                specfence.beneficiary,
                tx_idx,
                loc,
            )
        })
    {
        specfence.metrics.record_commute_skip();
        if let Some(p) = specfence.policy {
            p.note_commute_skip();
        }
        return true;
    }
    if !invalid.is_empty()
        && commute_ok(
            specfence.hints,
            mv_memory,
            specfence.beneficiary,
            tx_idx,
            invalid,
        )
    {
        let _ = mv_memory.try_rebind_invalid_reads_value_stable(tx_idx, invalid)
            || mv_memory.try_rebind_invalid_reads(tx_idx, invalid);
        specfence.metrics.record_commute_skip();
        if let Some(p) = specfence.policy {
            p.note_commute_skip();
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
/// Wired on the single-pay OCC abort path (C1) — not a long-spine Win prefix.
fn batch_park_abort(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
    invalid: &[MemoryLocationHash],
) -> Option<Task> {
    // C1: park only behind an *Executing* writer (Iter3/8). Aborting/Ready
    // leftover writers would serialize the Opt/Defer spine and raise wall
    // vs harness OCC (Estimate-park train). Unfinished-but-idle → OCC reexec.
    let writer = invalid.iter().find_map(|&loc| {
        mv_memory
            .last_writer_before(loc, tx_version.tx_idx)
            .filter(|&w| w < tx_version.tx_idx && scheduler.is_executing(w))
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

/// CC-L1/L2: first EffectiveWAW abort → OrderedAdmit retry + one successor hop.
fn promote_and_seed_short_edge(
    specfence: SpecFenceCtx<'_>,
    policy: &super::policy::CostPolicy,
    tx_idx: TxIdx,
    f: &super::collateral::FirstConflict,
) {
    policy.promote_short_edge(f.location, 0);
    let producer = f.peer.filter(|&w| w < tx_idx).unwrap_or(tx_idx);
    crate::specfence::admit::persist_short_chain_after_abort(
        specfence.hints,
        policy,
        tx_idx,
        producer,
        f.location,
    );
    // T3: queue the next Win_w idle hops. Do **not** flush here —
    // validate-time plant + same-thread re-exec livelocks (done-stamp race).
    let n_pairs = policy.pairs_of(f.location).len();
    if policy.hops_to_admit(f.location, n_pairs) > 0 {
        let from = specfence.hints.from_of(tx_idx);
        let to = specfence.hints.to_of(tx_idx);
        crate::specfence::admit::queue_nearest_unfinished_successor(
            specfence.ready_edges,
            policy,
            specfence.hints,
            f.location,
            tx_idx,
            from,
            to,
        );
    }
    // CC-L2: incarnation≥1 is idle at the next pick quantum (`flush` there).
    specfence.ready_edges.clear_started(tx_idx);
}

/// A0 / ungated: OCC abort after a failed commute. C1: park behind the
/// unfinished writer when one exists (batch repair) so we do not immediately
/// reexec against ESTIMATE. L2 still promotes the ℓ for the next begin.
fn occ_abort_ungated(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
    invalid: &[MemoryLocationHash],
) -> Option<Task> {
    batch_park_abort(mv_memory, scheduler, tx_version, specfence, invalid)
}

/// Avoid=noop independent-set validate (VisibilityPolicy::Opt).
///
/// Implementation reuses the optimistic bool walk + commute. This is
/// SpecFence DAG antichain validation — **not** `ConcurrencyMode::Occ`.
pub(crate) fn validate_optimistic_fast(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
) -> Option<Task> {
    if occ_read_set_valid(mv_memory, tx_version.tx_idx) {
        specfence.metrics.record_resolve_plan(ResolvePlan::Commit);
        return scheduler.finish_validation(tx_version, false);
    }
    if !is_value_transfer(specfence.hints, tx_version.tx_idx) {
        specfence
            .metrics
            .record_resolve_plan(ResolvePlan::FullReplay);
        return validate_occ_stage(mv_memory, scheduler, tx_version, Some(specfence.metrics));
    }
    if note_and_try_commute(specfence, mv_memory, tx_version.tx_idx, &[]) {
        specfence.metrics.record_resolve_plan(ResolvePlan::Commit);
        return scheduler.finish_validation(tx_version, false);
    }
    let invalid = mv_memory.collect_invalid_reads(tx_version.tx_idx);
    specfence
        .metrics
        .record_resolve_plan(ResolvePlan::FullReplay);
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
        specfence.metrics.record_resolve_plan(ResolvePlan::Commit);
        return scheduler.finish_validation(tx_version, false);
    }
    specfence.metrics.record_occ_kernel_validate();
    if note_and_try_commute(specfence, mv_memory, tx_version.tx_idx, &[]) {
        specfence.metrics.record_resolve_plan(ResolvePlan::Commit);
        return scheduler.finish_validation(tx_version, false);
    }
    let invalid = mv_memory.collect_invalid_reads(tx_version.tx_idx);
    // Avoid=noop Opt (ungated): FullReplay, still on the SpecFence spine.
    if !specfence.ready_edges.is_gated(tx_version.tx_idx) {
        specfence
            .metrics
            .record_resolve_plan(ResolvePlan::FullReplay);
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
    let mut resolve = ResolvePlan::FullReplay;
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
                    resolve = ResolvePlan::OrderedReplay;
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
    specfence.metrics.record_resolve_plan(resolve);
    // Suffix cascade + wave ready for park steal. Repeated FullReplay
    // feeds Detect/arm (deeper cover) — not “this belongs to OCC”.
    scheduler.finish_validation_fenced(
        tx_version,
        true,
        Some(tx_version.tx_idx + 1),
        Some(specfence.wave),
    )
}

/// Same-output invalid reads can be patched in place. Refuses Estimate and
/// any location whose published data does not match the read the tx took.
fn salvage_value_stable_rebind(
    mv_memory: &MvMemory,
    specfence: SpecFenceCtx<'_>,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> bool {
    if invalid.is_empty() {
        return false;
    }
    let value_stable = invalid.iter().all(|&loc| {
        let Some(cur) = mv_memory.current_data_value(tx_idx, loc) else {
            return false;
        };
        specfence
            .partial_retry
            .identity_stable_match(tx_idx, loc, &cur)
            || mv_memory.prior_read_value_stable(tx_idx, loc)
    });
    value_stable && mv_memory.try_rebind_invalid_reads_value_stable(tx_idx, invalid)
}

/// Validate → [`ResolvePlan`]. Does **not** abort or finish validation.
/// Edged paths never call [`validate_occ_kernel`] / [`validate_occ_stage`].
pub(crate) fn validate_to_plan(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
    vis: VisibilityPolicy,
) -> (ResolvePlan, Vec<MemoryLocationHash>) {
    if occ_read_set_valid(mv_memory, tx_version.tx_idx) {
        return (ResolvePlan::Commit, Vec::new());
    }
    if note_and_try_commute(specfence, mv_memory, tx_version.tx_idx, &[]) {
        return (ResolvePlan::Commit, Vec::new());
    }
    let invalid = mv_memory.collect_invalid_reads(tx_version.tx_idx);
    if note_and_try_commute(specfence, mv_memory, tx_version.tx_idx, &invalid) {
        return (ResolvePlan::Commit, invalid);
    }
    // P1: value-stable rebind is salvage on Opt and edged paths. It commits
    // this incarnation instead of an unfenced FullReplay.
    if salvage_value_stable_rebind(mv_memory, specfence, tx_version.tx_idx, &invalid) {
        return (ResolvePlan::PartialAbortRebind, invalid);
    }
    if vis.is_opt() {
        // Early-k WAW: a mid-tx checkpoint before fail_k arms hang-free
        // RewindTo (Indep, no live_capture). Otherwise FullReplay + ff_head.
        if let Some(plan) = try_early_waw_rewind(mv_memory, tx_version, specfence, &invalid) {
            return (plan, invalid);
        }
        return (ResolvePlan::FullReplay, invalid);
    }

    // Edged: Resolve first. No OCC-kernel fallback.
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
                return (ResolvePlan::PartialAbortRebind, invalid);
            }
            let strip_covers = specfence
                .certificates
                .covers_strips_all(tx_version.tx_idx, &invalid);
            let rewind_ev =
                bayes_q.is_none_or(|q| q.depth_frac >= 0.50 || q.ev_ordered_admit_beats_full_abort);
            if strip_covers && rewind_ev {
                let read_locations = mv_memory.read_locations(tx_version.tx_idx);
                let write_locations = mv_memory.write_locations(tx_version.tx_idx);
                if specfence
                    .partial_retry
                    .try_arm_partial_abort_rewind(
                        tx_version.tx_idx,
                        &read_locations,
                        &invalid,
                        &write_locations,
                    )
                    .is_some()
                {
                    return (ResolvePlan::PartialAbortRewind, invalid);
                }
            }
        }
        let first = classify_first_conflict(
            specfence.hints,
            mv_memory,
            specfence.beneficiary,
            tx_version.tx_idx,
            &invalid,
        );
        if specfence.ready_edges.was_queued(tx_version.tx_idx)
            || first.is_some_and(|f| f.class == ConflictClass::EffectiveWAW)
        {
            // Same rule as Opt: do not restart this read as Ordered-from-0.
            return (ResolvePlan::FullReplay, invalid);
        }
    }
    (ResolvePlan::FullReplay, invalid)
}

/// Validate.to_resolve → Resolve.apply. SpecFence product validate entry.
///
/// Opt / independent: Avoid=noop optimistic validate (Commit | FullReplay).
/// Edged: PartialAbortRebind / Rewind / OrderedReplay before FullReplay.
pub(crate) fn validate_specfence(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
) -> Option<Task> {
    let vis = VisibilityPolicy::for_ready(specfence.ready_edges, tx_version.tx_idx);
    if vis.is_opt() {
        return validate_optimistic_fast(mv_memory, scheduler, tx_version, specfence);
    }

    let has_cert = specfence.certificates.has_any(tx_version.tx_idx)
        || specfence.certificates.may_resolve(tx_version.tx_idx);
    if !has_cert {
        return validate_occ_kernel(mv_memory, scheduler, tx_version, specfence);
    }

    specfence.metrics.record_occ_kernel_validate();
    if occ_read_set_valid(mv_memory, tx_version.tx_idx) {
        specfence.metrics.record_resolve_plan(ResolvePlan::Commit);
        return scheduler.finish_validation(tx_version, false);
    }
    if note_and_try_commute(specfence, mv_memory, tx_version.tx_idx, &[]) {
        specfence.metrics.record_resolve_plan(ResolvePlan::Commit);
        return scheduler.finish_validation(tx_version, false);
    }

    let invalid = mv_memory.collect_invalid_reads(tx_version.tx_idx);
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
                specfence
                    .metrics
                    .record_resolve_plan(ResolvePlan::PartialAbortRebind);
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
                        specfence
                            .metrics
                            .record_resolve_plan(ResolvePlan::PartialAbortRewind);
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
    fn validate_to_plan_source_never_falls_back_to_occ_kernel() {
        let src = include_str!("executor.rs");
        let before_specfence = src
            .split("pub(crate) fn validate_specfence")
            .next()
            .unwrap();
        let to_plan = before_specfence
            .split("pub(crate) fn validate_to_plan")
            .nth(1)
            .expect("validate_to_plan present");
        assert!(
            !to_plan.contains("validate_occ_kernel") && !to_plan.contains("validate_occ_stage"),
            "edged/opt to_plan must not fallback to OCC validate"
        );
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
