//! SpecFence worker ring: RunnableSet.pick → Execute(vis) → Resolve.apply.
//!
//! This is the product loop. It must not call `Scheduler::next_task*` or
//! the OCC-stage validate entry. OCC contrast stays in `pevm.rs`.

use std::sync::atomic::Ordering;
use std::time::Instant;

use super::arm_table::ArmTable;
use super::resolve_plan::{self, ApplyCtx};
use super::runnable_set::RunnableSet;
use super::schedule;
use super::SpecFenceCtx;
use super::VisibilityPolicy;
use crate::mv_memory::MvMemory;
use crate::scheduler::Scheduler;
use crate::{Task, TxVersion};

/// Drive one worker until the block is committed or aborted.
pub(crate) fn run_sf_block<F, V>(
    scheduler: &Scheduler,
    mv_memory: &MvMemory,
    specfence: SpecFenceCtx<'_>,
    runnable: &RunnableSet,
    arms: &ArmTable,
    worker_i: usize,
    abort: impl Fn() -> bool,
    mut execute: F,
    mut validate_to_plan: V,
) where
    F: FnMut(TxVersion, VisibilityPolicy) -> SfExec,
    V: FnMut(&TxVersion, VisibilityPolicy) -> (super::ResolvePlan, Vec<crate::MemoryLocationHash>),
{
    let metrics = specfence.metrics;
    runnable.bind_worker(worker_i);
    super::busy_stall::bind(worker_i);
    struct ProbeExit;
    impl Drop for ProbeExit {
        fn drop(&mut self) {
            super::busy_stall::worker_exit();
        }
    }
    let _probe_exit = ProbeExit;
    // Unpark peers on every exit so a parked worker cannot outlive the scope.
    struct ExitWake<'a>(&'a RunnableSet);
    impl Drop for ExitWake<'_> {
        fn drop(&mut self) {
            self.0.wake_all_teardown();
        }
    }
    let _exit_wake = ExitWake(runnable);
    // Records the whole idle arm, including early `break` / `continue`.
    struct IdleAccum<'a> {
        set: &'a RunnableSet,
        t0: Instant,
    }
    impl Drop for IdleAccum<'_> {
        fn drop(&mut self) {
            self.set.add_idle_ns(self.t0.elapsed().as_nanos() as u64);
        }
    }
    let mut spins = 0u64;
    loop {
        spins += 1;
        if spins.is_multiple_of(8_000) && std::env::var_os("SPECFENCE_HANG_TRACE").is_some() {
            let (n_min, n_tip, n_ov) = specfence.ready_edges.hang_plant_n();
            let (g_min, g_chain) = specfence.ready_edges.hang_global_leftover();
            let (min_done, min_run, min_pred) = specfence.ready_edges.hang_leftover_min_status();
            let (pred_passed, pred_surplus, pred_claim, pred_done, pred_gated, pred_may) =
                specfence.ready_edges.hang_pred_flags(min_pred);
            let pred_st = if min_pred == usize::MAX {
                "-"
            } else if scheduler.is_validated(min_pred) {
                "val"
            } else if scheduler.is_executed(min_pred) {
                "exed"
            } else if scheduler.is_executing(min_pred) {
                "exec"
            } else if scheduler.is_aborting(min_pred) {
                "abt"
            } else if scheduler.is_ready(min_pred) {
                "rdy"
            } else {
                "?"
            };
            let n_unf = (0..scheduler.block_size())
                .filter(|&t| !scheduler.is_validated(t))
                .count();
            let chain = specfence.ready_edges.hang_block_chain(g_min);
            let depth = chain.len();
            let root = chain.last().map(|(tx, _)| *tx).unwrap_or(usize::MAX);
            let (root_inc, root_dep, root_wfd) = if root == usize::MAX {
                (0, usize::MAX, usize::MAX)
            } else {
                scheduler.hang_dep_of(root)
            };
            let dep_st = if root_dep == usize::MAX {
                "-".to_string()
            } else {
                let (p, s, c, d, g, m) = specfence.ready_edges.hang_pred_flags(root_dep);
                let (dep_inc, _, _) = scheduler.hang_dep_of(root_dep);
                format!(
                    "{root_dep}:{}/{dep_inc}/rst{} p/s/c/d/g/m={}/{}/{}/{}/{}/{}",
                    hang_sched(scheduler, root_dep),
                    runnable.hang_state(root_dep),
                    u8::from(p),
                    u8::from(s),
                    u8::from(c),
                    u8::from(d),
                    u8::from(g),
                    u8::from(m),
                )
            };
            let passed_hop = chain
                .iter()
                .find_map(|(tx, _)| specfence.ready_edges.leftover_passed(*tx).then_some(*tx));
            let surplus_hop = chain
                .iter()
                .find_map(|(tx, _)| specfence.ready_edges.leftover_surplus(*tx).then_some(*tx));
            let chain_s: String = chain
                .iter()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|(tx, pred)| {
                    let (p, s, c, d, g, m) = specfence.ready_edges.hang_pred_flags(*tx);
                    format!(
                        "{tx}->{} st={} rst={} p/s/c/d/g/m={}/{}/{}/{}/{}/{}",
                        if *pred == usize::MAX {
                            "-".to_string()
                        } else {
                            pred.to_string()
                        },
                        hang_sched(scheduler, *tx),
                        runnable.hang_state(*tx),
                        u8::from(p),
                        u8::from(s),
                        u8::from(c),
                        u8::from(d),
                        u8::from(g),
                        u8::from(m),
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let min_st = if g_min == usize::MAX {
                "none"
            } else if scheduler.is_validated(g_min) {
                "val"
            } else if scheduler.is_executed(g_min) {
                "exed"
            } else if scheduler.is_executing(g_min) {
                "exec"
            } else if scheduler.is_aborting(g_min) {
                "abt"
            } else if scheduler.is_ready(g_min) {
                "rdy"
            } else {
                "?"
            };
            eprintln!(
                "sf-hang-trace spins={spins} pending={} q_i/r/o/v={}/{}/{}/{} unfinished={} validated={} gated={} live_wait={} refuse={} leftover_min={} loc_tip={} overflow_tip={} glob_min={} glob_chain={} min_done={} min_exec={} min_pred={} min_st={} pred_passed={} pred_surp={} pred_claim={} pred_done={} pred_gated={} pred_may={} pred_st={} n_unf={} n_run={} depth={depth} root_inc={root_inc} root_dep={root_dep} dep=[{dep_st}] root_wfd={root_wfd} passed_hop={passed_hop:?} surplus_hop={surplus_hop:?} tail=[{chain_s}]",
                runnable.pending_work(),
                runnable.q_indep_len(),
                runnable.q_released_len(),
                runnable.q_ordered_len(),
                runnable.q_revalidate_len(),
                scheduler.has_unfinished(),
                scheduler.all_validated(),
                specfence.ready_edges.has_any_gated(),
                runnable.waiting_on_live_producer(specfence.ready_edges, scheduler),
                runnable.refuse_fill_n(),
                n_min,
                n_tip,
                n_ov,
                g_min,
                g_chain,
                min_done,
                min_run,
                min_pred,
                min_st,
                pred_passed,
                pred_surplus,
                pred_claim,
                pred_done,
                pred_gated,
                pred_may,
                pred_st,
                n_unf,
                runnable.running_n(),
            );
        }
        if abort() {
            mark_exit(specfence, runnable);
            break;
        }
        // Fast complete path: the tally only. A short counter must not scan
        // every tx before pick; QuietExit on the idle arm reads the flags.
        if scheduler.validated_tally_reached()
            && runnable.pending_work() == 0
            && specfence.wave.ready_depth() == 0
            && specfence.spine.handoff_is_empty()
        {
            mark_exit(specfence, runnable);
            break;
        }
        let t0 = Instant::now();
        let task = schedule::pick(
            scheduler,
            specfence.wave,
            specfence.ready_edges,
            specfence.producer_stages,
            runnable,
            arms,
            specfence.policy,
            Some(metrics),
            worker_i,
            specfence.spine,
        );
        match task {
            Some(Task::Execution(tx_version)) => {
                let tx_idx = tx_version.tx_idx;
                let mut first_ns = 0u64;
                if let Some(slot) = specfence.tx_first_start.get(tx_idx) {
                    if slot.load(Ordering::Relaxed) == 0 {
                        let ns = origin_ns(specfence).max(1);
                        if slot
                            .compare_exchange(0, ns, Ordering::Relaxed, Ordering::Relaxed)
                            .is_ok()
                        {
                            first_ns = ns;
                            if tx_idx == runnable.span_tail() {
                                publish_span_end(scheduler, specfence, runnable, ns);
                            }
                        }
                    }
                }
                if specfence.ideal_prox.enabled() {
                    let ns = if first_ns != 0 {
                        first_ns
                    } else {
                        origin_ns(specfence).max(1)
                    };
                    let ordered = specfence.spine.is_ordered_member(tx_idx);
                    let has_pred = specfence.ready_edges.recorded_pred(tx_idx).is_some();
                    let (blocker, secondary, role, preds) = super::ideal_prox::classify_enter(
                        ordered,
                        has_pred,
                        runnable.width_hint(),
                        runnable.cores(),
                    );
                    let wave = if ordered {
                        specfence
                            .spine
                            .ordered_pos(tx_idx)
                            .unwrap_or(0)
                            .min(u16::MAX as usize) as u16
                    } else {
                        0
                    };
                    specfence
                        .ideal_prox
                        .note_enter(tx_idx, ns, blocker, secondary, role, preds, wave);
                }
                specfence.ready_edges.note_started(tx_idx);
                // leftover_min must skip Estimate tips so nonce/fund
                // Blocking(tx-1) on a done writer cannot ghost-Executing mill
                // (19807137 leftover_min=514 n_unf=198). Not OCC pick.
                let vis = if specfence.ready_edges.is_live_leftover_min(tx_idx) {
                    VisibilityPolicy::WaitReleased
                } else {
                    VisibilityPolicy::for_ready(specfence.ready_edges, tx_idx)
                };
                let exec_start = origin_ns(specfence);
                match execute(tx_version.clone(), vis) {
                    SfExec::Executed { wrote_new_location } => {
                        let exec_end = origin_ns(specfence);
                        specfence.ideal_prox.note_finish(tx_idx, exec_end);
                        note_exec_span(runnable, exec_start, exec_end);
                        // Next hop may start on another core while we validate.
                        // Ownership ends with the hop, not with validation.
                        specfence.spine.release_owner_tx(tx_idx);
                        // finish_execution may have waved dependents — park them
                        // on RunnableSet before this worker spends time validating.
                        // Probe: drain + validate + resolve. Execute itself is
                        // outside this window. Classified by window start.
                        let outside = post_exec_start_outside_span(specfence, runnable);
                        let phase_t0 = Instant::now();
                        let val_start = origin_ns(specfence);
                        drain_wave(specfence, scheduler, runnable);
                        let phase_v0 = Instant::now();
                        if super::busy_stall::enabled() {
                            super::busy_stall::charge(
                                super::busy_stall::KIND_SCHED,
                                phase_v0.saturating_duration_since(phase_t0).as_nanos() as u64,
                                phase_t0,
                            );
                        }
                        let resolve_t0 = Instant::now();
                        apply_resolving(
                            &tx_version,
                            vis,
                            wrote_new_location,
                            specfence,
                            mv_memory,
                            scheduler,
                            runnable,
                            arms,
                            &mut validate_to_plan,
                        );
                        if super::busy_stall::enabled() {
                            let ns = resolve_t0.elapsed().as_nanos() as u64;
                            super::busy_stall::charge_span(
                                super::busy_stall::KIND_VALIDATE,
                                ns,
                                resolve_t0,
                                tx_idx as u32,
                                super::busy_stall::PRED_NONE,
                                tx_version.tx_incarnation as u16,
                                ns,
                            );
                        }
                        let val_ns = phase_v0.elapsed().as_nanos() as u64;
                        metrics.add_phase_val(val_ns);
                        crate::specfence::inflation::note_val(
                            tx_version.tx_idx,
                            tx_version.tx_incarnation as u16,
                            val_ns,
                        );
                        note_validate_span(runnable, val_start, origin_ns(specfence));
                        record_post_exec(runnable, outside, phase_t0.elapsed().as_nanos() as u64);
                    }
                    SfExec::Blocked { on } => {
                        note_exec_span(runnable, exec_start, origin_ns(specfence));
                        specfence.spine.release_owner_tx(tx_idx);
                        // add_dependency parks leave Aborting with no Detect
                        // edge. Heal then recovered them into a live antichain
                        // (incarnation++ mill — 6196166 reuse 206k / 19807137).
                        if let Some(w) = on {
                            if w < tx_idx {
                                // AccessArm WaitOnce on an ungated tx must not
                                // mark_gated — that left the fast Opt path.
                                if specfence.ready_edges.is_gated(tx_idx) {
                                    specfence.ready_edges.note_consumer_on(tx_idx, w, None);
                                } else {
                                    specfence.ready_edges.note_ungated_wait_on(tx_idx, w);
                                }
                                runnable.mark_wait(tx_idx);
                            } else if specfence.ready_edges.is_live_leftover_min(tx_idx)
                                || specfence.ready_edges.is_leftover_claimed(tx_idx)
                            {
                                // leftover_min must not stay parked on a later
                                // waiter. Flush Detect + detach only — recover
                                // leftover_min here raced 6196166 reuse heap.
                                specfence.ready_edges.flush_wait_on(w, tx_idx);
                                let _ = scheduler.detach_dependent(w, tx_idx);
                                runnable.mark_wait(tx_idx);
                            } else if specfence.ready_edges.detect_waits_on(w, tx_idx) {
                                // Later leftover is Detect-gated on us; we
                                // parked Aborting on them (6196166 reuse
                                // n_unf=44). Flush the leftover pred only —
                                // `ungate` + recover_executing double-freed.
                                specfence.ready_edges.flush_wait_on(w, tx_idx);
                                if !runnable.is_running(w)
                                    && specfence.ready_edges.may_execute(w)
                                    && (scheduler.is_ready(w) || scheduler.is_executed(w))
                                {
                                    let kind = if specfence.ready_edges.is_gated(w) {
                                        super::runnable_set::QueueKind::Released
                                    } else {
                                        super::runnable_set::QueueKind::Indep
                                    };
                                    let _ = runnable.wake_idle(w, kind);
                                }
                                runnable.mark_wait(tx_idx);
                            } else {
                                runnable.mark_wait(tx_idx);
                            }
                        } else {
                            runnable.mark_wait(tx_idx);
                        }
                        drain_wave(specfence, scheduler, runnable);
                    }
                    SfExec::Fatal => {
                        note_exec_span(runnable, exec_start, origin_ns(specfence));
                        specfence.spine.release_owner_tx(tx_idx);
                        mark_exit(specfence, runnable);
                        break;
                    }
                }
                metrics.add_worker_busy_ns(t0.elapsed().as_nanos() as u64);
            }
            Some(Task::Validation(tx_version)) => {
                let vis = VisibilityPolicy::for_ready(specfence.ready_edges, tx_version.tx_idx);
                // Revalidate is the same post-exec window, without the
                // execute-path drain that already ran on the owner.
                let outside = post_exec_start_outside_span(specfence, runnable);
                let phase_t0 = Instant::now();
                let val_start = origin_ns(specfence);
                let phase_v0 = Instant::now();
                apply_resolving(
                    &tx_version,
                    vis,
                    false,
                    specfence,
                    mv_memory,
                    scheduler,
                    runnable,
                    arms,
                    &mut validate_to_plan,
                );
                if super::busy_stall::enabled() {
                    let val_ns = phase_v0.elapsed().as_nanos() as u64;
                    super::busy_stall::charge_span(
                        super::busy_stall::KIND_VALIDATE,
                        val_ns,
                        phase_v0,
                        tx_version.tx_idx as u32,
                        super::busy_stall::PRED_NONE,
                        tx_version.tx_incarnation as u16,
                        val_ns,
                    );
                }
                let val_ns = phase_v0.elapsed().as_nanos() as u64;
                metrics.add_phase_val(val_ns);
                crate::specfence::inflation::note_val(
                    tx_version.tx_idx,
                    tx_version.tx_incarnation as u16,
                    val_ns,
                );
                note_validate_span(runnable, val_start, origin_ns(specfence));
                record_post_exec(runnable, outside, phase_t0.elapsed().as_nanos() as u64);
                metrics.add_worker_busy_ns(t0.elapsed().as_nanos() as u64);
            }
            None => {
                if runnable.span_end_ns() != 0 {
                    runnable.add_post_span_steal_ns(t0.elapsed().as_nanos() as u64);
                    runnable.inc_post_span_idle_n();
                }
                // Idle: `pick` already stole the antichain tail. Help release
                // is heal of writers that have published.
                // `idle_ns` is this whole arm. `heal_ns` is the heal cluster
                // only (not yield / spin / park). Both are worker sums.
                metrics.add_idle_core_ns(t0.elapsed().as_nanos() as u64);
                runnable.note_idle_spin();
                let _idle_accum = IdleAccum {
                    set: runnable,
                    t0: Instant::now(),
                };
                if abort() {
                    mark_exit(specfence, runnable);
                    break;
                }
                // BlockQuiet before another heal. `pick → None` already
                // missed one steal probe. Do not yield until the tally moves.
                if observe_quiet(scheduler, specfence, runnable) {
                    mark_exit(specfence, runnable);
                    break;
                }
                let heal_t0 = Instant::now();
                specfence
                    .ready_edges
                    .heal_finished_preds(|w| scheduler.is_done(w));
                let _ = specfence.ready_edges.wake_ready_sleepers(specfence.wave);
                let _ = runnable.heal(specfence.ready_edges, scheduler);
                drain_wave(specfence, scheduler, runnable);
                let heal_ns = heal_t0.elapsed().as_nanos() as u64;
                runnable.add_heal_ns(heal_ns);
                if super::busy_stall::enabled() {
                    super::busy_stall::charge(super::busy_stall::KIND_SCHED, heal_ns, heal_t0);
                }
                if runnable.span_end_ns() != 0 {
                    runnable.add_post_span_heal_ns(heal_ns);
                }
                if observe_quiet(scheduler, specfence, runnable) {
                    mark_exit(specfence, runnable);
                    break;
                }
                // Last-ditch: unfinished + empty queues + no live producer.
                // Heal already ran; recover ghosts, then QuietExit if the
                // schedule is silent. Do not spin on the validation tally.
                if runnable.pending_work() == 0
                    && !runnable.waiting_on_live_producer(specfence.ready_edges, scheduler)
                    && (scheduler.has_unfinished() || scheduler.has_done_unvalidated())
                {
                    let heal_t0 = Instant::now();
                    let _ = runnable.heal(specfence.ready_edges, scheduler);
                    drain_wave(specfence, scheduler, runnable);
                    if runnable.pending_work() == 0 && !runnable.any_running() {
                        let _ = runnable.force_idle_recover(specfence.ready_edges, scheduler);
                        drain_wave(specfence, scheduler, runnable);
                    }
                    let heal_ns = heal_t0.elapsed().as_nanos() as u64;
                    runnable.add_heal_ns(heal_ns);
                    if super::busy_stall::enabled() {
                        super::busy_stall::charge(super::busy_stall::KIND_SCHED, heal_ns, heal_t0);
                    }
                    if runnable.span_end_ns() != 0 {
                        runnable.add_post_span_heal_ns(heal_ns);
                    }
                    if runnable.pending_work() > 0 {
                        continue;
                    }
                }
                if observe_quiet(scheduler, specfence, runnable) {
                    mark_exit(specfence, runnable);
                    break;
                }
                if runnable.pending_work() > 0 {
                    continue;
                }
                // ExactWake only while a live producer still owns the core.
                // A short timeout here woke every idle worker together and
                // raced force-idle recover (large block seq!=par).
                if runnable.waiting_on_live_producer(specfence.ready_edges, scheduler) {
                    let spin_t0 = super::busy_stall::enabled().then(Instant::now);
                    for _ in 0..32 {
                        std::hint::spin_loop();
                    }
                    if let Some(t0) = spin_t0 {
                        super::busy_stall::charge(
                            super::busy_stall::KIND_SPIN,
                            t0.elapsed().as_nanos() as u64,
                            t0,
                        );
                    }
                    if runnable.pending_work() > 0 {
                        continue;
                    }
                    if observe_quiet(scheduler, specfence, runnable) {
                        mark_exit(specfence, runnable);
                        break;
                    }
                    let park_t0 = Instant::now();
                    runnable.park_idle(worker_i);
                    if runnable.span_end_ns() != 0 {
                        runnable.add_post_span_park_ns(park_t0.elapsed().as_nanos() as u64);
                    }
                    if super::busy_stall::enabled() {
                        let ns = park_t0.elapsed().as_nanos() as u64;
                        let kind = if runnable.span_end_ns() != 0 {
                            super::busy_stall::KIND_STALL_JOIN
                        } else {
                            super::busy_stall::KIND_STALL_WAITONCE
                        };
                        super::busy_stall::charge_span(
                            kind,
                            ns,
                            park_t0,
                            super::busy_stall::PRED_NONE,
                            super::busy_stall::PRED_NONE,
                            0,
                            ns,
                        );
                    }
                    continue;
                }
                // Still unfinished or a tip is unpublished. Back off so the
                // last producer keeps the core. QuietExit already returned.
                let yield_t0 = Instant::now();
                std::thread::yield_now();
                if runnable.span_end_ns() != 0 {
                    runnable.add_post_span_yield_ns(yield_t0.elapsed().as_nanos() as u64);
                }
                if super::busy_stall::enabled() {
                    let ns = yield_t0.elapsed().as_nanos() as u64;
                    let kind = if runnable.span_end_ns() != 0 {
                        super::busy_stall::KIND_STALL_JOIN
                    } else {
                        super::busy_stall::KIND_STALL_NOREADY
                    };
                    super::busy_stall::charge_span(
                        kind,
                        ns,
                        yield_t0,
                        super::busy_stall::PRED_NONE,
                        super::busy_stall::PRED_NONE,
                        0,
                        ns,
                    );
                }
            }
        }
    }
}

/// Validate and apply until a Commit is accepted or the plan aborts.
///
/// A Commit computed before a lower writer's publish is rejected via
/// [`Scheduler::try_commit_if_clean`]. Re-validate instead of sticking it.
fn apply_resolving(
    tx_version: &TxVersion,
    vis: VisibilityPolicy,
    wrote_new_location: bool,
    specfence: SpecFenceCtx<'_>,
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    runnable: &RunnableSet,
    arms: &ArmTable,
    validate_to_plan: &mut impl FnMut(
        &TxVersion,
        VisibilityPolicy,
    ) -> (super::ResolvePlan, Vec<crate::MemoryLocationHash>),
) {
    for _ in 0..8 {
        let (plan, invalid) = validate_to_plan(tx_version, vis);
        let retry = resolve_plan::apply(
            plan,
            ApplyCtx {
                specfence,
                mv_memory,
                scheduler,
                runnable,
                arms,
                tx_version,
                vis,
                wrote_new_location,
                invalid: &invalid,
            },
        );
        if !retry {
            return;
        }
    }
    let (plan, invalid) = validate_to_plan(tx_version, vis);
    let plan = if plan.commits() {
        super::ResolvePlan::FullReplay
    } else {
        plan
    };
    let _ = resolve_plan::apply(
        plan,
        ApplyCtx {
            specfence,
            mv_memory,
            scheduler,
            runnable,
            arms,
            tx_version,
            vis,
            wrote_new_location,
            invalid: &invalid,
        },
    );
}

fn origin_ns(specfence: SpecFenceCtx<'_>) -> u64 {
    specfence.exec_origin.elapsed().as_nanos() as u64
}

fn mark_exit(specfence: SpecFenceCtx<'_>, runnable: &RunnableSet) {
    runnable.note_worker_exit(origin_ns(specfence));
}

/// Nanoseconds of `[start, end)` that fall at or after `mark`.
/// `mark == 0` means the span end was never published.
pub(crate) fn ns_after(mark: u64, start: u64, end: u64) -> u64 {
    if mark == 0 || end <= mark || end <= start {
        return 0;
    }
    if start >= mark {
        end - start
    } else {
        end - mark
    }
}

fn note_exec_span(runnable: &RunnableSet, start: u64, end: u64) {
    runnable.note_last_exec(end);
    let mark = runnable.span_end_ns();
    runnable.add_post_span_exec_ns(ns_after(mark, start, end));
    if mark != 0 && start >= mark {
        runnable.inc_post_span_exec_n();
    }
}

fn note_validate_span(runnable: &RunnableSet, start: u64, end: u64) {
    runnable.note_last_validate(end);
    runnable.add_post_span_validate_ns(ns_after(runnable.span_end_ns(), start, end));
}

fn publish_span_end(
    scheduler: &Scheduler,
    specfence: SpecFenceCtx<'_>,
    runnable: &RunnableSet,
    origin: u64,
) {
    let n = scheduler.block_size();
    let mut not_started = 0usize;
    let mut unfinished = 0usize;
    let mut owed = 0usize;
    for i in 0..n {
        if specfence
            .tx_first_start
            .get(i)
            .is_some_and(|slot| slot.load(Ordering::Relaxed) == 0)
        {
            not_started += 1;
        }
        let done = scheduler.is_done(i);
        let validated = scheduler.is_validated(i);
        if !done && !validated {
            unfinished += 1;
        } else if done && !validated {
            owed += 1;
        }
    }
    let view = quiet_view(scheduler, specfence, runnable);
    runnable.publish_span_end(
        origin,
        super::runnable_set::SpanEndSnap {
            not_started,
            unfinished,
            owed,
            running: runnable.running_n(),
            pending: runnable.pending_work(),
            indep: runnable.q_indep_len(),
            false_bits: quiet_false_bits(view),
        },
    );
}

/// Inputs for [`quiet_exit`]. Booleans only, so the predicate can be tested
/// without a block.
#[derive(Clone, Copy)]
struct QuietExitView {
    /// Admit shards, released, ordered, or revalidate still hold a tx.
    pending: bool,
    /// Wave bag still holds a wake that [`drain_wave`] has not applied.
    wave_pending: bool,
    /// Spine handoff slot has an unclaimed hop.
    handoff_empty: bool,
    /// Chain hop still owns `spine_owner`.
    spine_busy: bool,
    /// Some tx is `ST_RUNNING`.
    any_running: bool,
    /// Detect still has a gated tx that has not been marked done.
    pending_gated: bool,
    /// A sleeper is waiting for a true tip.
    sleeping: bool,
    /// `ST_WAIT` on a producer that is executing.
    waiting_live: bool,
    /// Some tx is not yet Executed or Validated.
    unfinished: bool,
    /// Some tx is Executed and still needs validate.
    done_unvalidated: bool,
}

fn quiet_view(
    scheduler: &Scheduler,
    specfence: SpecFenceCtx<'_>,
    runnable: &RunnableSet,
) -> QuietExitView {
    QuietExitView {
        pending: runnable.pending_work() != 0,
        wave_pending: specfence.wave.ready_depth() != 0,
        handoff_empty: specfence.spine.handoff_is_empty(),
        spine_busy: specfence.spine.spine_busy(),
        any_running: runnable.any_running(),
        pending_gated: specfence.ready_edges.has_pending_gated(),
        sleeping: specfence.ready_edges.has_sleeping_waiters(),
        waiting_live: runnable.waiting_on_live_producer(specfence.ready_edges, scheduler),
        unfinished: scheduler.has_unfinished(),
        done_unvalidated: scheduler.has_done_unvalidated(),
    }
}

/// Soft=0 QuietExit.
///
/// BlockQuiet (every clause): admit/validate queues empty, the steal probe
/// already missed (`pick` returned `None` and `pending` is still false),
/// handoff empty, no `ST_RUNNING`, no unpublished WaitOnce/ordered tip,
/// every incarnation validated. The validation *counter* is not an input.
///
/// Idle workers also leave while the last executor is still inside validate,
/// once every tx has at least executed and no Avoid tip is unpublished.
/// The host still joins the scope.
fn quiet_exit(v: QuietExitView) -> bool {
    if v.pending || v.wave_pending || !v.handoff_empty || v.waiting_live {
        return false;
    }
    let tips_published = !v.pending_gated && !v.sleeping;
    let settled = !v.unfinished && !v.done_unvalidated;
    // BlockQuiet. A stale spine bit is not a live executor; handled below.
    if !v.any_running && !v.spine_busy && tips_published && settled {
        return true;
    }
    if !tips_published || v.unfinished {
        return false;
    }
    if v.any_running {
        // Last useful worker still holds the core. This idle worker leaves
        // so it does not heal/yield against that core.
        return true;
    }
    // Nobody running. Leave only when validation is not owed. A stale
    // spine owner must not pin `thread::scope` after every tx is validated.
    settled
}

fn block_quiet(v: QuietExitView) -> bool {
    !v.pending
        && !v.wave_pending
        && v.handoff_empty
        && !v.waiting_live
        && !v.any_running
        && !v.spine_busy
        && !v.pending_gated
        && !v.sleeping
        && !v.unfinished
        && !v.done_unvalidated
}

fn quiet_false_bits(v: QuietExitView) -> u64 {
    use super::runnable_set::RunnableSet;
    let mut bits = 0u64;
    if v.pending {
        bits |= RunnableSet::QF_PENDING;
    }
    if v.wave_pending {
        bits |= RunnableSet::QF_WAVE;
    }
    if !v.handoff_empty {
        bits |= RunnableSet::QF_HANDOFF;
    }
    if v.waiting_live {
        bits |= RunnableSet::QF_WAITING_LIVE;
    }
    if v.any_running {
        bits |= RunnableSet::QF_RUNNING;
    }
    if v.spine_busy {
        bits |= RunnableSet::QF_SPINE;
    }
    if v.pending_gated {
        bits |= RunnableSet::QF_GATED;
    }
    if v.sleeping {
        bits |= RunnableSet::QF_SLEEPING;
    }
    if v.unfinished {
        bits |= RunnableSet::QF_UNFINISHED;
    }
    if v.done_unvalidated {
        bits |= RunnableSet::QF_OWED;
    }
    bits
}

fn observe_quiet(
    scheduler: &Scheduler,
    specfence: SpecFenceCtx<'_>,
    runnable: &RunnableSet,
) -> bool {
    let view = quiet_view(scheduler, specfence, runnable);
    if runnable.span_end_ns() != 0 {
        if block_quiet(view) {
            runnable.note_quiet_true(origin_ns(specfence));
        } else if !quiet_exit(view) {
            runnable.note_quiet_false(quiet_false_bits(view));
        }
    }
    quiet_exit(view)
}

fn hang_sched(scheduler: &Scheduler, tx: crate::TxIdx) -> &'static str {
    if scheduler.is_validated(tx) {
        "val"
    } else if scheduler.is_executed(tx) {
        "exed"
    } else if scheduler.is_executing(tx) {
        "exec"
    } else if scheduler.is_aborting(tx) {
        "abt"
    } else if scheduler.is_ready(tx) {
        "rdy"
    } else {
        "?"
    }
}

