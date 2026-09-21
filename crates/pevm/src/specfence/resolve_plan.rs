//! ResolvePlan — validate produces a structured plan (SF-PS §C / T3).
//!
//! `apply` changes certificates, RunnableSet queues, and write visibility.
//! Edged paths never fall through to `validate_occ_kernel` as the default.

use crate::mv_memory::MvMemory;
use crate::scheduler::Scheduler;
use crate::{MemoryLocationHash, TxVersion};

use super::arm_table::ArmTable;
use super::collateral::{classify_first_conflict, location_is_lazy, ConflictClass};
use super::runnable_set::{QueueKind, RunnableSet};
use super::SpecFenceCtx;
use super::VisibilityPolicy;

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
            enqueue_higher_revalidate(&ctx, tx);
            ctx.runnable.mark_done(tx);
        }
        ResolvePlan::PartialAbortRewind => {
            ctx.specfence.metrics.record_partial_abort_attempt();
            if ctx.scheduler.try_validation_abort(ctx.tx_version) {
                let write_locations = ctx.mv_memory.write_locations(tx);
                let estimated = ctx
                    .mv_memory
                    .invalidate_partial_suffix(tx, &write_locations);
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
            enqueue_higher_revalidate(&ctx, tx);
        }
        ResolvePlan::OrderedReplay => {
            abort_and_estimate(&ctx);
            if let Some(f) = first
                && f.class == ConflictClass::EffectiveWAW
                && let Some(p) = ctx.specfence.policy
            {
                seed_short_edge(ctx.specfence, p, tx, &f);
            }
            if let Some(f) = first {
                plant_observed_waw(&ctx, &f);
                break_replay_mill(&ctx, &f);
            }
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, true, Some(tx + 1));
            ctx.specfence.ready_edges.clear_started(tx);
            if ctx.specfence.ready_edges.is_gated(tx) && !ctx.specfence.ready_edges.may_execute(tx)
            {
                ctx.runnable.mark_wait(tx);
            } else if ctx.specfence.ready_edges.is_gated(tx) || ctx.vis.needs_fence() {
                requeue(&ctx, tx, QueueKind::Released);
            } else {
                requeue(&ctx, tx, QueueKind::Indep);
            }
            enqueue_higher_revalidate(&ctx, tx);
        }
        ResolvePlan::FullReplay => {
            abort_and_estimate(&ctx);
            if let Some(f) = first {
                match f.class {
                    ConflictClass::EffectiveWAW => {
                        if let Some(p) = ctx.specfence.policy {
                            seed_short_edge(ctx.specfence, p, tx, &f);
                        }
                        plant_observed_waw(&ctx, &f);
                        break_replay_mill(&ctx, &f);
                    }
                    ConflictClass::LazyNoise | ConflictClass::CommuteCandidate => {
                        if let Some(p) = ctx.specfence.policy {
                            p.ignore_conflict(Some(f.location));
                        }
                        break_replay_mill(&ctx, &f);
                    }
                }
            }
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, true, Some(tx + 1));
            ctx.specfence.ready_edges.clear_started(tx);
            enqueue_higher_revalidate(&ctx, tx);
            // Observed WAW: wait for the producer instead of Opt ping-pong.
            if ctx.specfence.ready_edges.is_gated(tx) && !ctx.specfence.ready_edges.may_execute(tx)
            {
                ctx.runnable.mark_wait(tx);
            } else {
                // Never Q_ordered from FullReplay: 19807137 milled ~43
                // location-admitted OrderedTip heads (c5b6ce3/7e2dd73).
                // Heal/drain may still Ordered a location cohort.
                let kind = if ctx.specfence.ready_edges.is_gated(tx) || ctx.vis.needs_fence() {
                    QueueKind::Released
                } else {
                    QueueKind::Indep
                };
                requeue(&ctx, tx, kind);
            }
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
    if lazy && let Some(l) = loc {
        ctx.arms
            .demote_lazy_graph(l, ctx.specfence.ready_edges, ctx.runnable);
    }
}

