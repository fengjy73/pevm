//! SpecFence **v5** — Block-STM + FenceGraph SoftWait + AEC π + SuffixRepair resolve.
//!
//! Authoritative resolve: `lab/notes/specfence-native-resolve-protocol.md`.
//! SpecFence is its **own** CC protocol — not optimized OCC.
//!
//! # Single algorithm (hot path)
//! ```text
//! Block-STM scheduler + MvMemory          # L0
//!     ↑
//! FenceGraph: SoftWait / wake only        # L2 — sole Wait authority
//!     ↑
//! π = argmin EV[Bind, Await, Spec, Early] # L3 — choose_action (AEC)
//!     ↑
//! OutcomeLearner: P_abort, T_wait, …      # L4 — continuous θ features only
//!     ↑
//! Resolve: SuffixRepair (RewindTo+FF) | FullRestart last resort  # L1
//! ```
//!
//! **Resolve default:** SuffixRepair (hang-free RewindTo + journal FF + force-bind)
//! when a certified checkpoint exists before fail `k` — not OCC head restart.
//! SoftWait / Await when `EV_Wait ≲ EV_Spec` and producer Running; SpecRead for
//! writer absent/unknown discovery. Fan-out raises `EV_Wait` (discourage serialize).
//! Absolute jump / inspect stay opt-in research only.
//!
//! # Shoveled off SpecFence control (V5-P0)
//! - Heat / `seed_wait_regions` SoftWait arming (PCC may still seed account Wait)
//! - Account Wait / `promote_account` (diagnostic stub; always false)
//! - `RegionTable` Wait as decision authority (mirrors only; FenceGraph SoT)
//! - Bayes `should_wait_hard` Boolean second π (Beta posteriors = EV features)
//! - AdaptiveEngagement abort_rate mode ladders (always Lean execute)
//! - HotSet as Wait gate (`H_w`/`H_a` = optional dense-stat / fanout features)
//!
//! # Research-only (not default behavior) — V5-P3
//! Inspect / absolute jump / CallOutcome SC stay behind `SPECFENCE_ENABLE_INSPECT=1`.
//! Plant M1a–M1l + [`research_apply_abort_repair`] remain research; **not** graduated
//! (A/B: inspect hangs on 597 path). Lean SoftWait wake may still arm hang-free
//! journal FF via `try_arm_park_resume_at_k` (no absolute jump).
//! Finegrain collectors are lab opt-in.
//!
//! Correctness shield: cascade fence + Block-STM validate / ESTIMATE unchanged.
//! Learning never commits. SpecFence ≡ sequential on Ethereum fixtures.

use crate::{
    BuildSuffixHasher, MemoryLocation, TxIdx, chain::PevmChain, hash_deterministic,
    scheduler::Scheduler,
};
use alloy_primitives::Address;
use hashbrown::HashMap;

mod bayes;
mod engagement;
mod prior;
mod boundary;
mod dag;
#[allow(missing_docs)]
mod finegrain;
mod heat;
mod hotset;
mod learner;
mod metrics;
mod region;
mod rem;
mod resolve;

pub(crate) use bayes::{BayesMap, DEFAULT_TAU};
pub(crate) use engagement::{AdaptiveEngagement, research_inspect_enabled, softwait_disabled};
pub(crate) use hotset::HotSet;
#[allow(unused_imports)]
pub(crate) use hotset::{H_A, H_W};
pub(crate) use prior::RwPriorMap;
pub(crate) use dag::{FenceGraph, SpecDag};
pub(crate) use learner::{
    AdaptiveParams, InterBlockPrior, LiveLearner, MorphWeights, TopLocPrior,
};
pub(crate) use heat::HeatMap;
pub(crate) use metrics::MetricsInner;
pub use metrics::SpecFenceMetrics;
pub use finegrain::{
    AbortEvent, AccountGrainObserve, ConsumerFirstCross, DagStats, EffectClass, EffectLogEntry,
    FineGrainCollector, FineGrainSnapshot, EffectStreamDiag, HotLocation, L1DagSummary,
    LocationKind, MaMdProxy, MeasurementMethod, RawEffectEdge, TxRw, RawEdge, TxWorkTotal,
    analyze_dag, classify_raw_edges, dependency_edges, effect_raw_longest_chain,
    effect_raw_max_fanout, estimate_ma_md, filter_effect_edges, hot_locations, kind_histogram,
    l1_dag_summary, percentile_f64, producer_status_canonical, program_raw_longest_chain,
};
pub use region::RegionMode;
pub(crate) use region::RegionTable;
pub(crate) use rem::RemCounters;
pub(crate) use rem::PartialRetryTable;
pub(crate) use rem::WaveParkTable;
#[allow(unused_imports)]
pub(crate) use boundary::{
    arm_call_outcome_cache, arm_pc_resume, clear_pc_resume, in_inspect_run, jump_is_safe,
    last_boundary_snap, attach_current_live_snap, note_pending_effect_boundary, resume_was_applied,
    steps_this_run, try_arm_safe_absolute_jump, with_plant_tls, with_plant_tls_journal,
    BoundarySnapshot, CachedCallOutcome,
    JournalBlob,
};
pub use boundary::SpecFenceInspector;
#[allow(unused_imports)]
pub(crate) use rem::{
    AccessMode, Checkpoint, CheckpointId, CheckpointKind, EffectOrdinal, FfValue, LeanAbortRepair,
    ParkedWait, ParkResumeIntent, ParkResumeKind, PartialRetryPlan, PartialRetryState, PendingPark,
    RegionAccess, RemTask, RepairPlan, ResearchAbortRepair, ResumeContinuation, StorageWriteReplay,
};
pub(crate) use resolve::{PolicyCtx, ResolveAction, choose_action, early_abort_candidate};
#[allow(unused_imports)]
pub(crate) use resolve::{
    BindTarget, EvScores, SelectiveOutcome, C_RETRY, COST_MARGIN, D_EARLY, D_WAIT, TAU_REVOKE,
    TAU_S, TAU_VERY_HIGH, TAU_W, compute_ev, cost_prefers_wait, early_val_probability,
};

