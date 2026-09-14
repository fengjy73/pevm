//! SpecFenceComputer — fused ready-set on one pevm spine (v10).
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v10-raw-mixed.md`.
//! PC: Stages / steal / pipeline / ProducerStage / wave-fill / wall.
//! CC: ReadyEdge / lane / OrderedAdmit / PE refuse — first-class, not annotation.
//! Steal only Stages in ready. Never steal a PE-blocked Execute "to look busy".
//! Refuse consumer only when ProducerStage(w) is runnable (v6 deadlock designed out).
//! After refuse: scheduler wave-fills the next independent (`optimistic_read`).

use super::metrics::MetricsInner;
use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::wave::WaveParkTable;
use crate::Task;
use crate::scheduler::Scheduler;

/// SpecFence ready-set + steal. ProducerStages first; then PE-satisfied Execute.
#[inline]
pub(crate) fn next_sf_task(
    scheduler: &Scheduler,
    wave: &WaveParkTable,
    ready: &ReadyEdgeTable,
    stages: &ProducerStageTable,
    metrics: Option<&MetricsInner>,
) -> Option<Task> {
    let refuse_before = ready.refuse_count();
    // Drop Aborting / Done reservations so a dead writer cannot wait_for_dependency the
    // ProducerStage min and starve the rest of the ready-set.
    for _ in 0..8 {
        let Some(w) = stages.next_reserved() else {
            break;
        };
        if scheduler.is_done(w) {
            stages.note_done(w);
            continue;
        }
        scheduler.admit_spine(w, wave);
        stages.note_promote();
        if let Some(tx_version) = scheduler.try_execute_producer(w) {
            if let Some(m) = metrics {
                let n = ready.refuse_count().saturating_sub(refuse_before);
                m.record_refuse_admit_n(n);
            }
            return Some(Task::Execution(tx_version));
        }
        if !scheduler.producer_stage_runnable(w) {
            stages.note_done(w);
            continue;
        }
        break;
    }
    let task = scheduler.next_task_with_wave_ready(Some(wave), Some(ready));
    if let Some(m) = metrics {
        let n = ready.refuse_count().saturating_sub(refuse_before);
        m.record_refuse_admit_n(n);
    }
    task
}
