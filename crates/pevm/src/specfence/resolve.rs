//! Adaptive EV Controller (AEC) π: Bind / WaitHard / SpecRead / EarlyAbort.
//!
//! Sole decision = argmin EV over FenceGraph actions (makespan-relevant):
//! ```text
//! EV_Bind  = 0 if Data ready (published version) else +∞
//! EV_Wait  = E_wait_time(ℓ)*(1+α*fanout) + δ*max(0,E_idle−prior)/(1+fanout)
//! EV_Spec  = P_abort(ℓ,x) * (W_remain(d) + β * E_cascade + γ * E_reexec)
//! EV_Early = W_prefix(d) + E_reexec   # only if d known & heavy features
//! pick argmin; ties → SpecRead (OCC-like default)
//! ```
//! V5-P2 θ: measured wake latency → E_wait_time; cascade → E_cascade;
//! idle-steal → E_idle_steal; lean ForceBind/FullRestart → E_reexec.
//! Meta tax: ρ hard Spec + tiny-gap Spec bias (no SoftWait storm).
//! Bind is aggressive when Data is published / WŜ predicts a ready version —
//! still EV (not Boolean Wait ladders).
//!
//! Features (never Boolean Wait gates): HotSet, H_w/H_a, morph, live fanout, d,
//! heavy hint, WAW ratio. Removed ladders: `if fanout_hint: WaitHard`,
//! `if d≥D_WAIT: WaitHard`, morph `d:=0.9` Wait force, `P≥τ` Wait force.
//!
//! EarlyAbort only when measured/proxy `d` is Some (no morph-forced EarlyAbort).
//! Learning ∉ TCB. Constants live in [`AdaptiveParams`] as rates/priors.

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
    /// EarlyAbort: cut incarnation at early bad program cross (known d + heavy).
    /// VM arms rem RewindTo/FullRetry + Blocking on unresolved producer (hang-free).
    /// Does **not** SoftWait-arm (alternate fence to WaitHard).
    EarlyAbort,
}

/// Context features for AEC π (continuous EV; no Boolean Wait ladders).
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
    /// HotSet / high writer-count morphology **feature** — NOT a hard Wait gate.
    pub fanout_hint: bool,
    /// Live reader fanout at ℓ (continuous feature for EV_Wait).
    pub live_fanout: f64,
    /// Learned / prior E_wait_time(ℓ) in normalized work units (wake latency EMA).
    pub e_wait_time: f64,
    /// Learned / prior E_cascade(ℓ).
    pub e_cascade: f64,
    /// Learned / prior abort→reexec cost (feeds EV_Spec + EV_Early).
    pub e_reexec: f64,
    /// Idle-steal tax: park without steal raises EV_Wait (cores idle).
    pub e_idle_steal: f64,
    /// Continuous meta_ops/useful (0 before warmup) — tiny-gap Spec bias.
    pub meta_tax: f64,
    /// Meta budget exceeded → force SpecRead (OCC fallback).
    pub meta_budget_exceeded: bool,
    /// Known depth only: gross-work / effect-progress proxy.
    /// Morph late prior feeds **W_remain feature only** — not EarlyAbort, not Wait gate.
    pub gross_work_depth: Option<f64>,
    /// Morphology posterior weights (features).
    pub morph_weights: MorphWeights,
    /// WAW-heavy / multi-writer without RAW useful → suppress Wait (structure).
    pub waw_spine_hint: bool,
    /// Gas band / prior says heavy tx (EarlyAbort niche feature).
    pub tx_heavy_hint: bool,
    /// Learning rates / priors (not decision cuts).
    pub params: AdaptiveParams,
}

/// Legacy Spec v1 §7.3 thresholds (kept for revoke / seed docs / Bayes).
pub(crate) const TAU_W: f64 = 0.35;
pub(crate) const TAU_S: f64 = 0.50;
pub(crate) const TAU_REVOKE: f64 = 0.20;