/// Selectable concurrency control for parallel block execution.
///
/// Default is current PEVM Block-STM (OCC). `SpecFence` mixes Wait and Speculate
/// in the same block; PCC waits on hinted prior writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConcurrencyMode {
    /// Block-STM optimistic concurrency. Unchanged default path.
    #[default]
    Occ,
    /// Conservative PCC: hinted `from`/`to` accounts start in Wait.
    Pcc,
    /// SpecFence v5: AEC π + FenceGraph SoftWait at location grain (Bayes = θ features).
    SpecFence,
}

impl ConcurrencyMode {
    /// Whether this mode uses per-region Wait/Speculate state.
    pub const fn uses_regions(self) -> bool {
        matches!(self, Self::Pcc | Self::SpecFence)
    }
}

/// Cheap `from`/`to` index: which transactions hint they touch an account.
#[derive(Debug, Default)]
pub(crate) struct AccountHints {
    by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher>,
}

impl AccountHints {
    pub(crate) fn build<C: PevmChain>(chain: &C, txs: &[C::EvmTx]) -> Self {
        let mut by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher> =
            HashMap::with_hasher(BuildSuffixHasher::default());
        for (idx, tx) in txs.iter().enumerate() {
            let env = chain.tx_env(tx);
            by_account.entry(env.caller).or_default().push(idx);
            if let Some(to) = env.kind.to() {
                by_account.entry(*to).or_default().push(idx);
            }
        }
        for list in by_account.values_mut() {
            list.sort_unstable();
            list.dedup();
        }
        Self { by_account }
    }

    pub(crate) fn accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.by_account.keys().copied()
    }

    pub(crate) fn writer_count(&self, address: &Address) -> usize {
        self.by_account.get(address).map(Vec::len).unwrap_or(0)
    }

    /// Last transaction before `tx_idx` that hinted this account.
    pub(crate) fn prev(&self, address: &Address, tx_idx: TxIdx) -> Option<TxIdx> {
        let list = self.by_account.get(address)?;
        match list.binary_search(&tx_idx) {
            Ok(i) | Err(i) if i > 0 => Some(list[i - 1]),
            _ => None,
        }
    }
}

/// Shared `SpecFence` context for one block (copied into workers).
#[derive(Clone, Copy, Debug)]
pub(crate) struct SpecFenceCtx<'a> {
    pub mode: ConcurrencyMode,
    pub hints: &'a AccountHints,
    pub metrics: &'a MetricsInner,
    pub scheduler: &'a Scheduler,
    pub beneficiary: Address,
    pub bayes: &'a BayesMap,
    pub tau: f64,
    pub dag: &'a SpecDag,
    pub rem: &'a RemCounters,
    pub partial_retry: &'a PartialRetryTable,
    /// M2 wave park / ready deque (SpecFence only; unused by OCC/PCC).
    pub wave: &'a WaveParkTable,
    /// M3 process-local online WŜ/RŜ prior (Bind-before-touch).
    pub rw_prior: &'a RwPriorMap,
    /// M4/R1 adaptive lean engagement (SpecFence only).
    pub engagement: &'a AdaptiveEngagement,
    /// R1 location-local HotSet (fanout/tracking hint only).
    pub hotset: &'a HotSet,
    /// P1 dual-horizon live learner (morph / fanout).
    pub learner: &'a LiveLearner,
    /// P1 tunable π constants.
    pub params: &'a AdaptiveParams,
    /// Opt-in lab fine-grain OCC/RW tracer (None = disabled, zero cost).
    pub finegrain: Option<&'a crate::specfence::FineGrainCollector>,
}

