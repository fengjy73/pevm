//! Resolution algebra helpers: WaitHard / Bind / SpecRead / EarlyVal /
//! Rebind / InvalidateSelective (SpecFence Spec v1 §6).

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

/// Context features for π (Phase-1).
#[derive(Debug, Clone)]
pub(crate) struct PolicyCtx {
    pub location: MemoryLocationHash,
    pub writer_known: bool,
    pub writer: Option<TxIdx>,
    pub posterior_conflict: f64,
    pub posterior_bind_success: f64,
    pub placeholder_ready: bool,
    pub bind_version: Option<TxVersion>,
}

/// Defaults from Spec v1 §7.3.
pub(crate) const TAU_W: f64 = 0.35;
pub(crate) const TAU_S: f64 = 0.50;
pub(crate) const TAU_REVOKE: f64 = 0.20;

/// Choose WaitHard / Bind / SpecRead from posteriors and writer knowledge.
pub(crate) fn choose_action(ctx: PolicyCtx) -> ResolveAction {
    if ctx.writer_known && ctx.posterior_conflict >= TAU_W {
        if let Some(v) = ctx.bind_version {
            return ResolveAction::Bind(v);
        }
        return ResolveAction::WaitHard;
    }
    if ctx.placeholder_ready {
        if let Some(v) = ctx.bind_version {
            return ResolveAction::Bind(v);
        }
        // Residual write-set predicts ℓ but value not published yet → WaitHard.
        if ctx.writer_known {
            return ResolveAction::WaitHard;
        }
    }
    if ctx.posterior_conflict >= TAU_S {
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
