//! SpecFence worker ring: RunnableSet.pick → Execute(vis) → Resolve.apply.
//!
//! This is the product loop. It must not call `Scheduler::next_task*` or
//! the OCC-stage validate entry. OCC contrast stays in `pevm.rs`.

use std::sync::atomic::Ordering;
use std::time::Instant;

use super::SpecFenceCtx;
use super::VisibilityPolicy;
use super::arm_table::ArmTable;
use super::resolve_plan::{self, ApplyCtx};
use super::runnable_set::RunnableSet;
use super::schedule;
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
    // Unpark peers on every exit so a parked worker cannot outlive the scope.
    struct ExitWake<'a>(&'a RunnableSet);
    impl Drop for ExitWake<'_> {
        fn drop(&mut self) {
            self.0.wake_all_teardown();
        }
    }
    let _exit_wake = ExitWake(runnable);
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
            break;
        }
        if scheduler.all_validated() && runnable.pending_work() == 0 {
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
                if let Some(slot) = specfence.tx_first_start.get(tx_idx) {
                    if slot.load(Ordering::Relaxed) == 0 {
                        let ns = specfence.exec_origin.elapsed().as_nanos() as u64;
                        let _ = slot.compare_exchange(
                            0,
                            ns.max(1),
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        );
                    }
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
                match execute(tx_version.clone(), vis) {
                    SfExec::Executed { wrote_new_location } => {
                        // Next hop may start on another core while we validate.
                        // Ownership ends with the hop, not with validation.
                        specfence.spine.release_owner_tx(tx_idx);
                        // finish_execution may have waved dependents — park them
                        // on RunnableSet before this worker spends time validating.
                        drain_wave(specfence, scheduler, runnable);
                        let (plan, invalid) = validate_to_plan(&tx_version, vis);
                        resolve_plan::apply(
                            plan,
                            ApplyCtx {
                                specfence,
                                mv_memory,
                                scheduler,
                                runnable,
                                arms,
                                tx_version: &tx_version,
                                vis,
                                wrote_new_location,
                                invalid: &invalid,
                            },
                        );
                    }
                    SfExec::Blocked { on } => {
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
                        specfence.spine.release_owner_tx(tx_idx);
                        break;
                    }
                }
                metrics.add_worker_busy_ns(t0.elapsed().as_nanos() as u64);
            }
            Some(Task::Validation(tx_version)) => {
                let vis = VisibilityPolicy::for_ready(specfence.ready_edges, tx_version.tx_idx);
                let (plan, invalid) = validate_to_plan(&tx_version, vis);
                resolve_plan::apply(
                    plan,
                    ApplyCtx {
                        specfence,
                        mv_memory,
                        scheduler,
                        runnable,
                        arms,
                        tx_version: &tx_version,
                        vis,
                        wrote_new_location: false,
                        invalid: &invalid,
                    },
                );
                metrics.add_worker_busy_ns(t0.elapsed().as_nanos() as u64);
            }
            None => {
                // Idle: `pick` already stole the antichain tail. Help release
                // is heal of writers that have published.
                metrics.add_idle_core_ns(t0.elapsed().as_nanos() as u64);
                runnable.note_idle_spin();
                if abort() {
                    break;
                }
                specfence
                    .ready_edges
                    .heal_finished_preds(|w| scheduler.is_done(w));
                let _ = specfence.ready_edges.wake_ready_sleepers(specfence.wave);
                let _ = runnable.heal(specfence.ready_edges, scheduler);
                drain_wave(specfence, scheduler, runnable);
                if scheduler.all_validated() && runnable.pending_work() == 0 {
                    break;
                }
                // Last-ditch: unfinished + empty queues + no live producer.
                // Heal already ran; if still stuck, yield then retry heal.
                if runnable.pending_work() == 0
                    && scheduler.has_unfinished()
                    && !runnable.waiting_on_live_producer(specfence.ready_edges, scheduler)
                {
                    let _ = runnable.heal(specfence.ready_edges, scheduler);
                    drain_wave(specfence, scheduler, runnable);
                    if runnable.pending_work() == 0 {
                        let _ = runnable.force_idle_recover(specfence.ready_edges, scheduler);
                        drain_wave(specfence, scheduler, runnable);
                    }
                    if runnable.pending_work() > 0 {
                        continue;
                    }
                }
                if runnable.pending_work() > 0 {
                    continue;
                }
                // ExactWake: one parked core, woken by the next AdmitIndep push
                // or by teardown. Busy-poll a moment first so a publish in
                // flight does not pay the park.
                if runnable.any_running()
                    || runnable.waiting_on_live_producer(specfence.ready_edges, scheduler)
                {
                    for _ in 0..32 {
                        std::hint::spin_loop();
                    }
                    if runnable.pending_work() > 0 {
                        continue;
                    }
                    runnable.park_idle(worker_i);
                    continue;
                }
                std::thread::yield_now();
            }
        }
    }
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
    }
}
