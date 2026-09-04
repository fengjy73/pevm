//! `SpecFence`: adaptive region/wave concurrency control (v2).
//!
//! Control unit = memory location / slot-level region
//! (`MemoryLocation::{Basic, CodeHash, Storage}`), not whole-tx.
//! Bayesian Beta-Bernoulli posteriors predict Wait vs Speculate per region;
//! validation/abort/success observations update posteriors across waves and
//! blocks. Cascade fence is a correctness shield only — not the decision rule.
//! Whole-tx re-execution remains (revm); region-local repair is future work.

use crate::{
    BuildSuffixHasher, MemoryLocation, TxIdx, chain::PevmChain, hash_deterministic,
    scheduler::Scheduler,
};
use alloy_primitives::Address;
use hashbrown::HashMap;

mod bayes;
mod heat;
mod metrics;
mod region;

pub(crate) use bayes::{BayesMap, DEFAULT_TAU};
pub(crate) use heat::HeatMap;
pub(crate) use metrics::MetricsInner;
pub use metrics::SpecFenceMetrics;
pub use region::RegionMode;
pub(crate) use region::RegionTable;

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
}

impl<'a> SpecFenceCtx<'a> {
    pub(crate) fn should_wait_account(&self, regions: &RegionTable, address: &Address) -> bool {
        if !self.mode.uses_regions() || *address == self.beneficiary {
            return false;
        }
        if self.mode == ConcurrencyMode::Pcc {
            return true;
        }
        // SpecFence: account Wait only as cold-start / seeded Basic proxy.
        if regions.account_mode(address) == RegionMode::Wait {
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

    /// Location-granularity Wait: region table promotion, else bayes on that
    /// location hash, else address-level cold-start posterior.
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
        if regions.location_mode(location) == RegionMode::Wait {
            return true;
        }
        // Once the location has its own posterior, do **not** inherit whole-account Wait.
        if self.bayes.has_location(location) {
            if self.bayes.decide(location, Some(address), self.tau) == RegionMode::Wait {
                self.metrics.record_bayes_wait();
                self.bayes.note_wait_decision(location, Some(address));
                true
            } else {
                self.metrics.record_bayes_speculate();
                false
            }
        } else if regions.account_mode(address) == RegionMode::Wait {
            true
        } else if self.bayes.decide(location, Some(address), self.tau) == RegionMode::Wait {
            self.metrics.record_bayes_wait();
            self.bayes.note_wait_decision(location, Some(address));
            true
        } else {
            self.metrics.record_bayes_speculate();
            false
        }
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

    /// Promote location to Wait for the rest of the block; bump wave on new flip.
    pub(crate) fn promote_from_bayes(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<Address>,
    ) -> bool {
        if regions.promote_location(location) {
            self.metrics.record_promotion(address);
            self.metrics.record_wave_promotion();
            self.bayes.bump_wave();
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