impl<'a> SpecFenceCtx<'a> {
    pub(crate) fn should_wait_account(&self, regions: &RegionTable, address: &Address) -> bool {
        if !self.mode.uses_regions() || *address == self.beneficiary {
            return false;
        }
        if self.mode == ConcurrencyMode::Pcc {
            return true;
        }
        // V5-P0: SpecFence conflict key = MemoryLocation only. Account Wait is
        // a diagnostic stub — never schedules SoftWait / never promotes.
        let _ = (regions, address);
        false
    }

    /// PCC sticky Wait probe. SpecFence v5: **always false**.
    ///
    /// SpecFence Wait is decided only by `choose_action` → FenceGraph SoftWait
    /// inside `Vm::maybe_wait`. Bayes Bool / RegionTable sticky / account Wait
    /// must not OR into SpecFence π (V5-P0 shovel).
    pub(crate) fn should_wait_location(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: &Address,
    ) -> bool {
        if !self.mode.uses_regions() || *address == self.beneficiary {
            return false;
        }
        if self.mode == ConcurrencyMode::Pcc {
            return true;
        }
        // SpecFence: diagnostic no-op. SoftWait arms only via choose_action.
        let _ = (regions, location, address);
        false
    }

    /// Choose ResolveAction for a SpecFence location read (AEC argmin EV).
    pub(crate) fn choose_resolve(
        &self,
        location: crate::MemoryLocationHash,
        address: &Address,
        writer: Option<TxIdx>,
        writer_done: bool,
        bind_version: Option<crate::TxVersion>,
        residual_predicts: bool,
        prior_ws_predicts: bool,
        is_program: bool,
        fanout_hint: bool,
        live_fanout: f64,
        gross_work_depth: Option<f64>,
        waw_spine_hint: bool,
        tx_heavy_hint: bool,
    ) -> ResolveAction {
        let posterior_conflict = self.bayes.conflict_probability(location, Some(address));
        let posterior_bind = self.bayes.bind_useful_probability(location)
            .max(self.rw_prior.write_confidence(location));
        // M3: residual / process prior makes a published version a Bind placeholder.
        let prior = residual_predicts || prior_ws_predicts;
        let morph_weights = self.learner.morph_weights();
        let e_wait_time = self.learner.e_wait_time(location);
        let e_cascade = self.learner.e_cascade(location);
        let e_reexec = self.learner.e_reexec();
        let e_idle_steal = self.learner.e_idle_steal();
        let meta_tax = self.learner.meta_tax_ratio(self.params);
        let meta_budget_exceeded = self.learner.meta_budget_exceeded(self.params);
        // G3: pass published Data version into π even when writer not yet is_done —
        // choose_action decides Bind via prior_ws / high P / placeholder_ready.
        let ctx = PolicyCtx {
            location,
            writer_known: writer.is_some() || bind_version.is_some(),
            writer,
            writer_done,
            posterior_conflict,
            posterior_bind_success: posterior_bind,
            placeholder_ready: prior && (writer_done || bind_version.is_some()),
            bind_version,
            prior_ws_predicts: prior,
            is_program,
            fanout_hint,
            live_fanout,
            e_wait_time,
            e_cascade,
            e_reexec,
            e_idle_steal,
            meta_tax,
            meta_budget_exceeded,
            gross_work_depth,
            morph_weights,
            waw_spine_hint,
            tx_heavy_hint,
            params: *self.params,
        };
        let action = choose_action(ctx);
        match &action {
            ResolveAction::WaitHard => {
                self.metrics.record_cost_chose_wait();
                self.learner.note_meta_op();
                if is_program {
                    self.metrics.record_cost_chose_wait_program();
                } else {
                    self.metrics.record_cost_chose_wait_handler();
                }
                self.bayes.note_cost_decision_posterior(posterior_conflict, true);
            }
            ResolveAction::EarlyAbort => {
                // EarlyAbort niche — count as wait-side cost choice + early_abort.
                self.metrics.record_cost_chose_wait();
                if is_program {
                    self.metrics.record_cost_chose_wait_program();
                } else {
                    self.metrics.record_cost_chose_wait_handler();
                }
                self.metrics.record_early_abort();
                self.bayes.note_cost_decision_posterior(posterior_conflict, true);
            }
            ResolveAction::SpecRead => {
                self.metrics.record_cost_chose_spec();
                if is_program {
                    self.metrics.record_cost_chose_spec_program();
                } else {
                    self.metrics.record_cost_chose_spec_handler();
                }
                self.bayes.note_cost_decision_posterior(posterior_conflict, false);
            }
            ResolveAction::Bind(_) => {
                self.metrics.record_cost_chose_bind();
            }
        }
        action
    }

