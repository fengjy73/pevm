//! ResolvePlan — validate produces a structured plan (SF-PS §C / T3).
//!
//! `apply` changes certificates, RunnableSet queues, and write visibility.
//! Edged paths never fall through to `validate_occ_kernel` as the default.

use crate::mv_memory::MvMemory;
use crate::scheduler::Scheduler;
use crate::{MemoryLocationHash, TxVersion};

use super::SpecFenceCtx;
use super::VisibilityPolicy;
use super::arm_table::ArmTable;
use super::collateral::{ConflictClass, classify_first_conflict, location_is_lazy};
use super::runnable_set::{QueueKind, RunnableSet};

/// Structured validate outcome. Replaces “bool valid → abort” as the SF root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvePlan {
    /// Read set still matches; commit this incarnation.
    Commit,
    /// Value-stable rebind of invalid reads (same incarnation).
    PartialAbortRebind,
    /// Certified prefix kept; rewind uncertified suffix once.
    PartialAbortRewind,
    /// Replay the conflict segment under OrderedTip visibility.
    OrderedReplay,
    /// Whole-tx re-enter RunnableSet (Learn penalty). Not “this is OCC”.
    FullReplay,
}

impl ResolvePlan {
    /// True when this plan commits without a new EVM head.
    #[inline]
    pub const fn commits(self) -> bool {
        matches!(self, Self::Commit | Self::PartialAbortRebind)
    }

    /// True when Resolve repaired instead of throwing the tx.
    #[inline]
    pub const fn is_partial(self) -> bool {
        matches!(self, Self::PartialAbortRebind | Self::PartialAbortRewind)
    }
}

/// Inputs for [`apply`].
pub(crate) struct ApplyCtx<'a> {
    pub specfence: SpecFenceCtx<'a>,
    pub mv_memory: &'a MvMemory,
    pub scheduler: &'a Scheduler,
    pub runnable: &'a RunnableSet,
    pub arms: &'a ArmTable,
    pub tx_version: &'a TxVersion,
    pub vis: VisibilityPolicy,
    pub wrote_new_location: bool,
    pub invalid: &'a [MemoryLocationHash],
}

