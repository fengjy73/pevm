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
                // Early-k WAW Soft=0: RewindTo + ff_head, Indep requeue.
                // needs_live_capture + Released was the 19807137 hang class.
                let early_ungated =
                    ctx.invalid.len() == 1 && !ctx.specfence.ready_edges.is_gated(tx);
                if early_ungated {
                    // try_early_waw_rewind already installed ff_head + RewindTo.
                    if !ctx.specfence.partial_retry.has_ff_head(tx) {
                        let _ = keep_single_invalid_prefix(&ctx);
                    }
                } else {
                    ctx.specfence.partial_retry.mark_needs_live_capture(tx);
                }
            }
            // Partial rewind drops the publish. Leaving the edge done-bit set
            // made `note_consumer_on` bail (`is_writer_done`) while
            // `add_dependency` still parked, and heal incarnation-milled
            // (19807137 root_inc tens of thousands on a Ready passed writer).
            ctx.specfence.ready_edges.note_abort_reincarnate(tx);
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, true, Some(tx + 1));
            ctx.specfence.ready_edges.clear_started(tx);
            let early_ungated = ctx.invalid.len() == 1 && !ctx.specfence.ready_edges.is_gated(tx);
            if early_ungated {
                requeue(&ctx, tx, QueueKind::Indep);
            } else {
                requeue(&ctx, tx, QueueKind::Released);
            }
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
            plant_invalid_locs(&ctx);
            // Per-ℓ leftover can still leave many Released tips (19807137
            // ~40-head mill). Serialize only writers that may_execute after
            // the loc plant — do not drain-rebind first-wave onto leftover_min.
            if ctx.specfence.ready_edges.may_execute(tx) {
                let _ = ctx.specfence.ready_edges.plant_global_leftover(tx);
            }
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, true, Some(tx + 1));
            ctx.specfence.ready_edges.clear_started(tx);
            if ctx.specfence.ready_edges.leftover_surplus(tx)
                || (ctx.specfence.ready_edges.is_gated(tx)
                    && !ctx.specfence.ready_edges.may_execute(tx))
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
            plant_invalid_locs(&ctx);
            if ctx.specfence.ready_edges.may_execute(tx) {
                let _ = ctx.specfence.ready_edges.plant_global_leftover(tx);
            }
            ctx.scheduler
                .finish_validation_sf(ctx.tx_version, true, Some(tx + 1));
            ctx.specfence.ready_edges.clear_started(tx);
            // abort_and_estimate cleared ff_head. Install prefix snaps now,
            // before the next incarnation's reset, so reads with k < fail_k
            // are served from ff_head. The failed location is not in the set.
            // A miss is a true restart from k=0.
            if !keep_single_invalid_prefix(&ctx) {
                ctx.specfence.metrics.record_full_from_zero();
            }
            enqueue_higher_revalidate(&ctx, tx);
            // Observed WAW: wait for the producer instead of Opt ping-pong.
            if ctx.specfence.ready_edges.leftover_surplus(tx)
                || (ctx.specfence.ready_edges.is_gated(tx)
                    && !ctx.specfence.ready_edges.may_execute(tx))
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
    // E6: only ungate *lazy* ℓ. Demoting the first-conflict loc whenever
    // any invalid was lazy tore down observed-WAW plants (19807137 mill).
    if lazy {
        if first.is_some_and(|f| f.lazy) {
            ctx.arms
                .demote_lazy_graph(loc.unwrap(), ctx.specfence.ready_edges, ctx.runnable);
        }
        for &l in ctx.invalid {
            if first.is_some_and(|f| f.location == l && f.lazy) {
                continue;
            }
            if location_is_lazy(ctx.mv_memory, tx, l) {
                ctx.arms
                    .demote_lazy_graph(l, ctx.specfence.ready_edges, ctx.runnable);
            }
        }
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
    // Width 1: ArmTable::w_max is admit_seed cover. A window of 4 on one
    // register is the 19807137 Released mill. Do not skip when `producer`
    // is already done — leftover writers must elect/chain.
    let _ = ctx
        .specfence
        .ready_edges
        .plant_observed_window(tx, producer, f.location, 1);
}

