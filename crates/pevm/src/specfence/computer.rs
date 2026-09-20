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
    // S1/P1: no *pending* holes → OCC idx pick. Finished gates are not a
    // global mode switch — independents return to `next_occ_task`.
    if !stages.has_reserved() && !ready.has_pending_gated() {
        return scheduler.next_task();
    }
    // A0-majority / no RAW-fan reservation: OCC-class pick (no empty DashMap scan).
    // Gated txs still need refuse / wave-admit / wake (P2).
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
