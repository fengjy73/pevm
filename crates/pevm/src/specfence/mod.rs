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
//! suffix-only InvalidateSelective when safe.
//! M2: WaitHard parks (tx-level) + ready-queue steal (lower TxIdx first); worker never spins.

use crate::{
    BuildSuffixHasher, MemoryLocation, TxIdx, chain::PevmChain, hash_deterministic,
    scheduler::Scheduler,
};
use alloy_primitives::Address;
use hashbrown::HashMap;

mod bayes;
mod boundary;
mod dag;
mod heat;
mod metrics;
mod region;
mod rem;
mod resolve;

pub(crate) use bayes::{BayesMap, DEFAULT_TAU};
pub(crate) use dag::SpecDag;
pub(crate) use heat::HeatMap;
pub(crate) use metrics::MetricsInner;
pub use metrics::SpecFenceMetrics;
pub use region::RegionMode;
pub(crate) use region::RegionTable;
pub(crate) use rem::RemCounters;
pub(crate) use rem::PartialRetryTable;
pub(crate) use rem::WaveParkTable;
#[allow(unused_imports)]
pub(crate) use boundary::{
    arm_pc_resume, clear_pc_resume, last_boundary_snap, note_pending_effect_boundary,
    resume_was_applied, steps_this_run, with_plant_tls, BoundarySnapshot,
};
pub use boundary::SpecFenceInspector;
#[allow(unused_imports)]
pub(crate) use rem::{
    AccessMode, Checkpoint, CheckpointId, CheckpointKind, EffectOrdinal, FfValue, ParkedWait,
    PartialRetryPlan, PartialRetryState, RegionAccess, RemTask, RepairPlan, ResumeContinuation,
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
}

impl<'a> SpecFenceCtx<'a> {
    pub(crate) fn should_wait_account(&self, regions: &RegionTable, address: &Address) -> bool {
        if !self.mode.uses_regions() || *address == self.beneficiary {
            return false;
        }
        if self.mode == ConcurrencyMode::Pcc {
            return true;
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
    ) -> ResolveAction {
        let posterior_conflict = self.bayes.conflict_probability(location, Some(address));
        let posterior_bind = self.bayes.bind_useful_probability(location);
        let ctx = PolicyCtx {
            location,
            writer_known: writer.is_some(),
            writer,
            writer_done,
            posterior_conflict,
            posterior_bind_success: posterior_bind,
            placeholder_ready: residual_predicts && (writer_done || bind_version.is_some()),
            // Bind only against a published writer version.
            bind_version: if writer_done { bind_version } else { None },
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
