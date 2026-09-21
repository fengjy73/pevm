//! Schedule.pick — SpecFence Parallel Spine pick (SF-PS §A).
//!
//! Main loop: Detect → RunnableSet → pick → Execute(vis) → Validate.to_resolve
//! → Resolve.apply → Learn.
//!
//! **Forbidden as SpecFence main pick:** the OCC contrast `next_task` entry.
//! Empty wait-set is Avoid=noop antichain pick on this spine, not a retreat
//! to the Block-STM OCC computer.
//!
//! `refuse_admit` = wave-fill the next independent / released member of R.

use super::metrics::MetricsInner;
use super::policy::CostPolicy;
use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::runnable_set::RunnableSet;
use super::wave::WaveParkTable;
use crate::Task;
use crate::scheduler::Scheduler;

/// SpecFence schedule entry. Never calls the OCC contrast pick.
#[inline]
pub(crate) fn pick(
    scheduler: &Scheduler,
    wave: &WaveParkTable,
    ready: &ReadyEdgeTable,
    stages: &ProducerStageTable,
    policy: Option<&CostPolicy>,
    metrics: Option<&MetricsInner>,
) -> Option<Task> {
    // O1: plant queued leftover continuation hops at this pick quantum
    // (not mid-execute). One hop, never full-spine.
    // Near-independent / lazy-update large — drop leftover hops; do not
    // plant a useless cover wait-set (lazy is never an OrderedAdmit object).
    if let Some(p) = policy
        && p.has_pending_idle()
    {
        if p.skip_useless_cover_probe() || p.skip_reuse_leftover_flush() {
            let _ = p.take_pending_idle();
        } else {
            let _ = crate::specfence::admit::flush_pending_idle_edges(ready, p);
        }
    }

    let runnable = RunnableSet::from_detect(ready, stages);
    if let Some(m) = metrics {
        m.record_sf_schedule_pick();
        m.sample_runnable_width(runnable.width_hint());
    }

    let refuse_before = ready.refuse_count();
    // Large lazy leftover reservations are not OrderedAdmit objects.
    // Skip ProducerStage promote **and** ReadyEdge refuse so leftover
    // hops cannot serialize the block. Still Schedule.pick (wave host),
    // never the OCC contrast pick.
    let ignore_leftover = policy.is_some_and(|p| p.ignore_leftover_reservations());
    if ignore_leftover {
        let task = scheduler.next_task_with_wave_ready(Some(wave), None);
        if let (Some(m), Some(Task::Execution(v))) = (metrics, &task) {
            m.record_visibility(runnable.visibility(v.tx_idx));
        }
        return task;
    }
    if runnable.has_producer_work() {
        for _ in 0..8 {
            let Some(w) = runnable.next_producer() else {
                break;
            };
            if scheduler.is_done(w) {
                stages.note_done(w);
                continue;
            }
            scheduler.admit_spine(w, wave);
            stages.note_promote();
            if let Some(tx_version) = scheduler.try_execute_producer(w) {
                record_refuse(metrics, ready, refuse_before);
                if let Some(m) = metrics {
                    m.record_visibility(runnable.visibility(tx_version.tx_idx));
                }
                return Some(Task::Execution(tx_version));
            }
            if !scheduler.producer_stage_runnable(w) {
                stages.note_done(w);
                continue;
            }
            break;
        }
    }

    // Host index walk + wave bag + ReadyEdge refuse. This is Schedule.pick
    // over RunnableSet, including the independent antichain (Avoid=noop).
    let task = scheduler.next_task_with_wave_ready(Some(wave), Some(ready));
    record_refuse(metrics, ready, refuse_before);
    if let (Some(m), Some(Task::Execution(v))) = (metrics, &task) {
        m.record_visibility(runnable.visibility(v.tx_idx));
    }
    task
}

#[inline]
fn record_refuse(metrics: Option<&MetricsInner>, ready: &ReadyEdgeTable, refuse_before: usize) {
    if let Some(m) = metrics {
        let n = ready.refuse_count().saturating_sub(refuse_before);
        m.record_refuse_admit_n(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_source_never_calls_next_occ_task() {
        let src = include_str!("schedule.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            !code.contains("next_occ_task"),
            "SF-PS pick body must not invoke next_occ_task"
        );
        assert!(
            !code.contains(".next_task()"),
            "SF-PS pick must not retreat to OCC next_task()"
        );
    }
}
