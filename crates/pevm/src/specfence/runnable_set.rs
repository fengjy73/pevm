//! RunnableSet — Detect-driven ready set (SF-PS §A).
//!
//! `R = AntiChain(independent) ∪ Released(dependency satisfied)`.
//!
//! This is the SpecFence schedule input. It is **not** an OCC ready-bag
//! filter: `refuse_admit` means “pick another member of R” (wave-fill),
//! not “leave the tx sitting in the Block-STM collaborative index”.

use crate::TxIdx;

use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::visibility::VisibilityPolicy;

/// Detect graph view used by [`super::schedule::pick`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunnableSet<'a> {
    ready: &'a ReadyEdgeTable,
    stages: &'a ProducerStageTable,
}

impl<'a> RunnableSet<'a> {
    /// Build R from the current Detect edges + ProducerStage reservations.
    #[inline]
    pub(crate) fn from_detect(ready: &'a ReadyEdgeTable, stages: &'a ProducerStageTable) -> Self {
        Self { ready, stages }
    }

    /// Independent (Avoid=noop) or released consumer.
    #[inline]
    pub(crate) fn is_runnable(&self, tx: TxIdx) -> bool {
        self.ready.may_execute(tx)
    }

    /// Refuse = gated and not released. Caller wave-fills another runnable.
    #[inline]
    pub(crate) fn should_refuse(&self, tx: TxIdx) -> bool {
        self.ready.is_gated(tx) && !self.ready.may_execute(tx)
    }

    /// Visibility for a picked tx (Avoid=noop Opt vs edged).
    #[inline]
    pub(crate) fn visibility(&self, tx: TxIdx) -> VisibilityPolicy {
        VisibilityPolicy::for_ready(self.ready, tx)
    }

    /// Conflict-subgraph progress: reserved producer Stages.
    #[inline]
    pub(crate) fn has_producer_work(&self) -> bool {
        self.stages.has_reserved()
    }

    #[inline]
    pub(crate) fn next_producer(&self) -> Option<TxIdx> {
        self.stages.next_reserved()
    }

    #[inline]
    pub(crate) fn ready_edges(&self) -> &'a ReadyEdgeTable {
        self.ready
    }

    #[inline]
    pub(crate) fn stages(&self) -> &'a ProducerStageTable {
        self.stages
    }

    /// Best-effort runnable width: wave bag + unblocked gated + antichain hint.
    #[inline]
    pub(crate) fn width_hint(&self) -> usize {
        let pending = self.ready.pending_gated_count();
        let gated = self.ready.gated_count();
        gated.saturating_sub(pending).saturating_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_is_not_occ_bag() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        ready.note_consumer(4, 1);
        let r = RunnableSet::from_detect(&ready, &stages);
        assert!(r.is_runnable(0), "tx0 is always antichain");
        assert!(r.is_runnable(2), "ungated independent is runnable");
        assert!(r.should_refuse(4), "known consumer refused until release");
        assert_eq!(r.visibility(2), VisibilityPolicy::Opt);
    }
}