/// Second+ incarnation FullReplay that is not EffectiveWAW still Opt-mills
/// (19469101 ~390%). After one retry, raise an anonymous wait on the peer
/// even if Learn classified LazyNoise / Commute.
/// Plant every non-lazy invalid ℓ, not only FirstConflict. Shared storage
/// slots that are not the first fail still Opt-mill (19807137 ~30 heads).
fn plant_invalid_locs(ctx: &ApplyCtx<'_>) {
    let tx = ctx.tx_version.tx_idx;
    for &loc in ctx.invalid {
        if location_is_lazy(ctx.mv_memory, tx, loc) {
            continue;
        }
        let producer = ctx
            .mv_memory
            .last_writer_before(loc, tx)
            .filter(|&w| w < tx)
            .unwrap_or(tx);
        let _ = ctx
            .specfence
            .ready_edges
            .plant_observed_window(tx, producer, loc, 1);
    }
}

fn break_replay_mill(ctx: &ApplyCtx<'_>, f: &super::collateral::FirstConflict) {
    if ctx.tx_version.tx_incarnation < 1 {
        return;
    }
    let tx = ctx.tx_version.tx_idx;
    let producer = f
        .peer
        .or_else(|| ctx.mv_memory.last_writer_before(f.location, tx))
        .filter(|&w| w < tx)
        .unwrap_or(tx);
    let _ = ctx
        .specfence
        .ready_edges
        .plant_observed_window(tx, producer, f.location, 1);
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
    // Clear a prior Commit done-stamp. Leaving it set made leftover_min
    // sticky-done (19807137 glob_min=396 min_done=true, 24-head mill).
    // Bits only — do not walk waiters (DashMap abort).
    ctx.specfence.ready_edges.note_abort_reincarnate(tx);
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

/// n_invalid = 1 at a known k: keep snaps for k' < fail_k. Not Ordered-from-0.
/// Returns whether a prefix was installed.
fn keep_single_invalid_prefix(ctx: &ApplyCtx<'_>) -> bool {
    if ctx.invalid.len() != 1 {
        return false;
    }
    let tx = ctx.tx_version.tx_idx;
    let loc = ctx.invalid[0];
    if ctx.specfence.access_arms.is_never(loc) || location_is_lazy(ctx.mv_memory, tx, loc) {
        return false;
    }
    let Some(k) = ctx.specfence.access_log.first_k(tx, loc).filter(|k| *k > 0) else {
        return false;
    };
    let peer = ctx
        .mv_memory
        .last_writer_before(loc, tx)
        .filter(|&w| w < tx)
        .or_else(|| {
            ctx.specfence
                .ready_edges
                .writers_of(loc)
                .into_iter()
                .rev()
                .find(|&w| w < tx)
        })
        .unwrap_or(0);
    ctx.specfence
        .access_arms
        .note_early_waw_peer(loc, k, peer);
    // Thin: ungated publish-order Avoid — no mark_gated.
    if peer > 0 && ctx.scheduler.block_size() <= super::THIN_SHELL_N {
        ctx.specfence.ready_edges.note_ungated_wait_on(tx, peer);
    }
    let prefix = ctx.specfence.access_log.prefix_before(tx, k);
    let mut n = ctx
        .specfence
        .partial_retry
        .arm_prefix_keep(tx, loc, k, &prefix);
    // Ungated early reads sometimes leave access_log prefix without rem
    // snaps (code_hash / None basic). Keep every snap except the fail loc.
    if n == 0 {
        n = ctx
            .specfence
            .partial_retry
            .arm_prefix_keep_all_except(tx, loc);
    }
    if n > 0 {
        ctx.specfence.access_arms.note_prefix_resume(n);
        ctx.specfence.metrics.record_prefix_resume(n);
        ctx.specfence.metrics.record_fail_k(k);
        true
    } else {
        false
    }
}

/// Opt early-WAW: mid-tx checkpoint before fail_k → hang-free RewindTo.
pub(crate) fn try_early_waw_rewind(
    mv_memory: &MvMemory,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
    invalid: &[MemoryLocationHash],
) -> Option<ResolvePlan> {
    if invalid.len() != 1 {
        return None;
    }
    // Thin shell: RewindTo tax exceeds the FullReplay it removes (3356896).
    if specfence.scheduler.block_size() <= super::THIN_SHELL_N {
        return None;
    }
    let tx = tx_version.tx_idx;
    let loc = invalid[0];
    if specfence.access_arms.is_never(loc) || location_is_lazy(mv_memory, tx, loc) {
        return None;
    }
    // Already RewindTo once this block — escalate to FullReplay. Prefix
    // re-snap on FF basics still cuts full_from_0 on that escalate.
    if specfence.partial_retry.suffix_repair_depth(tx) != 0 {
        return None;
    }
    let k = specfence.access_log.first_k(tx, loc).filter(|&k| k > 1)?;
    let k_fail = k as usize;
    let cp = specfence.partial_retry.last_checkpoint_before(tx, k_fail)?;
    if cp.k == 0 || cp.k >= k_fail {
        return None;
    }
    let prefix = specfence.access_log.prefix_before(tx, k);
    let mut n = specfence.partial_retry.arm_prefix_keep(tx, loc, k, &prefix);
    if n == 0 {
        n = specfence.partial_retry.arm_prefix_keep_all_except(tx, loc);
    }
    if n == 0 {
        return None;
    }
    let certified: Vec<_> = prefix.iter().map(|(l, _)| *l).collect();
    specfence
        .partial_retry
        .arm_rewind_to(tx, cp, k_fail, certified, Vec::new(), Vec::new());
    specfence.partial_retry.note_suffix_repair(tx);
    specfence.access_arms.note_early_waw(loc, k);
    specfence.access_arms.note_prefix_resume(n);
    specfence.metrics.record_prefix_resume(n);
    specfence.metrics.record_fail_k(k);
    Some(ResolvePlan::PartialAbortRewind)
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
    let mut still_running = Vec::new();
    while let Some(t) = ctx.specfence.wave.pop_ready() {
        if ctx.scheduler.is_validated(t) {
            ctx.runnable.mark_done(t);
            continue;
        }
        // Same rule as the worker drain: do not steal `ST_RUNNING`.
        if ctx.runnable.is_running(t) {
            still_running.push(t);
            continue;
        }
        if ctx.specfence.ready_edges.is_gated(t) && !ctx.specfence.ready_edges.may_execute(t) {
            ctx.runnable.note_wait_unless_running(t);
            continue;
        }
        let kind = if ctx.specfence.ready_edges.is_gated(t) {
            QueueKind::Released
        } else {
            QueueKind::Indep
        };
        if !ctx.runnable.wake_idle(t, kind) && ctx.runnable.is_running(t) {
            still_running.push(t);
        }
    }
    for t in still_running {
        ctx.specfence.wave.push_ready(t);
    }
}

fn requeue(ctx: &ApplyCtx<'_>, tx: crate::TxIdx, kind: QueueKind) {
    // Owner holds ST_RUNNING from the pick. CAS it onto the queue.
    // force_push would also overwrite a claim another worker already took.
    if !ctx.runnable.release_owner(tx, kind) {
        let _ = ctx.runnable.wake_idle(tx, kind);
    }
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
    let _ = ctx.runnable.wake_idle(reader, QueueKind::Revalidate);
}

fn enqueue_higher_revalidate(ctx: &ApplyCtx<'_>, tx: crate::TxIdx) {
    let writes = ctx.mv_memory.write_locations(tx);
    // Lazy beneficiary/sender writes still invalidate higher readers. Skipping
    // that fan-out on the thin shell (n≤176) committed a different account
    // balance than sequential on 3356896 while receipts matched.
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
            code.contains("plant_observed_window"),
            "mid-block WAW plant must stay windowed (no deep/fat spine)"
        );
        assert!(
            code.contains("break_replay_mill"),
            "incarnation≥1 FullReplay must not Opt-mill"
        );
    }
}