/// Execute outcome for the SF ring (no Block-STM next-task steal).
#[derive(Debug)]
pub(crate) enum SfExec {
    Executed {
        wrote_new_location: bool,
    },
    /// Parked. `on` is the scheduler-dependency producer when known.
    Blocked {
        on: Option<crate::TxIdx>,
    },
    Fatal,
}

/// `true` when a post-exec window that starts at `now_ns` is outside the
/// prior-crit span `[head.first_start, tail.first_start)`.
///
/// No chain, a zero-width chain, or a head that has not started yet counts
/// as outside. An open span (head started, tail not) counts as inside for
/// the whole window, even if the tail starts mid-window.
pub(crate) const fn post_exec_outside_span(
    head: usize,
    tail: usize,
    head_ns: u64,
    tail_ns: u64,
    now_ns: u64,
) -> bool {
    if head == usize::MAX || tail == usize::MAX || head == tail {
        return true;
    }
    if head_ns == 0 {
        return true;
    }
    if tail_ns == 0 {
        return false;
    }
    now_ns < head_ns || now_ns >= tail_ns
}

fn post_exec_start_outside_span(specfence: SpecFenceCtx<'_>, runnable: &RunnableSet) -> bool {
    let head = runnable.span_head();
    let tail = runnable.span_tail();
    let load = |tx: usize| {
        specfence
            .tx_first_start
            .get(tx)
            .map(|slot| slot.load(Ordering::Relaxed))
            .unwrap_or(0)
    };
    let now_ns = specfence.exec_origin.elapsed().as_nanos() as u64;
    post_exec_outside_span(head, tail, load(head), load(tail), now_ns)
}