/// Apply a plan: certificates, queues, release, learn. Never returns a
/// Block-STM `Task` — the worker always picks from [`RunnableSet`].
pub(crate) fn apply(plan: ResolvePlan, ctx: ApplyCtx<'_>) {
    let tx = ctx.tx_version.tx_idx;
    ctx.specfence.metrics.record_resolve_plan(plan);
    ctx.specfence.metrics.record_resolve_apply();

    let first = if ctx.invalid.is_empty() {
        None
    } else {
        classify_first_conflict(
            ctx.specfence.hints,
            ctx.mv_memory,
            ctx.specfence.beneficiary,
            tx,
            ctx.invalid,
        )
    };
    let loc = first.map(|f| f.location);
    let lazy = first.is_some_and(|f| f.lazy)
        || ctx
            .invalid
            .iter()
            .any(|&l| location_is_lazy(ctx.mv_memory, tx, l));
    let unfenced = !ctx.specfence.ready_edges.was_queued(tx)
        && matches!(plan, ResolvePlan::FullReplay | ResolvePlan::OrderedReplay);

    match plan {
        ResolvePlan::Commit | ResolvePlan::PartialAbortRebind => {
            if plan == ResolvePlan::PartialAbortRebind {
                ctx.specfence.learner.note_resolve_partial_abort();
                ctx.specfence.metrics.record_rebind_only();
                ctx.specfence.metrics.record_partial_abort_win();
                ctx.specfence.metrics.record_partial_retry();
                clear_retry(ctx.specfence, tx);
            }
            if let Some(l) = loc {
                ctx.specfence.certificates.note_success(tx, l);
            }
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, false, None);
            release_successors(&ctx, tx);
            // Every published write can invalidate a higher reader, including
            // a re-execution that rewrites the same locations (not only
            // WroteNewLocation). Block-STM did this via validation_idx.
            enqueue_higher_revalidate(&ctx, tx, ctx.wrote_new_location);
            ctx.runnable.mark_done(tx);
        }
        ResolvePlan::PartialAbortRewind => {
            ctx.specfence.metrics.record_partial_abort_attempt();
            if ctx.scheduler.try_validation_abort(ctx.tx_version) {
                let write_locations = ctx.mv_memory.write_locations(tx);
                let estimated = ctx.mv_memory.invalidate_partial_suffix(tx, &write_locations);
                if !estimated.is_empty() {
                    ctx.specfence
                        .metrics
                        .record_selective_invalidate(estimated.len());
                }
                ctx.specfence.learner.note_resolve_partial_abort();
                ctx.specfence.metrics.record_partial_abort_win();
                ctx.specfence.metrics.record_rewind_to_cp();
                ctx.specfence.metrics.record_partial_retry();
                ctx.specfence.partial_retry.mark_needs_live_capture(tx);
            }
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, true, Some(tx + 1));
            ctx.specfence.ready_edges.clear_started(tx);
            requeue(&ctx, tx, QueueKind::Released);
            enqueue_higher_revalidate(&ctx, tx, true);
        }
        ResolvePlan::OrderedReplay => {
            abort_and_estimate(&ctx);
            if let Some(f) = first
                && f.class == ConflictClass::EffectiveWAW
                && let Some(p) = ctx.specfence.policy
            {
                seed_short_edge(ctx.specfence, p, tx, &f);
            }
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, true, Some(tx + 1));
            ctx.specfence.ready_edges.clear_started(tx);
            requeue(&ctx, tx, QueueKind::Ordered);
            enqueue_higher_revalidate(&ctx, tx, true);
        }
        ResolvePlan::FullReplay => {
            abort_and_estimate(&ctx);
            if let Some(f) = first {
                match f.class {
                    ConflictClass::EffectiveWAW => {
                        if let Some(p) = ctx.specfence.policy {
                            seed_short_edge(ctx.specfence, p, tx, &f);
                        }
                    }
                    ConflictClass::LazyNoise | ConflictClass::CommuteCandidate => {
                        if let Some(p) = ctx.specfence.policy {
                            p.ignore_conflict(Some(f.location));
                        }
                    }
                }
            }
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, true, Some(tx + 1));
            ctx.specfence.ready_edges.clear_started(tx);
            let kind = if ctx.vis.needs_fence() {
                if ctx.specfence.ready_edges.was_queued(tx) {
                    QueueKind::Ordered
                } else {
                    QueueKind::Released
                }
            } else {
                QueueKind::Indep
            };
            requeue(&ctx, tx, kind);
            enqueue_higher_revalidate(&ctx, tx, true);
        }
    }

    ctx.arms.observe(
        plan,
        loc,
        lazy,
        unfenced,
        ctx.scheduler.block_size(),
        ctx.invalid.len(),
    );
}

fn seed_short_edge(
    specfence: SpecFenceCtx<'_>,
    policy: &super::policy::CostPolicy,
    tx_idx: crate::TxIdx,
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
    let n_pairs = policy.pairs_of(f.location).len();
    if policy.hops_to_admit(f.location, n_pairs) > 0 {
        crate::specfence::admit::queue_nearest_unfinished_successor(
            specfence.ready_edges,
            policy,
            specfence.hints,
            f.location,
            tx_idx,
            specfence.hints.from_of(tx_idx),
            specfence.hints.to_of(tx_idx),
        );
    }
}

fn abort_and_estimate(ctx: &ApplyCtx<'_>) {
    let tx = ctx.tx_version.tx_idx;
    let aborted = ctx.scheduler.try_validation_abort(ctx.tx_version);
    if aborted {
        ctx.mv_memory.convert_writes_to_estimates(tx);
        ctx.specfence.metrics.record_occ_abort();
        ctx.specfence.metrics.record_full_abort_reexecute();
    }
    if !ctx.invalid.is_empty() {
        ctx.specfence
            .metrics
            .record_region_validate_fail(ctx.invalid.len());
    }
    clear_retry(ctx.specfence, tx);
    for &location in ctx.invalid {
        ctx.specfence.metrics.record_bayes_conflict();
        ctx.specfence.hotset.note_abort(location);
        let loc_k = ctx
            .specfence
            .access_log
            .first_k(tx, location)
            .or_else(|| {
                ctx.specfence
                    .partial_retry
                    .first_k(tx, location)
                    .map(|k| k as u32)
            })
            .or_else(|| ctx.specfence.edges.min_k_of_location(tx, location))
            .filter(|&k| k > 0);
        if !location_is_lazy(ctx.mv_memory, tx, location) {
            crate::specfence::feeder::observe_abort(
                ctx.specfence.learner,
                ctx.specfence.bayes,
                location,
                ctx.invalid.len().max(1),
                loc_k,
            );
        }
        if let Some(k) = loc_k {
            ctx.specfence.sketch.mark_access_class(location, k);
        }
    }
}

