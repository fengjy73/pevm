//! SpecFenceComputer — PC ready/steal fused with CC PE refuse.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.
//! PC owns ProducerStage progress + steal. CC owns ReadyEdges.
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
    if let Some(w) = stages.next_reserved() {
        if !scheduler.is_done(w) {
            scheduler.admit_spine(w, wave);
            stages.note_promote();
            if let Some(tx_version) = scheduler.try_execute_producer(w) {
                return Some(Task::Execution(tx_version));
            }
        } else {
            stages.note_done(w);
        }
    }
    scheduler.next_task_with_wave_ready(Some(wave), Some(ready))
}
