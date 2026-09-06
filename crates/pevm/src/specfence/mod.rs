//! `SpecFence`: adaptive region/wave concurrency control (Spec v1 / P2).
//!
//! Control unit = memory location / slot-level region
//! (`MemoryLocation::{Basic, CodeHash, Storage}`), not whole-tx.
//! Bayesian Beta-Bernoulli posteriors + cost-aware π drive WaitHard / Bind /
//! SpecRead per region; sticky Wait is revokeable when posterior < τ_revoke.
//! Cascade fence
//! remains a correctness shield. FullRestart remains whole-tx reexec (revm).
//! M1: RebindOnly / RewindTo on certified prefix (not head reexec when cps allow);
//! M1b: journal FF + bound-value cache on RewindTo (prefix DB heavy path skipped).
//! M1c: CALL/effect-boundary PC resume via stock Inspector (prefix opcodes skipped).
//! M1d: live `inspect_run` on SpecFence production path (Ethereum) — real PC skip.
//! M1e: journal-blob FF + safety-gated absolute PC jump on RewindTo resume.
//! M1f: absolute PC jump default-on when safe (MemoryGas+refund restore); `SPECFENCE_ABSOLUTE_JUMP=0` disables.
//! M1g: Storage-prefix jump (no Db poison) + nested CallOutcome cache; bytecode_len relaxed carefully.
//! M1i: post-SSTORE write-prefix jump default-on when safe; valued CallOutcome hang-free.
//! M1j: multi-SSTORE write-prefix jump (+ LOG after tip).
//! M1k: hang-free jump-past-LOG (LogReplay) + valued CallOutcome default-on; zero-value+write combine at CALL-boundary.
//! M1l: lighter inspect step + multi-SSTORE at higher width; warm valued SC gas_limit-match; valued+write CALL-boundary jump.
//! suffix-only InvalidateSelective when safe.
//! M2: WaitHard parks (tx-level) + ready-queue steal (lower TxIdx first); worker never spins.
//! M3: online WŜ/RŜ prior → Bind-before-touch on first incarnation when writer version known.
//! M4 (superseded by Adaptive CC R1): lean thresholds that never fired on mainnet.
//! Adaptive CC R0–R2: default LeanOCC + location HotSet; inspect/jump off unless
//! `SPECFENCE_ENABLE_INSPECT=1`; WaitHard only for ℓ ∈ HotSet; HotLocal Bind/park on hot ℓ.

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
mod metrics;
mod region;
mod rem;
mod resolve;

