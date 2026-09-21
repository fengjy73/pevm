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
    loop {
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
                let vis = VisibilityPolicy::for_ready(specfence.ready_edges, tx_idx);
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
                    SfExec::Blocked => {
                        runnable.mark_wait(tx_idx);
                        drain_wave(specfence, scheduler, runnable);
                    }
                    SfExec::Fatal => break,
                }
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
                let _ = specfence
                    .ready_edges
                    .wake_ready_sleepers(specfence.wave);
                let _ = runnable.heal(specfence.ready_edges, scheduler);
                drain_wave(specfence, scheduler, runnable);
                if scheduler.all_validated() && runnable.pending_work() == 0 {
                    break;
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
    Executed { wrote_new_location: bool },
    Blocked,
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
        let kind = if specfence.ready_edges.was_queued(t) {
            super::runnable_set::QueueKind::Ordered
        } else if specfence.ready_edges.is_gated(t) {
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
    }
}
