//! SpecFenceComputer — fused ready-set on one pevm spine (v10).
//!
//! Protocol: `lab/notes/specfence-complete-architecture-v10-raw-mixed.md`.
//! Scheduler: Stages / steal / pipeline / ProducerStage / wave-fill / wall.
//! Admission: ReadyEdge / lane / OrderedAdmit / PE refuse — first-class, not annotation.
//! Steal only Stages in ready. Never steal a PE-blocked Execute "to look busy".
//! Refuse consumer only when ProducerStage(w) is runnable (v6 deadlock designed out).
//! After refuse: scheduler wave-fills the next independent (`optimistic_read`).
//! A0 / ungated majority: skip empty ProducerStage scan; ReadyEdge tax only on gated txs.

use super::metrics::MetricsInner;
use super::policy::CostPolicy;
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
    policy: Option<&CostPolicy>,
    metrics: Option<&MetricsInner>,
) -> Option<Task> {
    // O1: plant queued leftover continuation hops at this pick quantum
    // (not mid-execute). One hop, never full-spine.
    if let Some(p) = policy
        && p.has_pending_idle()
    {
        let _ = crate::specfence::admit::flush_pending_idle_edges(ready, p);
    }
    let refuse_before = ready.refuse_count();
    // Gates ≠ global mode. Ungated txs keep OCC collaborative task
    // selection; a closed wait-for is skip-only (`next_task_with_wave_ready`).
    // Large + lazy-update already seen → ignore leftover reservations.
    // Mid-band real spines (CallWaw / Win_2, e.g. 19469101 n=469) still
    // have pending gates without ProducerStage reserve — must not OCC-steal
    // through those wait-for deps (PR24 abort-train hang).
    let large_lazy = policy
        .is_some_and(|p| p.block_n() >= super::policy::LARGE_BLOCK_N && p.lazy_already_seen());
    if large_lazy || (!stages.has_reserved() && !ready.has_pending_gated()) {
        return scheduler.next_task();
    }
    if !stages.has_reserved() {
        let task = scheduler.next_task_with_wave_ready(Some(wave), Some(ready));
        if let Some(m) = metrics {
            let n = ready.refuse_count().saturating_sub(refuse_before);
            m.record_refuse_admit_n(n);
        }
        return task;
    }
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
