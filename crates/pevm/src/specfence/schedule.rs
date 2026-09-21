//! Schedule.pick — SpecFence Parallel Spine pick (SF-PS T1 / PC).
//!
//! **Forbidden:** `Scheduler::next_task` / wave-ready next-task / OCC stage
//! validate. Any Block-STM index cursor as the pick host is banned.

use super::arm_table::ArmTable;
use super::metrics::MetricsInner;
use super::policy::CostPolicy;
use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::runnable_set::{RunnableSet, SfPick};
use super::wave::WaveParkTable;
use crate::Task;
use crate::scheduler::Scheduler;

/// SpecFence schedule entry. Queues + steal only.
#[inline]
pub(crate) fn pick(
    scheduler: &Scheduler,
    wave: &WaveParkTable,
    ready: &ReadyEdgeTable,
    stages: &ProducerStageTable,
    runnable: &RunnableSet,
    arms: &ArmTable,
    policy: Option<&CostPolicy>,
    metrics: Option<&MetricsInner>,
    worker_i: usize,
) -> Option<Task> {
    let _ = (wave, stages);
    if let Some(p) = policy
        && p.has_pending_idle()
    {
        if p.skip_useless_cover_probe() || p.skip_reuse_leftover_flush() {
            let _ = p.take_pending_idle();
        } else {
            let _ = crate::specfence::admit::flush_pending_idle_edges(ready, p);
        }
    }

    // IntraPatch at the pick quantum — never inside the interpreter frame.
    let _ = arms.apply_pending_patches(
        runnable,
        ready,
        policy,
        runnable.cores(),
        scheduler.block_size(),
    );

    if let Some(m) = metrics {
        m.record_sf_schedule_pick();
        runnable.sample_width();
        m.sample_runnable_width(runnable.width_hint());
    }

    let refuse_before = runnable.refuse_fill_n();
    for _ in 0..16 {
        match runnable.pick(worker_i, ready) {
            Some(SfPick::Execute {
                tx,
                vis,
                refused,
                ..
            }) => {
                if scheduler.is_validated(tx) {
                    runnable.mark_done(tx);
                    continue;
                }
                if scheduler.is_executed(tx) {
                    if let Some(v) = scheduler.prepare_revalidate(tx) {
                        if let Some(m) = metrics {
                            m.record_visibility(vis);
                        }
                        return Some(Task::Validation(v));
                    }
                    continue;
                }
                if let Some(tx_version) = scheduler.try_execute_producer(tx) {
                    if refused {
                        arms.note_e4();
                    }
                    if let Some(m) = metrics {
                        m.record_visibility(vis);
                        if refused {
                            m.record_refuse_fill(1);
                        }
                    }
                    return Some(Task::Execution(tx_version));
                }
                if scheduler.is_ready(tx) {
                    // Race: still Ready but claim failed — owner requeue.
                    runnable.force_push(tx, super::runnable_set::QueueKind::Indep);
                    continue;
                }
                if scheduler.is_executed(tx) {
                    if let Some(v) = scheduler.prepare_revalidate(tx) {
                        return Some(Task::Validation(v));
                    }
                    runnable.force_push(tx, super::runnable_set::QueueKind::Revalidate);
                }
            }
            Some(SfPick::Revalidate(tx)) => {
                if let Some(v) = scheduler.prepare_revalidate(tx) {
                    if let Some(m) = metrics {
                        m.record_visibility(runnable.visibility(ready, tx));
                    }
                    return Some(Task::Validation(v));
                }
                if scheduler.is_ready(tx) {
                    if let Some(tx_version) = scheduler.try_execute_producer(tx) {
                        return Some(Task::Execution(tx_version));
                    }
                    runnable.force_push(tx, super::runnable_set::QueueKind::Indep);
                } else if scheduler.is_executed(tx) {
                    runnable.force_push(tx, super::runnable_set::QueueKind::Revalidate);
                } else {
                    runnable.mark_wait(tx);
                }
            }
            None => break,
        }
    }

    if let Some(m) = metrics {
        let n = runnable.refuse_fill_n().saturating_sub(refuse_before);
        if n > 0 {
            m.record_refuse_fill(n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn pick_source_never_calls_block_stm_next_task() {
        let src = include_str!("schedule.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            !code.contains("next_task_with_wave_ready(") && !code.contains(".next_task("),
            "SF-PS pick must not call Scheduler::next_task*"
        );
        assert!(!code.contains("next_occ_task("));
        assert!(!code.contains("validate_occ_stage("));
    }
}