/// Partial-retry reexec factor (legacy SpecRead cost helper / docs).
pub(crate) const C_RETRY: f64 = 3.0;
/// DEPRECATED — not a Wait gate in AEC.
pub(crate) const COST_MARGIN: f64 = 0.40;
/// Bind escalation with published Data (not a Wait gate).
pub(crate) const TAU_VERY_HIGH: f64 = 0.75;
/// DEPRECATED — not a Wait gate in AEC (feature demotion only).
pub(crate) const D_WAIT: f64 = 0.50;
/// EarlyAbort niche: known d ≤ D_EARLY (with heavy features + EV).
pub(crate) const D_EARLY: f64 = 0.15;

/// Estimated producer remaining work: 0 if published/done, else 1.0 unit.
#[inline]
pub(crate) fn cost_wait(writer_done: bool) -> f64 {
    if writer_done { 0.0 } else { 1.0 }
}

/// Expected SpecRead cost: base progress + conflict-weighted retry (legacy helper).
#[inline]
pub(crate) fn cost_spec(p_conflict: f64) -> f64 {
    cost_spec_params(p_conflict, C_RETRY)
}

#[inline]
pub(crate) fn cost_spec_params(p_conflict: f64, c_retry: f64) -> f64 {
    1.0 + p_conflict.clamp(0.0, 1.0) * c_retry
}

/// Legacy helper retained for Bayes `should_wait_hard` facade / docs.
/// AEC π does **not** use this as a Boolean Wait ladder.
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

/// EarlyAbort niche — heavy ∧ program ∧ known d≤D_EARLY ∧ unresolved producer.
/// Returns false when `gross_work_depth` is None (no morph-forced EarlyAbort).
#[inline]
pub(crate) fn early_abort_candidate(ctx: &PolicyCtx) -> bool {
    let d = ctx.gross_work_depth;
    ctx.tx_heavy_hint
        && ctx.is_program
        && ctx.writer_known
        && !ctx.writer_done
        && d.map(|x| x <= ctx.params.d_early).unwrap_or(false)
}

/// Remaining work if SpecRead aborts (continuous). Measured d preferred;
/// morph late prior is a **feature** for W_remain only (never Boolean Wait).
#[inline]
fn w_remain(ctx: &PolicyCtx) -> f64 {
    let depth = ctx
        .gross_work_depth
        .or_else(|| ctx.morph_weights.wait_depth_prior());
    match depth {
        Some(d) => (1.0 - d.clamp(0.0, 1.0)).max(0.05),
        None => 1.0,
    }
}

/// Continuous fanout feature for EV_Wait. HotSet membership is a mild bump only.
#[inline]
fn fanout_feature(ctx: &PolicyCtx) -> f64 {
    let mut f = ctx.live_fanout.max(0.0);
    if ctx.fanout_hint {
        f = f.max(1.0);
    }
    f
}

/// Compute EV vector for debugging / tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EvScores {
    pub ev_wait: f64,
    pub ev_spec: f64,
    pub ev_early: f64,
}

/// AEC EV estimates (finite actions only; Bind handled separately).
///
/// V5-P2: θ-driven continuous costs only — no Boolean Wait gates.
pub(crate) fn compute_ev(ctx: &PolicyCtx) -> EvScores {
    let params = &ctx.params;
    let fanout = fanout_feature(ctx);

    // EV_Wait: wake latency × fanout tax + idle-steal tax.
    // HIGH fanout raises cost (discourage serialize); idle park raises Wait.
    let ev_wait = if !ctx.is_program || ctx.waw_spine_hint || !ctx.writer_known {
        f64::INFINITY
    } else if ctx.writer_done {
        0.0
    } else {
        let e_wait = ctx.e_wait_time.max(params.e_wait_prior * 0.05);
        // Idle excess above prior; scale by 1/(1+fanout) so high-fanout
        // serialize tax (α) is not double-counted with worker park.
        let idle_excess = (ctx.e_idle_steal - params.e_idle_prior).max(0.0);
        e_wait * (1.0 + params.alpha_fanout * fanout)
            + params.delta_idle * idle_excess / (1.0 + fanout)
    };

    let p_abort = ctx.posterior_conflict.clamp(0.0, 1.0);
    let e_cascade = ctx.e_cascade.max(0.0);
    // Measured abort→reexec (V5-P1 ForceBind≈1.2 / FullRestart≈2.2) via γ.
    let e_reexec = ctx.e_reexec.max(params.e_reexec * 0.25).max(0.0);
    let ev_spec = p_abort
        * (w_remain(ctx)
            + params.beta_cascade * e_cascade
            + params.gamma_reexec * e_reexec);

    let ev_early = if early_abort_candidate(ctx) {
        let w_prefix = ctx.gross_work_depth.unwrap_or(0.0).clamp(0.0, 1.0);
        w_prefix + e_reexec
    } else {
        f64::INFINITY
    };

    EvScores {
        ev_wait,
        ev_spec,
        ev_early,
    }
}

