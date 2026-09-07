//! Resolution algebra: WaitHard / Bind / SpecRead / EarlyAbort (control law v3 + P3).
//!
//! v3 (frozen from plant-measured effect RAW) + P0/P1/P3 hooks:
//! 1. Bind if producer Data published.
//! 2. Else if program && (fanout_hint || d large) && !waw_spine_hint: WaitHard+park
//!    — unless EarlyAbort niche (heavy ∧ d≤D_EARLY ∧ known d ∧ unresolved producer).
//! 3. Else if handler / short-lag / WAW spine: SpecRead.
//! 4. EarlyAbort (P3): cut incarnation at first-cross; rem RewindTo/FullRetry + Blocking.
//! 5. HotSet / writer counts = fanout_hint only, not a hard Wait gate.
//! Constants live in [`AdaptiveParams`] (also re-exported as module consts).
//!
//! **Depth rule:** EarlyAbort only when `gross_work_depth` is `Some` (known d).
//! No gas_limit proxy — `used/limit` underestimates true `used/tx_gas_used` and can
//! false-positive EarlyAbort. LeanOCC (no inspect) keeps WaitHard/SpecRead.

#![allow(dead_code)]
use crate::{MemoryLocationHash, TxIdx, TxIncarnation, TxVersion};

use super::learner::{AdaptiveParams, MorphWeights};

/// Policy action chosen by π for one region read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ResolveAction {
    /// Block until last writer `< t` has non-ESTIMATE Data (or aborted).
    WaitHard,
    /// Read exact version `v=(t_w,inc)` once published; `t_w < t`.
    Bind(TxVersion),
    /// OrderedDirtyRead: last Data `< t` skipping ESTIMATE (else Wait).
    SpecRead,
    /// P3: cut incarnation at early bad program cross (heavy ∧ d≤D_EARLY).
    /// VM arms rem RewindTo/FullRetry + Blocking on unresolved producer (hang-free).
    /// Does **not** SoftWait-arm (alternate fence to WaitHard).
    EarlyAbort,
}

/// Context features for π (control law v3 + P1 morph/waw/heavy).
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
    /// HotSet / high writer-count / live fanout morphology hint — NOT a hard gate.
    pub fanout_hint: bool,
    /// Gross-work depth d = gas_used_so_far/tx_gas_used when known (inspect/research).
    pub gross_work_depth: Option<f64>,
    /// P1: morphology posterior weights (or summary).
    pub morph_weights: MorphWeights,
    /// P1: WAW-heavy / multi-writer without RAW useful → suppress WaitHard.
    pub waw_spine_hint: bool,
    /// P1: gas band / prior says heavy tx (EarlyAbort niche for P3).
    pub tx_heavy_hint: bool,
    /// Tunable π constants.
    pub params: AdaptiveParams,
}

/// Legacy Spec v1 §7.3 thresholds (kept for revoke / seed docs / Bayes).
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
/// EarlyAbort only when d is small (heavy-tx minority) — research path / P3.
pub(crate) const D_EARLY: f64 = 0.15;

/// Estimated producer remaining work: 0 if published/done, else 1.0 unit.
#[inline]
pub(crate) fn cost_wait(writer_done: bool) -> f64 {
    if writer_done { 0.0 } else { 1.0 }
}

/// Expected SpecRead cost: base progress + conflict-weighted retry.
#[inline]
pub(crate) fn cost_spec(p_conflict: f64) -> f64 {
    cost_spec_params(p_conflict, C_RETRY)
}

#[inline]
pub(crate) fn cost_spec_params(p_conflict: f64, c_retry: f64) -> f64 {
    1.0 + p_conflict.clamp(0.0, 1.0) * c_retry
}

/// True when cost model prefers WaitHard over SpecRead (legacy helper / safety).
#[inline]
pub(crate) fn cost_prefers_wait(
    writer_known: bool,
    writer_done: bool,
    p_conflict: f64,
) -> bool {
    cost_prefers_wait_params(
        writer_known,
        writer_done,
        p_conflict,
        &AdaptiveParams::default(),
    )
}

#[inline]
pub(crate) fn cost_prefers_wait_params(
    writer_known: bool,
    writer_done: bool,
    p_conflict: f64,
    params: &AdaptiveParams,
) -> bool {
    let p = p_conflict.clamp(0.0, 1.0);
    if writer_known
        && cost_wait(writer_done) < cost_spec_params(p, params.c_retry) * params.cost_margin
    {
        return true;
    }
    p >= params.tau_very_high
}

/// P3: EarlyAbort niche — heavy ∧ program ∧ known d≤D_EARLY ∧ unresolved producer.
/// Returns false when `gross_work_depth` is None (no guess / no gas_limit proxy).
#[inline]
pub(crate) fn early_abort_candidate(ctx: &PolicyCtx) -> bool {
    let d = ctx.gross_work_depth;
    ctx.tx_heavy_hint
        && ctx.is_program
        && ctx.writer_known
        && !ctx.writer_done
        && d.map(|x| x <= ctx.params.d_early).unwrap_or(false)
}

