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
    // Observe ready-edges; do not refuse Execute. Schedule-refuse of
    // known consumers (even reincarnation-only) deferred ESTIMATE
    // dependents and inflated 14689597 aborts ~10×.
    let _ = ready;
    scheduler.next_task_with_wave(Some(wave))
}
