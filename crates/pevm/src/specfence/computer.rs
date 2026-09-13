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
    // Observe/wake still use `ready`. Do **not** refuse Execute(t) here:
    // Block-STM needs the consumer to start so it can plant ESTIMATE writes.
    // Suffix-global or known-consumer schedule-refuse deferred writers' later
    // dependents and inflated validation aborts (14689597: 229 vs OCC 37).
    // First-wave Avoid stays at the PE access (WaitFor / Bind / SerialLane).
    let _ = ready;
    scheduler.next_task_with_wave(Some(wave))
}