fn clear_retry(specfence: SpecFenceCtx<'_>, tx: crate::TxIdx) {
    specfence.partial_retry.clear_force_ordered_admit(tx);
    specfence.partial_retry.clear_force_writers(tx);
    specfence.partial_retry.clear_repair(tx);
    specfence.partial_retry.clear_ff_head(tx);
    specfence.partial_retry.clear_suffix_repair_depth(tx);
}

fn release_successors(ctx: &ApplyCtx<'_>, producer: crate::TxIdx) {
    ctx.specfence
        .ready_edges
        .note_producer_done_stamp(producer);
    if ctx.specfence.ready_edges.has_known_waiters(producer) {
        ctx.specfence
            .ready_edges
            .note_producer_done(producer, ctx.specfence.wave);
    }
    ctx.specfence.producer_stages.note_done(producer);
    drain_wave_to_runnable(ctx);
}

fn drain_wave_to_runnable(ctx: &ApplyCtx<'_>) {
    while let Some(t) = ctx.specfence.wave.pop_ready() {
        if ctx.scheduler.is_validated(t) {
            ctx.runnable.mark_done(t);
            continue;
        }
        if ctx.specfence.ready_edges.was_queued(t) {
            ctx.runnable.push(t, QueueKind::Ordered);
        } else if ctx.specfence.ready_edges.is_gated(t) {
            ctx.runnable.push(t, QueueKind::Released);
        } else {
            ctx.runnable.push(t, QueueKind::Indep);
        }
    }
}

fn requeue(ctx: &ApplyCtx<'_>, tx: crate::TxIdx, kind: QueueKind) {
    ctx.runnable.force_push(tx, kind);
}

fn enqueue_revalidate(ctx: &ApplyCtx<'_>, reader: crate::TxIdx) {
    if reader >= ctx.scheduler.block_size() {
        return;
    }
    if !ctx.scheduler.is_executed(reader) && !ctx.scheduler.is_validated(reader) {
        return;
    }
    // Demote before the worker loop samples all_validated, otherwise the
    // last Commit exits every core and the revalidate never runs.
    let _ = ctx.scheduler.prepare_revalidate(reader);
    ctx.runnable.force_push(reader, QueueKind::Revalidate);
}

fn enqueue_higher_revalidate(ctx: &ApplyCtx<'_>, tx: crate::TxIdx, new_location: bool) {
    let writes = ctx.mv_memory.write_locations(tx);
    for loc in writes {
        for reader in ctx.mv_memory.higher_readers_of(loc, tx) {
            if reader > tx {
                enqueue_revalidate(ctx, reader);
            }
        }
    }
    // New locations may have been read from storage before this writer
    // existed — readers index can miss them until the next incarnation.
    if new_location {
        for reader in (tx + 1)..ctx.scheduler.block_size() {
            enqueue_revalidate(ctx, reader);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_kinds() {
        assert!(ResolvePlan::Commit.commits());
        assert!(ResolvePlan::PartialAbortRebind.commits());
        assert!(ResolvePlan::PartialAbortRebind.is_partial());
        assert!(!ResolvePlan::FullReplay.commits());
        assert!(!ResolvePlan::OrderedReplay.is_partial());
    }

    #[test]
    fn apply_source_never_calls_occ_pick_or_occ_stage() {
        let src = include_str!("resolve_plan.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("next_task_with_wave_ready("));
        assert!(!code.contains("validate_occ_stage("));
        assert!(!code.contains("validate_occ_kernel("));
        assert!(code.contains("finish_validation_sf"));
        assert!(code.contains("release_successors"));
    }
}
