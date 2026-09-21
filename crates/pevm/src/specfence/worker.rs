//! SpecFence worker ring: RunnableSet.pick → Execute(vis) → Resolve.apply.
//!
//! This is the product loop. It must not call `Scheduler::next_task*` or
//! the OCC-stage validate entry. OCC contrast stays in `pevm.rs`.

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
    let mut spins = 0u64;
    loop {
        spins += 1;
        if spins.is_multiple_of(8_000) && std::env::var_os("SPECFENCE_HANG_TRACE").is_some() {
            let (n_min, n_tip, n_ov) = specfence.ready_edges.hang_plant_n();
            let (g_min, g_chain) = specfence.ready_edges.hang_global_leftover();
            let (min_done, min_run, min_pred) = specfence.ready_edges.hang_leftover_min_status();
            let n_unf = (0..scheduler.block_size())
                .filter(|&t| !scheduler.is_validated(t))
                .count();
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
                "sf-hang-trace spins={spins} pending={} q_i/r/o/v={}/{}/{}/{} unfinished={} validated={} gated={} live_wait={} refuse={} leftover_min={} loc_tip={} overflow_tip={} glob_min={} glob_chain={} min_done={} min_exec={} min_pred={} min_st={} n_unf={} n_run={}",
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
        );
        match task {
            Some(Task::Execution(tx_version)) => {
                let tx_idx = tx_version.tx_idx;
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
                        // add_dependency parks leave Aborting with no Detect
                        // edge. Heal then recovered them into a live antichain
                        // (incarnation++ mill — 6196166 reuse 206k / 19807137).
                        if let Some(w) = on {
                            if specfence.ready_edges.is_live_leftover_min(tx_idx)
                                && specfence.ready_edges.leftover_min_skips_blocker(w)
                            {
                                // leftover leftover_min already passed (204 when
                                // leftover_min=205). Detect-plant gates leftover_min
                                // off pick; refuse mill heap-aborts (unaligned tcache).
                                // Flush leftover_min ← w only — no recover / force_push.
                                specfence.ready_edges.flush_wait_on(tx_idx, w);
                                let _ = scheduler.detach_dependent(w, tx_idx);
                                runnable.mark_wait(tx_idx);
                            } else if w < tx_idx {
                                specfence.ready_edges.note_consumer_on(tx_idx, w, None);
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
                                    runnable.force_push(w, kind);
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
                    SfExec::Fatal => break,
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
                if runnable.waiting_on_live_producer(specfence.ready_edges, scheduler) {
                    for _ in 0..32 {
                        std::hint::spin_loop();
                    }
                    continue;
                }
                std::thread::yield_now();
            }
        }
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
    while let Some(t) = specfence.wave.pop_ready() {
        if scheduler.is_validated(t) {
            runnable.mark_done(t);
            continue;
        }
        // force_push: the waiter may still be ST_RUNNING inside try_execute_sf
        // (add_dependency succeeded, Blocked not yet returned). push() would
        // refuse and drop the wake.
        if specfence.ready_edges.leftover_surplus(t)
            || (specfence.ready_edges.is_gated(t) && !specfence.ready_edges.may_execute(t))
        {
            runnable.mark_wait(t);
            continue;
        }
        let kind = if specfence.ready_edges.is_gated(t) {
            super::runnable_set::QueueKind::Released
        } else {
            super::runnable_set::QueueKind::Indep
        };
        runnable.force_push(t, kind);
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
