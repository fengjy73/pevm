//! Resolution algebra helpers: WaitHard / Bind / SpecRead / EarlyVal /
//! Rebind / InvalidateSelective (SpecFence Spec v1 §6).
//!
//! v6 π: expected-cost comparison (cost-aware) to cut over-WaitHard.

#![allow(dead_code)]
use crate::{MemoryLocationHash, TxIdx, TxIncarnation, TxVersion};

/// Policy action chosen by π for one region read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ResolveAction {
    /// Block until last writer `< t` has non-ESTIMATE Data (or aborted).
    WaitHard,
    /// Read exact version `v=(t_w,inc)` once published; `t_w < t`.
    Bind(TxVersion),
    /// OrderedDirtyRead: last Data `< t` skipping ESTIMATE (else Wait).
    SpecRead,
}

/// Context features for π (Phase-1 + v6 producer signal).
#[derive(Debug, Clone)]
pub(crate) struct PolicyCtx {
    pub location: MemoryLocationHash,
    pub writer_known: bool,
    pub writer: Option<TxIdx>,
    /// True when last writer is Executed/Validated (wait is cheap).
    pub writer_done: bool,
    pub posterior_conflict: f64,
    pub posterior_bind_success: f64,
    pub placeholder_ready: bool,
    pub bind_version: Option<TxVersion>,
}

/// Legacy Spec v1 §7.3 thresholds (kept for revoke / seed docs; live π is cost-aware).
pub(crate) const TAU_W: f64 = 0.35;
pub(crate) const TAU_S: f64 = 0.50;
pub(crate) const TAU_REVOKE: f64 = 0.20;

/// Partial-retry reexec factor in expected SpecRead cost (`1 + P * C_retry`).
pub(crate) const C_RETRY: f64 = 3.0;
/// Wait must be clearly cheaper than Spec (`cost_wait < cost_spec * margin`).
/// `< 1` raises the Wait bar so SpecRead is the default unless wait wins.
pub(crate) const COST_MARGIN: f64 = 0.40;
/// Safety valve: always WaitHard when conflict posterior is very high.
pub(crate) const TAU_VERY_HIGH: f64 = 0.75;

/// Estimated producer remaining work: 0 if published/done, else 1.0 unit.
#[inline]
pub(crate) fn cost_wait(writer_done: bool) -> f64 {
    if writer_done { 0.0 } else { 1.0 }
}

/// Expected SpecRead cost: base progress + conflict-weighted retry.
#[inline]
pub(crate) fn cost_spec(p_conflict: f64) -> f64 {
    1.0 + p_conflict.clamp(0.0, 1.0) * C_RETRY
}

/// True when cost model prefers WaitHard over SpecRead.
#[inline]
pub(crate) fn cost_prefers_wait(
    writer_known: bool,
    writer_done: bool,
    p_conflict: f64,
) -> bool {
    let p = p_conflict.clamp(0.0, 1.0);
    if writer_known && cost_wait(writer_done) < cost_spec(p) * COST_MARGIN {
        return true;
    }
    p >= TAU_VERY_HIGH
}

/// Cost-aware π: Bind if ready; else WaitHard only when wait is clearly
/// cheaper than SpecRead (or P ≥ τ_very_high); else SpecRead.
pub(crate) fn choose_action(ctx: PolicyCtx) -> ResolveAction {
    // Bind when a concrete published version is ready.
    if let Some(v) = ctx.bind_version.clone() {
        if ctx.placeholder_ready || ctx.writer_done {
            return ResolveAction::Bind(v);
        }
    }
    if ctx.placeholder_ready {
        if let Some(v) = ctx.bind_version {
            return ResolveAction::Bind(v);
        }
    }

    if cost_prefers_wait(ctx.writer_known, ctx.writer_done, ctx.posterior_conflict) {
        return ResolveAction::WaitHard;
    }
    ResolveAction::SpecRead
}

/// EarlyVal probability linear in P_conflict (clamped).
pub(crate) fn early_val_probability(p_conflict: f64) -> f64 {
    p_conflict.clamp(0.0, 1.0)
}

/// Result of a selective invalidate attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectiveOutcome {
    /// ESTIMATE marked only on locations with known higher readers.
    Selective,
    /// Fell back to full write-set ESTIMATE for safety.
    FallbackFull,
}

/// Describe a Bind target for Bohm-lite residual write-set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BindTarget {
    pub writer: TxIdx,
    pub incarnation: TxIncarnation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(
        p: f64,
        writer_known: bool,
        writer_done: bool,
        bind: Option<TxVersion>,
        placeholder_ready: bool,
    ) -> PolicyCtx {
        PolicyCtx {
            location: 1,
            writer_known,
            writer: if writer_known { Some(0) } else { None },
            writer_done,
            posterior_conflict: p,
            posterior_bind_success: 0.1,
            placeholder_ready,
            bind_version: bind,
        }
    }

    #[test]
    fn cost_high_p_writer_done_waits() {
        // Writer published → cost_wait=0 → WaitHard (or Bind if version given).
        let a = choose_action(ctx(0.8, true, true, None, false));
        assert_eq!(a, ResolveAction::WaitHard);
    }

    #[test]
    fn cost_moderate_p_writer_not_done_specs() {
        // P=0.45, writer running: cost_wait=1, cost_spec=1+1.35=2.35, *0.4=0.94
        // 1 < 0.94? false; P < 0.75 → SpecRead.
        let a = choose_action(ctx(0.45, true, false, None, false));
        assert_eq!(a, ResolveAction::SpecRead);
    }

    #[test]
    fn cost_bind_ready_prefers_bind() {
        let v = TxVersion {
            tx_idx: 3,
            tx_incarnation: 0,
        };
        let a = choose_action(ctx(0.9, true, true, Some(v.clone()), true));
        assert_eq!(a, ResolveAction::Bind(v));
    }

    #[test]
    fn cost_safety_valve_very_high_p() {
        // Writer unknown but P very high → WaitHard.
        let a = choose_action(ctx(0.80, false, false, None, false));
        assert_eq!(a, ResolveAction::WaitHard);
    }

    #[test]
    fn cost_constants_raise_wait_bar() {
        assert!(COST_MARGIN < 1.0);
        assert!(C_RETRY >= 2.0 && C_RETRY <= 4.0);
        assert!((TAU_VERY_HIGH - 0.75).abs() < f64::EPSILON);
    }
}
