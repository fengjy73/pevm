//! ResolvePlan — validate produces a structured plan (SF-PS §C).
//!
//! SpecFence edged paths apply Resolve **before** bool-fail → incarnation++.
//! Independent / Opt (Avoid=noop) still uses Commit or FullReplay — that is
//! the DAG independent-set algorithm, not a retreat to the OCC computer.

/// Structured validate outcome. Replaces “bool valid → abort” as the SF root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvePlan {
    /// Read set still matches; commit this incarnation.
    Commit,
    /// Value-stable rebind of invalid reads (same incarnation).
    PartialAbortRebind,
    /// Certified prefix kept; rewind uncertified suffix once.
    PartialAbortRewind,
    /// Replay the conflict segment under OrderedTip visibility.
    OrderedReplay,
    /// Whole-tx re-enter RunnableSet (Learn penalty). Not “this is OCC”.
    FullReplay,
}

impl ResolvePlan {
    /// True when this plan commits without a new EVM head.
    #[inline]
    pub const fn commits(self) -> bool {
        matches!(self, Self::Commit | Self::PartialAbortRebind)
    }

    /// True when Resolve repaired instead of throwing the tx.
    #[inline]
    pub const fn is_partial(self) -> bool {
        matches!(self, Self::PartialAbortRebind | Self::PartialAbortRewind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_kinds() {
        assert!(ResolvePlan::Commit.commits());
        assert!(ResolvePlan::PartialAbortRebind.commits());
        assert!(ResolvePlan::PartialAbortRebind.is_partial());
        assert!(!ResolvePlan::FullReplay.commits());
        assert!(!ResolvePlan::OrderedReplay.is_partial());
    }
}
