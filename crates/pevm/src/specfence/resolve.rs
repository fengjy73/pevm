//! Resolution algebra: WaitHard / Bind / SpecRead (SpecFence control law v3).
//!
//! v3 (frozen from plant-measured effect RAW):
//! 1. Bind if producer Data published.
//! 2. Else if program && (fanout_hint || d large): WaitHard+park.
//! 3. Else if handler / short-lag: SpecRead.
//! 4. EarlyAbort only when heavy-tx morphology && d small (~0.1) — optional research.
//! 5. Long WAW spine (handler multi-writer): SpecRead / schedule — not WaitHard.
//! HotSet / writer counts = fanout_hint only, not a hard Wait gate.

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

/// Context features for π (control law v3).
#[derive(Debug, Clone)]
pub(crate) struct PolicyCtx {
    pub location: MemoryLocationHash,
    pub writer_known: bool,
    pub writer: Option<TxIdx>,
    /// True when last writer is Executed/Validated (wait is cheap / Bindable).
    pub writer_done: bool,
    pub posterior_conflict: f64,
    pub posterior_bind_success: f64,
    pub placeholder_ready: bool,
    pub bind_version: Option<TxVersion>,
    /// M3: residual / process WŜ predicts a lower writer on this location.
    pub prior_ws_predicts: bool,
    /// Program (storage/code/selfdestruct) vs handler (basic/lazy).
    pub is_program: bool,
    /// HotSet / high writer-count morphology hint — NOT a hard gate.
    pub fanout_hint: bool,
    /// Gross-work depth d = gas_used_so_far/tx_gas_used when known (inspect/research).
    pub gross_work_depth: Option<f64>,
}

/// Legacy Spec v1 §7.3 thresholds (kept for revoke / seed docs).
pub(crate) const TAU_W: f64 = 0.35;
pub(crate) const TAU_S: f64 = 0.50;
pub(crate) const TAU_REVOKE: f64 = 0.20;

/// Partial-retry reexec factor in expected SpecRead cost (`1 + P * C_retry`).
pub(crate) const C_RETRY: f64 = 3.0;
/// Wait must be clearly cheaper than Spec (`cost_wait < cost_spec * margin`).
pub(crate) const COST_MARGIN: f64 = 0.40;
/// Safety valve: always WaitHard when conflict posterior is very high (program only).
pub(crate) const TAU_VERY_HIGH: f64 = 0.75;
/// Gross-work depth above which Wait beats EarlyAbort on fan-out program edges.
pub(crate) const D_WAIT: f64 = 0.50;
/// EarlyAbort only when d is small (heavy-tx minority) — research path.
pub(crate) const D_EARLY: f64 = 0.15;

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

/// True when cost model prefers WaitHard over SpecRead (legacy helper / safety).
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

/// Control law v3 π: Bind → WaitHard (program fanout / late d) → SpecRead.
/// HotSet is `fanout_hint` only. Handler / WAW-spine paths prefer SpecRead.
pub(crate) fn choose_action(ctx: PolicyCtx) -> ResolveAction {
    // 1. Bind when a concrete published version is ready.
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

    let d = ctx.gross_work_depth;
    let d_large = d.map(|x| x >= D_WAIT).unwrap_or(false);
    // Without inspect depth, fanout_hint stands in for "late discovery likely" on hot program locs.
    let want_wait = ctx.is_program
        && ctx.writer_known
        && !ctx.writer_done
        && (ctx.fanout_hint || d_large || ctx.posterior_conflict >= TAU_VERY_HIGH);

    // 2. Program + (fanout / late d / very-high P) → WaitHard+park.
    if want_wait {
        // EarlyAbort research niche: heavy-tx small d — still WaitHard here is safer
        // without a true abort API on this path; SpecRead would waste more when d tiny
        // only if we could drop the incarnation cheaply (not available without rem arm).
        let _early = d.map(|x| x <= D_EARLY).unwrap_or(false) && ctx.fanout_hint;
        let _ = _early;
        return ResolveAction::WaitHard;
    }

    // 3. Handler / short-lag / no fanout → SpecRead (WAW spine: schedule/steal ≫ WaitHard).
    // 4. Cost safety: program + very high P even without fanout hint.
    if ctx.is_program
        && ctx.writer_known
        && cost_prefers_wait(ctx.writer_known, ctx.writer_done, ctx.posterior_conflict)
    {
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

    fn ctx_v3(
        p: f64,
        writer_known: bool,
        writer_done: bool,
        bind: Option<TxVersion>,
        placeholder_ready: bool,
        is_program: bool,
        fanout_hint: bool,
        d: Option<f64>,
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
            prior_ws_predicts: false,
            is_program,
            fanout_hint,
            gross_work_depth: d,
        }
    }

    #[test]
    fn v3_bind_when_data_ready() {
        let v = TxVersion {
            tx_idx: 0,
            tx_incarnation: 0,
        };
        let a = choose_action(ctx_v3(0.2, true, true, Some(v.clone()), true, true, true, Some(0.9)));
        assert_eq!(a, ResolveAction::Bind(v));
    }

    #[test]
    fn v3_program_fanout_waits() {
        let a = choose_action(ctx_v3(0.3, true, false, None, false, true, true, Some(0.9)));
        assert_eq!(a, ResolveAction::WaitHard);
    }

    #[test]
    fn v3_handler_specs_even_if_fanout() {
        // WAW spine / handler chatter: SpecRead, not WaitHard on writer-count.
        let a = choose_action(ctx_v3(0.3, true, false, None, false, false, true, None));
        assert_eq!(a, ResolveAction::SpecRead);
    }

    #[test]
    fn v3_program_no_fanout_specs() {
        let a = choose_action(ctx_v3(0.2, true, false, None, false, true, false, None));
        assert_eq!(a, ResolveAction::SpecRead);
    }

    #[test]
    fn v3_program_late_depth_waits_without_fanout() {
        let a = choose_action(ctx_v3(0.2, true, false, None, false, true, false, Some(0.94)));
        assert_eq!(a, ResolveAction::WaitHard);
    }

    #[test]
    fn v3_safety_valve_very_high_p_program() {
        let a = choose_action(ctx_v3(0.80, true, false, None, false, true, false, None));
        assert_eq!(a, ResolveAction::WaitHard);
    }

    #[test]
    fn m3_prior_ws_binds_when_version_ready() {
        let v = TxVersion {
            tx_idx: 0,
            tx_incarnation: 0,
        };
        let mut c = ctx_v3(0.2, true, true, Some(v.clone()), true, true, false, None);
        c.prior_ws_predicts = true;
        assert_eq!(choose_action(c), ResolveAction::Bind(v));
    }

    #[test]
    fn cost_constants_raise_wait_bar() {
        assert!(COST_MARGIN < 1.0);
        assert!(C_RETRY >= 2.0 && C_RETRY <= 4.0);
        assert!((TAU_VERY_HIGH - 0.75).abs() < f64::EPSILON);
    }
}