/// Raise a Detect edge on an observed non-lazy WAW. Thin `hops=0` must
/// not leave two Opt writers ping-ponging FullReplay forever — but a
/// deep / fat plant serializes ERC-20 and 19469101. Window = wait only
/// on a runnable producer, and at most `w_max` waiters per ℓ.
fn plant_observed_waw(ctx: &ApplyCtx<'_>, f: &super::collateral::FirstConflict) {
    if f.lazy || f.class != ConflictClass::EffectiveWAW {
        return;
    }
    let tx = ctx.tx_version.tx_idx;
    let Some(producer) = f.peer.filter(|&w| w < tx) else {
        return;
    };
    if !ctx
        .specfence
        .ready_edges
        .should_plant_observed_waw(tx, producer)
    {
        return;
    }
    let w_max = ArmTable::w_max(ctx.scheduler.block_size(), 0, false) as usize;
    let _ = ctx
        .specfence
        .ready_edges
        .plant_observed_window(tx, producer, f.location, w_max);
}

/// Second+ incarnation FullReplay that is not EffectiveWAW still Opt-mills
/// (19469101 ~390%). After one retry, raise an anonymous wait on the peer
/// even if Learn classified LazyNoise / Commute.
fn break_replay_mill(ctx: &ApplyCtx<'_>, f: &super::collateral::FirstConflict) {
    if ctx.tx_version.tx_incarnation < 1 {
        return;
    }
    let tx = ctx.tx_version.tx_idx;
    let producer = f
        .peer
        .or_else(|| ctx.mv_memory.last_writer_before(f.location, tx))
        .filter(|&w| w < tx);
    let Some(producer) = producer else {
        return;
    };
    if ctx.specfence.ready_edges.is_writer_done(producer) {
        return;
    }
    if ctx
        .specfence
        .ready_edges
        .blocking_producer(tx)
        .is_some_and(|w| w >= producer)
    {
        return;
    }
    ctx.specfence
        .ready_edges
        .note_consumer_on(tx, producer, None);
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
        .note_producer_done(producer, ctx.specfence.wave);
    ctx.specfence.producer_stages.note_done(producer);
    drain_wave_to_runnable(ctx);
    // IntraPatch at the release boundary (same rule as pick): ≤1 / ℓ / block.
    let _ = ctx.arms.apply_pending_patches(
        ctx.runnable,
        ctx.specfence.ready_edges,
        ctx.specfence.policy,
        ctx.runnable.cores(),
        ctx.scheduler.block_size(),
    );
}

fn drain_wave_to_runnable(ctx: &ApplyCtx<'_>) {
    while let Some(t) = ctx.specfence.wave.pop_ready() {
        if ctx.scheduler.is_validated(t) {
            ctx.runnable.mark_done(t);
            continue;
        }
        if ctx.specfence.ready_edges.is_gated(t) && !ctx.specfence.ready_edges.may_execute(t) {
            ctx.runnable.mark_wait(t);
            continue;
        }
        let kind = if ctx.specfence.ready_edges.is_gated(t) {
            QueueKind::Released
        } else {
            QueueKind::Indep
        };
        ctx.runnable.force_push(t, kind);
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
    // Skip readers whose read set still matches — avoids an O(n²) beneficiary
    // revalidate mill on independent raw transfers.
    if super::occ_read_set_valid(ctx.mv_memory, reader) {
        return;
    }
    // Demote before the worker loop samples all_validated, otherwise the
    // last Commit exits every core and the revalidate never runs.
    let _ = ctx.scheduler.prepare_revalidate(reader);
    ctx.runnable.force_push(reader, QueueKind::Revalidate);
}

fn enqueue_higher_revalidate(ctx: &ApplyCtx<'_>, tx: crate::TxIdx) {
    let writes = ctx.mv_memory.write_locations(tx);
    for loc in writes {
        for reader in ctx.mv_memory.higher_readers_of(loc, tx) {
            if reader > tx {
                enqueue_revalidate(ctx, reader);
            }
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
        assert!(
            code.contains("should_plant_observed_waw"),
            "mid-block WAW plant must stay windowed (no deep/fat spine)"
        );
        assert!(
            code.contains("break_replay_mill"),
            "incarnation≥1 FullReplay must not Opt-mill"
        );
    }
}