pub(crate) use bayes::{BayesMap, DEFAULT_TAU};
pub(crate) use engagement::{AdaptiveEngagement, research_inspect_enabled};
pub(crate) use hotset::HotSet;
#[allow(unused_imports)]
pub(crate) use hotset::{H_A, H_W};
pub(crate) use prior::RwPriorMap;
pub(crate) use dag::SpecDag;
pub(crate) use heat::HeatMap;
pub(crate) use metrics::MetricsInner;
pub use metrics::SpecFenceMetrics;
pub use finegrain::{
    AbortEvent, AccountGrainObserve, ConsumerFirstCross, DagStats, EffectClass, FineGrainCollector, FineGrainSnapshot, EffectStreamDiag,
    HotLocation, LocationKind, MaMdProxy, RawEffectEdge, TxRw, RawEdge, analyze_dag,
    classify_raw_edges, dependency_edges, effect_raw_longest_chain, effect_raw_max_fanout,
    estimate_ma_md, filter_effect_edges, hot_locations, kind_histogram, percentile_f64,
    program_raw_longest_chain,
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
    AccessMode, Checkpoint, CheckpointId, CheckpointKind, EffectOrdinal, FfValue, ParkedWait,
    PartialRetryPlan, PartialRetryState, RegionAccess, RemTask, RepairPlan, ResumeContinuation,
    StorageWriteReplay,
};
pub(crate) use resolve::{PolicyCtx, ResolveAction, choose_action};
#[allow(unused_imports)]
pub(crate) use resolve::{
    BindTarget, SelectiveOutcome, C_RETRY, COST_MARGIN, TAU_REVOKE, TAU_S, TAU_VERY_HIGH, TAU_W,
    cost_prefers_wait, early_val_probability,
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
    /// Mixed Wait/Speculate at **location** granularity with Bayesian feedback.
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
    /// R1 location-local HotSet (WaitHard/Bind only for members).
    pub hotset: &'a HotSet,
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
        // R1: account-level Wait only when Basic(address) ∈ HotSet (no block-wide Wait).
        let basic = hash_deterministic(MemoryLocation::Basic(*address));
        if !self.hotset.contains(basic) {
            return false;
        }
        // SpecFence: revoke sticky account Wait when posterior low.
        if regions.account_mode(address) == RegionMode::Wait {
            if self.bayes.should_revoke(
                hash_deterministic(MemoryLocation::Basic(*address)),
                Some(address),
            ) {
                if regions.clear_account_wait(*address) {
                    self.metrics.record_soft_edge_revoke();
                }
                return false;
            }
            return true;
        }
        if self.bayes.decide_account(address, self.tau) == RegionMode::Wait {
            self.metrics.record_bayes_wait();
            self.bayes.note_wait_decision_account(address);
            true
        } else {
            self.metrics.record_bayes_speculate();
            false
        }
    }

    /// Location-granularity Wait via cost-aware π with revokeable sticky flags.
    /// Sticky Wait is a soft hint only: always re-evaluate with producer-unknown
    /// (`writer_done=false`) so over-WaitHard from sticky bias is avoided.
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
        // R1: WaitHard forbidden for ℓ ∉ HotSet.
        if !self.hotset.contains(location) {
            return false;
        }
        // Revoke sticky Wait when posterior < τ_revoke.
        if regions.location_mode(location) == RegionMode::Wait
            || self.dag.is_wait(location)
        {
            if self.bayes.should_revoke(location, Some(address)) {
                let cleared_region = regions.clear_location_wait(location);
                let cleared_dag = self.dag.clear_wait(location);
                if cleared_region || cleared_dag {
                    self.metrics.record_soft_edge_revoke();
                }
            }
            // Fall through — do not auto-WaitHard on sticky (v6 cost-aware).
        }
        // Live cost-aware π (producer unknown here → SpecRead-biased).
        let writer_known = self.hints.prev(address, 0).is_some()
            || self.bayes.has_location(location);
        let writer_done = false;
        if self.bayes.has_location(location) {
            if self
                .bayes
                .should_wait_hard(location, Some(address), writer_known, writer_done)
            {
                self.metrics.record_bayes_wait();
                self.bayes.note_wait_decision(location, Some(address));
                true
            } else {
                self.metrics.record_bayes_speculate();
                false
            }
        } else if regions.account_mode(address) == RegionMode::Wait {
            if self.bayes.should_revoke(location, Some(address)) {
                if regions.clear_account_wait(*address) {
                    self.metrics.record_soft_edge_revoke();
                }
                false
            } else if self
                .bayes
                .should_wait_hard(location, Some(address), writer_known, writer_done)
            {
                self.metrics.record_bayes_wait();
                self.bayes.note_wait_decision(location, Some(address));
                true
            } else {
                self.metrics.record_bayes_speculate();
                false
            }
        } else if self
            .bayes
            .should_wait_hard(location, Some(address), writer_known, writer_done)
        {
            self.metrics.record_bayes_wait();
            self.bayes.note_wait_decision(location, Some(address));
            true
        } else {
            self.metrics.record_bayes_speculate();
            false
        }
    }

    /// Choose ResolveAction for a SpecFence location read (cost-aware π).
    pub(crate) fn choose_resolve(
        &self,
        location: crate::MemoryLocationHash,
        address: &Address,
        writer: Option<TxIdx>,
        writer_done: bool,
        bind_version: Option<crate::TxVersion>,
        residual_predicts: bool,
        prior_ws_predicts: bool,
    ) -> ResolveAction {
        let posterior_conflict = self.bayes.conflict_probability(location, Some(address));
        let posterior_bind = self.bayes.bind_useful_probability(location)
            .max(self.rw_prior.write_confidence(location));
        // M3: residual / process prior makes a published version a Bind placeholder.
        let prior = residual_predicts || prior_ws_predicts;
        let ctx = PolicyCtx {
            location,
            writer_known: writer.is_some(),
            writer,
            writer_done,
            posterior_conflict,
            posterior_bind_success: posterior_bind,
            placeholder_ready: prior && (writer_done || bind_version.is_some()),
            // Bind only against a published writer version.
            bind_version: if writer_done { bind_version } else { None },
            prior_ws_predicts: prior,
        };
        let action = choose_action(ctx);
        match &action {
            ResolveAction::WaitHard => {
                self.metrics.record_cost_chose_wait();
                self.bayes.note_cost_decision_posterior(posterior_conflict, true);
            }
            ResolveAction::SpecRead => {
                self.metrics.record_cost_chose_spec();
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

    /// Promote location to Wait; bump wave on new flip. Also mirrors into Ĝ.
    pub(crate) fn promote_from_bayes(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<Address>,
    ) -> bool {
        let promoted = regions.promote_location(location);
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

    /// Attempt revoke of sticky Wait when posterior dropped.
    pub(crate) fn try_revoke(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<&Address>,
    ) -> bool {
        if !self.bayes.should_revoke(location, address) {
            return false;
        }
        let cleared_region = regions.clear_location_wait(location);
        let cleared_dag = self.dag.clear_wait(location);
        if cleared_region || cleared_dag {
            self.metrics.record_soft_edge_revoke();
            true
        } else {
            false
        }
    }
}

/// Seed Wait from PCC (all hinted accounts) or SpecFence Bayesian posteriors.
pub(crate) fn seed_wait_regions(
    regions: &RegionTable,
    hints: &AccountHints,
    bayes: &BayesMap,
    mode: ConcurrencyMode,
    beneficiary: Address,
    tau: f64,
    initial_wait: &mut std::collections::HashSet<Address>,
) {
    if !mode.uses_regions() {
        return;
    }
    for address in hints.accounts() {
        if address == beneficiary {
            continue;
        }
        let wait = mode == ConcurrencyMode::Pcc
            || bayes.decide_account(&address, tau) == RegionMode::Wait;
        if wait {
            regions.seed_account_wait(address);
            regions.promote_location(hash_deterministic(MemoryLocation::Basic(address)));
            initial_wait.insert(address);
        }
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