/// Control law v3 π: Bind → WaitHard|EarlyAbort (program fanout / late d, !waw) → SpecRead.
/// HotSet is `fanout_hint` only. Handler / WAW-spine paths prefer SpecRead.
pub(crate) fn choose_action(ctx: PolicyCtx) -> ResolveAction {
    let params = ctx.params;
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
    let d_large = d.map(|x| x >= params.d_wait).unwrap_or(false);
    // Without inspect depth, fanout_hint stands in for "late discovery likely" on hot program locs.
    let want_wait = ctx.is_program
        && ctx.writer_known
        && !ctx.writer_done
        && !ctx.waw_spine_hint
        && (ctx.fanout_hint || d_large || ctx.posterior_conflict >= params.tau_very_high);

    // 2. Program + (fanout / late d / very-high P) ∧ !waw_spine → WaitHard+park
    //    or EarlyAbort (P3 heavy ∧ known d≤D_EARLY).
    if want_wait {
        if early_abort_candidate(&ctx) {
            return ResolveAction::EarlyAbort;
        }
        return ResolveAction::WaitHard;
    }

    // 3. Handler / short-lag / WAW spine / no fanout → SpecRead.
    // 4. Cost safety: program + very high P even without fanout hint (still suppress on WAW).
    if ctx.is_program
        && ctx.writer_known
        && !ctx.waw_spine_hint
        && cost_prefers_wait_params(
            ctx.writer_known,
            ctx.writer_done,
            ctx.posterior_conflict,
            &params,
        )
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
            morph_weights: MorphWeights::default(),
            waw_spine_hint: false,
            tx_heavy_hint: false,
            params: AdaptiveParams::default(),
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
        let p = AdaptiveParams::default();
        assert!((p.d_wait - D_WAIT).abs() < f64::EPSILON);
        assert!((p.d_early - D_EARLY).abs() < f64::EPSILON);
        assert!((p.tau_revoke - TAU_REVOKE).abs() < f64::EPSILON);
    }

    // --- P0/P1 extended matrix ---

    #[test]
    fn program_fanout_wait_via_pi_not_hotset_gate() {
        // HotSet absent is represented as fanout_hint=false; with high P still Wait via π.
        let a = choose_action(ctx_v3(0.80, true, false, None, false, true, false, None));
        assert_eq!(a, ResolveAction::WaitHard);
        // With fanout_hint (HotSet member) and moderate P → Wait.
        let a2 = choose_action(ctx_v3(0.3, true, false, None, false, true, true, None));
        assert_eq!(a2, ResolveAction::WaitHard);
    }

    #[test]
    fn hotset_absent_still_specread_or_wait_via_pi() {
        // Cold ℓ, low P, no fanout → SpecRead (π, not HotSet gate).
        let a = choose_action(ctx_v3(0.15, true, false, None, false, true, false, None));
        assert_eq!(a, ResolveAction::SpecRead);
        // Cold ℓ, late depth → Wait via π without HotSet.
        let a2 = choose_action(ctx_v3(0.15, true, false, None, false, true, false, Some(0.9)));
        assert_eq!(a2, ResolveAction::WaitHard);
    }

    #[test]
    fn handler_specread() {
        let a = choose_action(ctx_v3(0.5, true, false, None, false, false, true, Some(0.9)));
        assert_eq!(a, ResolveAction::SpecRead);
    }

    #[test]
    fn waw_spine_suppresses_waithard() {
        let mut c = ctx_v3(0.3, true, false, None, false, true, true, Some(0.9));
        c.waw_spine_hint = true;
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
        // Even very-high P safety path is suppressed on WAW spine.
        let mut c2 = ctx_v3(0.90, true, false, None, false, true, true, None);
        c2.waw_spine_hint = true;
        assert_eq!(choose_action(c2), ResolveAction::SpecRead);
    }

    #[test]
    fn bind_when_ready() {
        let v = TxVersion {
            tx_idx: 1,
            tx_incarnation: 0,
        };
        let a = choose_action(ctx_v3(
            0.4,
            true,
            true,
            Some(v.clone()),
            true,
            true,
            true,
            None,
        ));
        assert_eq!(a, ResolveAction::Bind(v));
    }

    #[test]
    fn early_abort_candidate_arms_early_abort() {
        let mut c = ctx_v3(0.3, true, false, None, false, true, true, Some(0.10));
        c.tx_heavy_hint = true;
        assert!(early_abort_candidate(&c));
        assert_eq!(choose_action(c), ResolveAction::EarlyAbort);
    }

    #[test]
    fn early_abort_requires_known_depth() {
        // No inspect → d=None → never EarlyAbort (keep WaitHard).
        let mut c = ctx_v3(0.3, true, false, None, false, true, true, None);
        c.tx_heavy_hint = true;
        assert!(!early_abort_candidate(&c));
        assert_eq!(choose_action(c), ResolveAction::WaitHard);
    }

    #[test]
    fn early_abort_not_when_late_depth() {
        let mut c = ctx_v3(0.3, true, false, None, false, true, true, Some(0.90));
        c.tx_heavy_hint = true;
        assert!(!early_abort_candidate(&c));
        assert_eq!(choose_action(c), ResolveAction::WaitHard);
    }

    #[test]
    fn early_abort_not_when_not_heavy() {
        let mut c = ctx_v3(0.3, true, false, None, false, true, true, Some(0.10));
        c.tx_heavy_hint = false;
        assert!(!early_abort_candidate(&c));
        assert_eq!(choose_action(c), ResolveAction::WaitHard);
    }

    #[test]
    fn early_abort_not_when_writer_done() {
        let mut c = ctx_v3(0.3, true, true, None, false, true, true, Some(0.10));
        c.tx_heavy_hint = true;
        // writer_done → not EarlyAbort niche (producer resolved).
        assert!(!early_abort_candidate(&c));
        assert_ne!(choose_action(c), ResolveAction::EarlyAbort);
    }


    #[test]
    fn morph_quiet_does_not_force_wait() {
        let mut c = ctx_v3(0.2, true, false, None, false, true, false, None);
        c.morph_weights = MorphWeights {
            fan_out: 0.05,
            mixed: 0.10,
            waw_spine: 0.05,
            quiet: 0.80,
        };
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
    }
}