/// Tiny-gap meta bias: Wait barely beats Spec under meta pressure → SpecRead.
#[inline]
fn meta_tax_prefers_spec(ev_wait: f64, ev_spec: f64, meta_tax: f64, params: &AdaptiveParams) -> bool {
    if !ev_wait.is_finite() || !ev_spec.is_finite() {
        return false;
    }
    // Only when measured meta tax is present — cold start matches pre-P2 π.
    if meta_tax <= f64::EPSILON {
        return false;
    }
    let gap = ev_spec - ev_wait; // >0 ⇒ Wait currently cheaper
    if gap <= 0.0 {
        return false;
    }
    let thresh = params.meta_gap_eps * (1.0 + meta_tax);
    gap < thresh
}

/// AEC π: Bind if Data ready; else argmin EV; ties → SpecRead; meta budget → SpecRead.
pub(crate) fn choose_action(ctx: PolicyCtx) -> ResolveAction {
    let params = ctx.params;
    // 1. Raise Bind hits when published Data is hang-free to consume (writer_done)
    // or WŜ/placeholder/high bind posterior predicts the ready version.
    // Do NOT Bind solely on bind_version when !writer_done — that SoftWait-arms and
    // can livelock fan-out (M1k/M1l). Unresolved Data still enters EV below.
    if let Some(v) = ctx.bind_version.clone() {
        if ctx.writer_done
            || ctx.placeholder_ready
            || ctx.prior_ws_predicts
            || ctx.posterior_conflict >= params.tau_very_high
            || ctx.posterior_bind_success >= params.tau_s
        {
            return ResolveAction::Bind(v);
        }
    }

    // Phase C: measured meta tax → OCC SpecRead fallback.
    if ctx.meta_budget_exceeded {
        return ResolveAction::SpecRead;
    }

    let mut ev = compute_ev(&ctx);
    // Data published but writer not yet done: EV_Bind ≈ small wake cost (not +∞).
    // Prefer Bind (may SoftWait) only when that beats Spec — still argmin EV.
    if let Some(v) = ctx.bind_version.clone() {
        let ev_bind = ctx.e_wait_time.max(params.e_wait_prior * 0.05)
            * (1.0 + params.alpha_fanout * fanout_feature(&ctx) * 0.25);
        if ctx.prior_ws_predicts || ctx.placeholder_ready {
            ev.ev_spec *= 1.0 + ctx.posterior_bind_success.clamp(0.0, 1.0);
        }
        const EPS: f64 = 1e-9;
        let mut best = ResolveAction::SpecRead;
        let mut best_ev = ev.ev_spec;
        if ev_bind + EPS < best_ev {
            best = ResolveAction::Bind(v);
            best_ev = ev_bind;
        }
        if ev.ev_wait + EPS < best_ev
            && !meta_tax_prefers_spec(ev.ev_wait, ev.ev_spec, ctx.meta_tax, &params)
        {
            best = ResolveAction::WaitHard;
            best_ev = ev.ev_wait;
        }
        if ev.ev_early + EPS < best_ev {
            best = ResolveAction::EarlyAbort;
        }
        return best;
    }

    // When WŜ predicts a writer but Data not yet published, raise Spec abort cost.
    if ctx.prior_ws_predicts || ctx.placeholder_ready {
        ev.ev_spec *= 1.0 + ctx.posterior_bind_success.clamp(0.0, 1.0);
    }

    // Argmin; strict improvement only — ties keep SpecRead (OCC-like default).
    // V5-P2: measured meta tax biases Spec on tiny Wait wins (no SoftWait storm).
    const EPS: f64 = 1e-9;
    let mut best = ResolveAction::SpecRead;
    let mut best_ev = ev.ev_spec;

    if ev.ev_wait + EPS < best_ev
        && !meta_tax_prefers_spec(ev.ev_wait, ev.ev_spec, ctx.meta_tax, &params)
    {
        best = ResolveAction::WaitHard;
        best_ev = ev.ev_wait;
    }
    if ev.ev_early + EPS < best_ev {
        best = ResolveAction::EarlyAbort;
    }

    best
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

    fn ctx_aec(
        p: f64,
        writer_known: bool,
        writer_done: bool,
        bind: Option<TxVersion>,
        placeholder_ready: bool,
        is_program: bool,
        fanout_hint: bool,
        live_fanout: f64,
        d: Option<f64>,
    ) -> PolicyCtx {
        let params = AdaptiveParams::default();
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
            live_fanout,
            e_wait_time: params.e_wait_prior,
            e_cascade: params.e_cascade_prior,
            e_reexec: params.e_reexec,
            e_idle_steal: params.e_idle_prior,
            meta_tax: 0.0,
            meta_budget_exceeded: false,
            gross_work_depth: d,
            morph_weights: MorphWeights::default(),
            waw_spine_hint: false,
            tx_heavy_hint: false,
            params,
        }
    }

    #[test]
    fn aec_bind_when_data_ready() {
        let v = TxVersion {
            tx_idx: 0,
            tx_incarnation: 0,
        };
        let a = choose_action(ctx_aec(
            0.2, true, true, Some(v.clone()), true, true, true, 16.0, Some(0.9),
        ));
        assert_eq!(a, ResolveAction::Bind(v));
    }

    #[test]
    fn aec_high_fanout_prefers_spec_over_wait_when_e_wait_high() {
        // Smoking gun 597: high fanout must raise EV_Wait → SpecRead.
        let mut c = ctx_aec(0.35, true, false, None, false, true, true, 16.0, Some(0.9));
        c.e_wait_time = 1.0;
        let ev = compute_ev(&c);
        assert!(
            ev.ev_wait > ev.ev_spec,
            "EV_Wait={:?} should exceed EV_Spec={:?} at high fanout",
            ev.ev_wait,
            ev.ev_spec
        );
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
    }

    #[test]
    fn aec_fanout_hint_is_not_boolean_wait() {
        // fanout_hint alone must NOT force WaitHard (feature only).
        // High E_wait + hint without live fanout → Spec (EV), never Boolean Wait.
        let mut c = ctx_aec(0.25, true, false, None, false, true, true, 0.0, None);
        c.e_wait_time = 2.0;
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
        // Hint + moderate P + high fanout → Spec because fanout raises EV_Wait.
        let mut c2 = ctx_aec(0.40, true, false, None, false, true, true, 16.0, None);
        c2.e_wait_time = 1.0;
        assert_eq!(choose_action(c2), ResolveAction::SpecRead);
    }

    #[test]
    fn aec_no_d_wait_boolean_ladder() {
        // Late measured d must NOT Boolean-force WaitHard.
        let a = choose_action(ctx_aec(0.20, true, false, None, false, true, false, 2.0, Some(0.94)));
        assert_eq!(a, ResolveAction::SpecRead);
    }

    #[test]
    fn aec_low_fanout_high_p_can_wait() {
        // Wait wins when fanout low, E_wait modest, P_abort high.
        let mut c = ctx_aec(0.90, true, false, None, false, true, false, 1.0, Some(0.2));
        c.e_wait_time = 0.3;
        c.e_cascade = 2.0;
        let ev = compute_ev(&c);
        assert!(
            ev.ev_wait < ev.ev_spec,
            "EV_Wait={:?} should beat EV_Spec={:?}",
            ev.ev_wait,
            ev.ev_spec
        );
        assert_eq!(choose_action(c), ResolveAction::WaitHard);
    }

    #[test]
    fn aec_handler_specs_even_if_fanout() {
        let a = choose_action(ctx_aec(0.9, true, false, None, false, false, true, 32.0, None));
        assert_eq!(a, ResolveAction::SpecRead);
    }

    #[test]
    fn aec_tie_defaults_to_specread() {
        let mut c = ctx_aec(0.5, true, false, None, false, true, false, 0.0, None);
        // EV_Spec = 0.5 * (1 + β*1 + γ*1.5) = 1.375 with γ=0.5; set EV_Wait equal → tie → SpecRead
        // EV_Wait also adds δ*E_idle — zero idle for exact tie.
        c.e_idle_steal = 0.0;
        c.e_wait_time = 1.375;
        c.posterior_conflict = 0.5;
        c.e_cascade = 1.0;
        c.e_reexec = 1.5;
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
    }

    #[test]
    fn aec_early_abort_only_with_known_early_d() {
        let mut c = ctx_aec(0.8, true, false, None, false, true, false, 1.0, None);
        c.tx_heavy_hint = true;
        c.e_wait_time = 5.0; // Wait expensive
        c.e_cascade = 5.0; // Spec expensive
        assert!(!early_abort_candidate(&c));
        assert_ne!(choose_action(c.clone()), ResolveAction::EarlyAbort);

        c.gross_work_depth = Some(0.10);
        assert!(early_abort_candidate(&c));
        // EV_Early = 0.1 + 1.5 = 1.6; may or may not win vs Spec — require known d path.
        let a = choose_action(c.clone());
        assert!(
            matches!(a, ResolveAction::EarlyAbort | ResolveAction::SpecRead | ResolveAction::WaitHard),
            "{a:?}"
        );
        // With extreme Spec/Wait cost, EarlyAbort wins.
        c.posterior_conflict = 0.99;
        c.e_cascade = 10.0;
        c.e_wait_time = 10.0;
        assert_eq!(choose_action(c), ResolveAction::EarlyAbort);
    }

    #[test]
    fn aec_morph_fan_out_does_not_force_wait() {
        let mut c = ctx_aec(0.25, true, false, None, false, true, false, 4.0, None);
        c.morph_weights = MorphWeights {
            fan_out: 0.55,
            mixed: 0.25,
            waw_spine: 0.10,
            quiet: 0.10,
        };
        // Morph late prior shrinks W_remain → Spec cheaper; must NOT Wait-force.
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
    }

    #[test]
    fn aec_meta_budget_forces_specread() {
        let mut c = ctx_aec(0.95, true, false, None, false, true, false, 1.0, Some(0.2));
        c.e_wait_time = 0.1;
        c.meta_budget_exceeded = true;
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
    }

    #[test]
    fn aec_waw_spine_suppresses_wait() {
        let mut c = ctx_aec(0.9, true, false, None, false, true, true, 2.0, Some(0.2));
        c.waw_spine_hint = true;
        c.e_wait_time = 0.1;
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
    }

    #[test]
    fn aec_bind_prior_ws_or_high_p() {
        let v = TxVersion {
            tx_idx: 0,
            tx_incarnation: 0,
        };
        let mut c = ctx_aec(0.2, true, false, Some(v.clone()), false, true, false, 8.0, None);
        c.prior_ws_predicts = true;
        assert_eq!(choose_action(c), ResolveAction::Bind(v.clone()));
        let c2 = ctx_aec(0.80, true, false, Some(v.clone()), false, true, false, 8.0, None);
        assert_eq!(choose_action(c2), ResolveAction::Bind(v));
    }

    #[test]
    fn aec_bind_when_data_and_writer_done_or_quality() {
        let v = TxVersion {
            tx_idx: 1,
            tx_incarnation: 0,
        };
        // writer_done → Bind (EV_Bind=0, hang-free).
        let c = ctx_aec(0.05, true, true, Some(v.clone()), false, true, true, 32.0, None);
        assert_eq!(choose_action(c), ResolveAction::Bind(v.clone()));
        // High bind posterior + Data → Bind (quality signal).
        let mut c2 = ctx_aec(0.05, true, false, Some(v.clone()), false, true, false, 2.0, None);
        c2.posterior_bind_success = 0.80;
        assert_eq!(choose_action(c2), ResolveAction::Bind(v.clone()));
        // Data alone at high fanout without quality → Spec (no SoftWait storm).
        let mut c3 = ctx_aec(0.05, true, false, Some(v.clone()), false, true, true, 32.0, None);
        c3.e_wait_time = 2.0;
        c3.e_reexec = 0.5;
        assert_eq!(choose_action(c3), ResolveAction::SpecRead);
    }

    #[test]
    fn aec_ev_spec_includes_reexec_so_cheap_wake_can_wait() {
        // Without E_reexec, high-P low-fanout might still Spec; with reexec, Wait wins.
        let mut c = ctx_aec(0.85, true, false, None, false, true, false, 1.0, Some(0.3));
        c.e_wait_time = 0.4;
        c.e_cascade = 0.5;
        c.e_reexec = 2.5; // measured FullRestart-ish abort cost
        let ev = compute_ev(&c);
        assert!(
            ev.ev_wait < ev.ev_spec,
            "EV_Wait={:?} should beat EV_Spec={:?} when reexec is priced",
            ev.ev_wait,
            ev.ev_spec
        );
        assert_eq!(choose_action(c), ResolveAction::WaitHard);
    }

    #[test]
    fn aec_prior_ws_raises_spec_cost_without_boolean_wait() {
        let mut c = ctx_aec(0.30, true, false, None, false, true, false, 8.0, None);
        c.e_wait_time = 3.0; // Wait expensive at fanout
        c.e_cascade = 1.0;
        c.e_reexec = 1.5;
        c.prior_ws_predicts = true;
        c.posterior_bind_success = 0.8;
        // High fanout keeps Wait expensive; Spec still wins (no Boolean Wait), but cost raised.
        let a = choose_action(c);
        assert_eq!(a, ResolveAction::SpecRead);
    }

    #[test]
    fn aec_params_are_rates_not_wait_cuts() {
        let p = AdaptiveParams::default();
        assert!(p.alpha_fanout > 0.0);
        assert!(p.meta_budget_rho > 0.0);
        // d_wait retained but unused by choose_action ladders.
        assert!((p.d_wait - D_WAIT).abs() < f64::EPSILON);
        assert!((p.d_early - D_EARLY).abs() < f64::EPSILON);
    }

    #[test]
    fn early_abort_requires_known_depth() {
        let mut c = ctx_aec(0.3, true, false, None, false, true, true, 8.0, None);
        c.tx_heavy_hint = true;
        assert!(!early_abort_candidate(&c));
    }

    #[test]
    fn early_abort_candidate_arms_when_ev_prefers() {
        let mut c = ctx_aec(0.95, true, false, None, false, true, false, 1.0, Some(0.10));
        c.tx_heavy_hint = true;
        c.e_wait_time = 20.0;
        c.e_cascade = 20.0;
        assert!(early_abort_candidate(&c));
        assert_eq!(choose_action(c), ResolveAction::EarlyAbort);
    }

    #[test]
    fn cost_constants_documented() {
        assert!(COST_MARGIN < 1.0);
        assert!(C_RETRY >= 2.0 && C_RETRY <= 4.0);
        assert!((TAU_VERY_HIGH - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn g3_no_handler_waithard_from_high_p() {
        let a = choose_action(ctx_aec(0.90, true, false, None, false, false, true, 8.0, None));
        assert_eq!(a, ResolveAction::SpecRead);
    }

    #[test]
    fn v5_p2_ev_wait_uses_wake_and_idle_theta() {
        let mut c = ctx_aec(0.5, true, false, None, false, true, false, 2.0, None);
        c.e_wait_time = 1.0;
        c.e_idle_steal = c.params.e_idle_prior; // at prior → no excess tax
        let ev0 = compute_ev(&c);
        c.e_idle_steal = 0.0; // below prior still no excess
        let ev_lo = compute_ev(&c);
        assert!(
            (ev_lo.ev_wait - ev0.ev_wait).abs() < 1e-12,
            "idle at/below prior must match P1 EV_Wait"
        );
        c.e_idle_steal = 2.0; // park-heavy excess
        let ev1 = compute_ev(&c);
        assert!(
            ev1.ev_wait > ev0.ev_wait,
            "idle excess θ must raise EV_Wait: {:?} vs {:?}",
            ev1.ev_wait,
            ev0.ev_wait
        );
        c.e_wait_time = 3.0; // longer wake
        let ev2 = compute_ev(&c);
        assert!(
            ev2.ev_wait > ev1.ev_wait,
            "wake θ must raise EV_Wait: {:?} vs {:?}",
            ev2.ev_wait,
            ev1.ev_wait
        );
    }

    #[test]
    fn v5_p2_ev_spec_uses_measured_reexec_gamma() {
        let mut c = ctx_aec(0.8, true, false, None, false, true, false, 1.0, Some(0.3));
        c.e_cascade = 0.5;
        c.e_reexec = 1.2; // Lean ForceBind
        let cheap = compute_ev(&c);
        c.e_reexec = 2.2; // FullRestart
        let dear = compute_ev(&c);
        assert!(
            dear.ev_spec > cheap.ev_spec,
            "higher E_reexec must raise EV_Spec: {:?} vs {:?}",
            dear.ev_spec,
            cheap.ev_spec
        );
        // γ is a param (not a Boolean gate).
        assert!((c.params.gamma_reexec - 0.5).abs() < 1e-9);
    }

    #[test]
    fn v5_p2_meta_tax_tiny_gap_biases_specread() {
        // Clear Wait win with zero meta → WaitHard (cold start ≡ P1).
        let mut c = ctx_aec(0.90, true, false, None, false, true, false, 1.0, Some(0.2));
        c.e_wait_time = 0.3;
        c.e_cascade = 2.0;
        c.e_idle_steal = 0.0;
        c.meta_tax = 0.0;
        let ev = compute_ev(&c);
        assert!(ev.ev_wait + 1e-9 < ev.ev_spec, "{:?}", ev);
        assert_eq!(choose_action(c.clone()), ResolveAction::WaitHard);

        // Tiny Wait win + measured meta_tax → SpecRead (no SoftWait storm).
        let mut c2 = c.clone();
        let gap_target = c2.params.meta_gap_eps * 0.5;
        let fanout = 1.0;
        let ev_spec = compute_ev(&c2).ev_spec;
        c2.e_wait_time = (ev_spec - gap_target) / (1.0 + c2.params.alpha_fanout * fanout);
        c2.e_idle_steal = 0.0;
        c2.meta_tax = 0.0;
        assert_eq!(
            choose_action(c2.clone()),
            ResolveAction::WaitHard,
            "zero meta_tax must not tiny-gap Spec"
        );
        c2.meta_tax = 0.5; // thresh = 0.06*(1.5)=0.09 > gap_target
        assert_eq!(
            choose_action(c2),
            ResolveAction::SpecRead,
            "measured meta tax + tiny Wait win → Spec"
        );
    }

    #[test]
    fn v5_p2_high_meta_tax_widens_tiny_gap_window() {
        let mut c = ctx_aec(0.85, true, false, None, false, true, false, 1.0, Some(0.25));
        c.e_cascade = 1.0;
        c.e_reexec = 1.5;
        c.e_idle_steal = 0.0;
        let ev_spec = compute_ev(&c).ev_spec;
        // Gap = 1.5*eps: zero meta → Wait; high meta widens window → Spec.
        let gap = c.params.meta_gap_eps * 1.5;
        c.e_wait_time = (ev_spec - gap) / (1.0 + c.params.alpha_fanout * 1.0);
        c.meta_tax = 0.0;
        assert_eq!(choose_action(c.clone()), ResolveAction::WaitHard);
        c.meta_tax = 2.0; // thresh = 0.06*(1+2)=0.18 > gap
        assert_eq!(choose_action(c), ResolveAction::SpecRead);
    }
}
