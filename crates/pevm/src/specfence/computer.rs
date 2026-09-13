//! SpecFenceComputer — PC ⊗ CC jointly author the ready-set.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.
//! PC: Stages / steal / pipeline / ProducerStage / wall.
//! CC: ReadyEdge / lane / OrderedAdmit / PE refuse — first-class, not annotation.
//! Steal only Stages in ready. Never steal a PE-blocked Execute "to look busy".
//! Refuse consumer only when ProducerStage(w) is runnable (v6 deadlock designed out).

use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::rem::WaveParkTable;
use crate::Task;
use crate::scheduler::Scheduler;

/// SpecFence ready-set + steal. ProducerStages first; then PE-satisfied Execute.
#[inline]
pub(crate) fn next_sf_task(
    scheduler: &Scheduler,
    wave: &WaveParkTable,
    ready: &ReadyEdgeTable,
    stages: &ProducerStageTable,
) -> Option<Task> {
    // Drop Aborting / Done reservations so a dead writer cannot pin the
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
            return Some(Task::Execution(tx_version));
        }
        if !scheduler.producer_stage_runnable(w) {
            stages.note_done(w);
            continue;
        }
        break;
    }
    scheduler.next_task_with_wave_ready(Some(wave), Some(ready))
}