    /// Proactive PCC / cold-start: previous hinted writer that has not finished.
    pub(crate) fn wait_blocker(
        &self,
        regions: &RegionTable,
        address: &Address,
        tx_idx: TxIdx,
    ) -> Option<TxIdx> {
        if !self.should_wait_account(regions, address) {
            return None;
        }
        let prev = self.hints.prev(address, tx_idx)?;
        if self.scheduler.is_done(prev) {
            None
        } else {
            Some(prev)
        }
    }

    /// Promote location Wait **mirror** (FenceGraph SoftWait remains authority for arms).
    /// Abort/conflict densifies tracking; does not arm SoftWait by itself.
    pub(crate) fn promote_from_bayes(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<Address>,
    ) -> bool {
        let promoted = regions.promote_location(location);
        // Mirror only — SoftWait arms come from choose_action→WaitHard→arm_soft.
        let _ = self.dag.set_wait(location);
        if promoted {
            self.metrics.record_promotion(address);
            self.metrics.record_wave_promotion();
            self.bayes.bump_wave();
            true
        } else {
            false
        }
    }

    /// Unified SoftWait revoke: τ_revoke and/or morph quiet|waw.
    pub(crate) fn try_revoke_unified(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<&Address>,
    ) -> bool {
        let morph = self.learner.morph_weights();
        let morph_revoke = morph.dominant_quiet() || morph.dominant_waw();
        let bayes_revoke = self.bayes.should_revoke(location, address);
        if !bayes_revoke && !morph_revoke {
            return false;
        }
        // Only morph-revoke SoftWaits when posterior is also not insisting on Wait,
        // or when quiet/waw dominates (schedule/steal ≫ sticky Wait).
        if morph_revoke || bayes_revoke {
            let cleared_region = regions.clear_location_wait(location);
            let cleared_fence = self.dag.clear(location) > 0 || self.dag.clear_wait(location);
            if cleared_region || cleared_fence {
                self.metrics.record_soft_edge_revoke();
                return true;
            }
        }
        false
    }

    /// Attempt revoke of sticky Wait when posterior dropped (delegates to unified).
    pub(crate) fn try_revoke(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<&Address>,
    ) -> bool {
        self.try_revoke_unified(regions, location, address)
    }
}

/// Seed account Wait for **PCC only**.
///
/// SpecFence v5: this is a **no-op** for SoftWait / Region Wait. Heat and Bayes
/// priors must never arm SoftWait at block start (V5-P0 shovel). Callers should
/// gate on `ConcurrencyMode::Pcc` (see `pevm.rs`).
pub(crate) fn seed_wait_regions(
    regions: &RegionTable,
    hints: &AccountHints,
    bayes: &BayesMap,
    mode: ConcurrencyMode,
    beneficiary: Address,
    tau: f64,
    initial_wait: &mut std::collections::HashSet<Address>,
) {
    if mode != ConcurrencyMode::Pcc {
        let _ = (regions, hints, bayes, beneficiary, tau, initial_wait);
        return;
    }
    for address in hints.accounts() {
        if address == beneficiary {
            continue;
        }
        let _ = (bayes, tau);
        regions.seed_account_wait(address);
        regions.promote_location(hash_deterministic(MemoryLocation::Basic(address)));
        initial_wait.insert(address);
    }
}

/// Apply bounded EWMA updates (PCC / legacy heat path). SpecFence uses Bayes.
pub(crate) fn update_heat(
    heat: &HeatMap,
    hints: &AccountHints,
    metrics: &MetricsInner,
    beneficiary: Address,
) {
    for address in hints.accounts() {
        if address != beneficiary && hints.writer_count(&address) >= 2 {
            heat.observe(address);
        }
    }
    for address in metrics.hot_accounts() {
        if address != beneficiary {
            heat.observe(address);
        }
    }
}

/// End-of-block Bayesian maintenance for SpecFence.
pub(crate) fn update_bayes(bayes: &BayesMap) {
    bayes.decay_block();
}

/// End-of-block RW prior decay (M3).
pub(crate) fn update_rw_prior(rw_prior: &RwPriorMap) {
    rw_prior.decay_block();
}
