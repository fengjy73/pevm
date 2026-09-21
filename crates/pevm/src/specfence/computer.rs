//! SpecFenceComputer — Schedule.pick over RunnableSet (SF-PS).
//!
//! Protocol: Detect → RunnableSet → pick → Execute(vis) → Resolve → Learn.
//! ProducerStage / ReadyEdge / refuse are first-class schedule inputs.
//! Refuse = wave-fill another runnable, not an OCC ready-bag filter.
//!
//! **Never** call the OCC contrast pick from this file. Empty wait-set is the
//! independent antichain (Avoid=noop Opt), still on the SpecFence spine.

use super::metrics::MetricsInner;
use super::policy::CostPolicy;
use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::schedule;
use super::wave::WaveParkTable;
use crate::Task;
use crate::scheduler::Scheduler;

/// SpecFence schedule entry. Delegates to [`schedule::pick`].
#[inline]
pub(crate) fn next_sf_task(
    scheduler: &Scheduler,
    wave: &WaveParkTable,
    ready: &ReadyEdgeTable,
    stages: &ProducerStageTable,
    policy: Option<&CostPolicy>,
    metrics: Option<&MetricsInner>,
) -> Option<Task> {
    schedule::pick(scheduler, wave, ready, stages, policy, metrics)
}

#[cfg(test)]
mod tests {
    #[test]
    fn computer_source_never_calls_next_occ_task() {
        let src = include_str!("computer.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            !code.contains("next_occ_task"),
            "SF-PS computer body must not invoke next_occ_task"
        );
        assert!(
            !code.contains(".next_task()"),
            "SF-PS computer must not retreat to OCC next_task()"
        );
    }
}
