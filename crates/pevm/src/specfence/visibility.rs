//! VisibilityPolicy — SpecFence read-plane policy (SF-PS §B).
//!
//! Three policies, chosen from the Detect graph — **not** a global OCC MV walk.
//!
//! - [`VisibilityPolicy::Opt`]: no edge / independent antichain. SpecFence
//!   **Avoid=noop** (DAG independent-set algorithm). Implementation may reuse
//!   the existing optimistic MV read. This is **not** `ConcurrencyMode::Occ`.
//! - [`VisibilityPolicy::WaitReleased`]: edged access; run only after the
//!   producer is released (Validated/Committed). Do not discover WAW by
//!   racing an unfinished writer then aborting.
//! - [`VisibilityPolicy::OrderedTip`]: edged access; read the ordered
//!   producer tip (OrderedAdmit / WAW window).

use crate::TxIdx;

use super::ready_edge::ReadyEdgeTable;

/// Per-access / per-tx visibility for Execute (SF-PS first-class).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisibilityPolicy {
    /// Independent / no-edge: Avoid=noop, optimistic read then validate.
    Opt,
    /// Dependency edge: execute after producer release.
    WaitReleased,
    /// Dependency edge: read the ordered producer tip.
    OrderedTip,
}

impl VisibilityPolicy {
    /// Detect-driven visibility for `tx` on the current ReadyEdge graph.
    ///
    /// Ungated txs are the DAG antichain → [`Self::Opt`] (Avoid=noop).
    /// Released gated consumers → [`Self::OrderedTip`] when Detect bound a
    /// location queue, else [`Self::WaitReleased`].
    #[inline]
    pub(crate) fn for_ready(ready: &ReadyEdgeTable, tx: TxIdx) -> Self {
        if !ready.is_gated(tx) {
            return Self::Opt;
        }
        if !ready.may_execute(tx) {
            // Schedule should have refused. If a force-optimistic override
            // still executes, that is still Avoid=noop — not OCC mode.
            return Self::Opt;
        }
        if ready.admitted_on_location(tx) {
            Self::OrderedTip
        } else {
            Self::WaitReleased
        }
    }

    /// True when this is the independent-set Avoid=noop path.
    #[inline]
    pub const fn is_opt(self) -> bool {
        matches!(self, Self::Opt)
    }

    /// Edged access: take the SpecFence fence/wave execute wrap.
    #[inline]
    pub const fn needs_fence(self) -> bool {
        !self.is_opt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ungated_is_avoid_noop_opt() {
        let ready = ReadyEdgeTable::new();
        assert_eq!(
            VisibilityPolicy::for_ready(&ready, 3),
            VisibilityPolicy::Opt
        );
        assert!(VisibilityPolicy::Opt.is_opt());
        assert!(!VisibilityPolicy::Opt.needs_fence());
    }

    #[test]
    fn released_location_consumer_is_ordered_tip() {
        let ready = ReadyEdgeTable::new();
        let wave = super::super::wave::WaveParkTable::new();
        ready.note_consumer_on(3, 1, Some(0xabc));
        ready.note_producer_done(1, &wave);
        assert!(ready.may_execute(3));
        assert_eq!(
            VisibilityPolicy::for_ready(&ready, 3),
            VisibilityPolicy::OrderedTip
        );
        assert!(VisibilityPolicy::OrderedTip.needs_fence());
    }

    #[test]
    fn released_anonymous_consumer_is_wait_released() {
        let ready = ReadyEdgeTable::new();
        let wave = super::super::wave::WaveParkTable::new();
        ready.note_consumer(3, 1);
        ready.note_producer_done(1, &wave);
        assert!(ready.may_execute(3));
        assert_eq!(
            VisibilityPolicy::for_ready(&ready, 3),
            VisibilityPolicy::WaitReleased
        );
    }
}
