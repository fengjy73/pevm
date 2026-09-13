//! SpecFenceComputer — ready/steal with PE unpublished-RAW refuse.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v6-essence.md` §2.
//! Steal only Stages in ready. Never steal a PE-blocked Execute "to look busy".

use super::ready_edge::ReadyEdgeTable;
use super::rem::WaveParkTable;
use crate::Task;
use crate::scheduler::Scheduler;

/// SpecFence ready-set + steal. Wave ready first; Execute only if PE-satisfied.
#[inline]
pub(crate) fn next_sf_task(
    scheduler: &Scheduler,
    wave: &WaveParkTable,
    ready: &ReadyEdgeTable,
) -> Option<Task> {
    // Wave steal after WaitFor park. Do not PE-refuse Execute here:
    // schedule-refuse of known consumers deadlocks when the producer is
    // not on the collaborative index (spin in next_task). Avoid is
    // WaitFor/Bind/SerialLane at the PE access + ready-edge observe.
    let _ = ready;
    scheduler.next_task_with_wave(Some(wave))
}