fn record_post_exec(runnable: &RunnableSet, outside: bool, ns: u64) {
    if outside {
        runnable.add_post_exec_validate_ns(ns);
    } else {
        runnable.add_post_exec_in_span_ns(ns);
    }
}

fn drain_wave(specfence: SpecFenceCtx<'_>, scheduler: &Scheduler, runnable: &RunnableSet) {
    let mut still_executing = Vec::new();
    while let Some(t) = specfence.wave.pop_ready() {
        if scheduler.is_validated(t) {
            runnable.mark_done(t);
            continue;
        }
        // Do not mark_wait or force_push while the owner is still
        // ST_RUNNING. mark_wait clears the bit (false ghost). force_push
        // lets a second worker enter the same result slot (335 SEGV).
        // add_dependency already set Aborting before Blocked returns, so
        // "not Executing" is not proof the worker left. Put the wake back
        // after this drain; the owner's post-return drain applies it.
        if runnable.is_running(t) {
            still_executing.push(t);
            continue;
        }
        if specfence.ready_edges.leftover_surplus(t)
            || (specfence.ready_edges.is_gated(t) && !specfence.ready_edges.may_execute(t))
        {
            runnable.note_wait_unless_running(t);
            continue;
        }
        // Wave bag is publish / producer-done wake only (not antichain seed).
        // ChainSpine one-hop and gated release both land on Q_released so
        // Avoid is schedule-on-Released; Indep stays for seed antichain fill.
        let kind = super::runnable_set::QueueKind::Released;
        if !runnable.wake_idle(t, kind) && runnable.is_running(t) {
            still_executing.push(t);
        }
    }
    for t in still_executing {
        specfence.wave.push_ready(t);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn worker_source_never_calls_block_stm_pick() {
        let src = include_str!("worker.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("next_task_with_wave_ready("));
        assert!(!code.contains("next_occ_task("));
        assert!(!code.contains("validate_occ_stage("));
        assert!(!code.contains(".next_task("));
        assert!(code.contains("run_sf_block"));
        assert!(code.contains("resolve_plan::apply"));
        assert!(
            code.contains("note_consumer_on"),
            "Blocked parks must plant Detect so heal cannot incarnation++ mill"
        );
        assert!(
            code.contains("quiet_exit"),
            "Soft=0 idle arm must QuietExit on BlockQuiet"
        );
        let idle = code
            .split("None =>")
            .nth(1)
            .expect("idle arm")
            .split("fn hang_sched")
            .next()
            .expect("idle arm body");
        assert!(
            !idle.contains("all_validated()"),
            "idle exit must not wait on the validation tally scan"
        );
    }

    #[test]
    fn quiet_exit_on_block_quiet_and_not_while_work_remains() {
        let quiet = super::QuietExitView {
            pending: false,
            wave_pending: false,
            handoff_empty: true,
            spine_busy: false,
            any_running: false,
            pending_gated: false,
            sleeping: false,
            waiting_live: false,
            unfinished: false,
            done_unvalidated: false,
        };
        assert!(super::quiet_exit(quiet), "BlockQuiet");

        let mut handoff = quiet;
        handoff.handoff_empty = false;
        assert!(!super::quiet_exit(handoff), "spine hop still in the slot");

        let mut wave = quiet;
        wave.wave_pending = true;
        assert!(!super::quiet_exit(wave), "wake not drained");

        let mut live = quiet;
        live.waiting_live = true;
        live.any_running = true;
        assert!(!super::quiet_exit(live), "WaitOnce on a live producer");

        let mut unfinished = quiet;
        unfinished.unfinished = true;
        assert!(!super::quiet_exit(unfinished), "tx not yet executed");

        let mut owed = quiet;
        owed.done_unvalidated = true;
        assert!(
            !super::quiet_exit(owed),
            "Executed tx with nobody left to validate"
        );

        let mut validating = quiet;
        validating.any_running = true;
        validating.done_unvalidated = true;
        assert!(
            super::quiet_exit(validating),
            "idle worker leaves while the last validate runs"
        );

        let mut gated = validating;
        gated.pending_gated = true;
        assert!(
            !super::quiet_exit(gated),
            "unpublished Avoid tip keeps the idle worker"
        );

        let mut stale_spine = quiet;
        stale_spine.spine_busy = true;
        assert!(
            super::quiet_exit(stale_spine),
            "settled block must not wait on a stale spine bit"
        );
    }

    #[test]
    fn post_exec_span_window_is_start_classified() {
        use super::post_exec_outside_span;
        assert!(post_exec_outside_span(usize::MAX, usize::MAX, 0, 0, 10));
        assert!(post_exec_outside_span(3, 3, 10, 10, 10));
        assert!(post_exec_outside_span(1, 9, 0, 0, 50));
        assert!(!post_exec_outside_span(1, 9, 100, 0, 150));
        assert!(post_exec_outside_span(1, 9, 100, 400, 50));
        assert!(!post_exec_outside_span(1, 9, 100, 400, 100));
        assert!(!post_exec_outside_span(1, 9, 100, 400, 399));
        assert!(post_exec_outside_span(1, 9, 100, 400, 400));
    }

    #[test]
    fn post_span_overlap_and_quiet_bits() {
        use super::ns_after;
        use crate::specfence::runnable_set::RunnableSet;
        assert_eq!(ns_after(0, 10, 20), 0);
        assert_eq!(ns_after(100, 40, 90), 0);
        assert_eq!(ns_after(100, 40, 130), 30);
        assert_eq!(ns_after(100, 100, 140), 40);
        assert_eq!(ns_after(100, 110, 140), 30);
        let unfinished = super::QuietExitView {
            pending: true,
            wave_pending: false,
            handoff_empty: true,
            spine_busy: false,
            any_running: true,
            pending_gated: false,
            sleeping: false,
            waiting_live: false,
            unfinished: true,
            done_unvalidated: false,
        };
        let bits = super::quiet_false_bits(unfinished);
        assert_eq!(
            bits,
            RunnableSet::QF_PENDING | RunnableSet::QF_RUNNING | RunnableSet::QF_UNFINISHED
        );
        assert!(!super::block_quiet(unfinished));
    }
}
