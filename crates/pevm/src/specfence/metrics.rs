//! Test-visible `SpecFence` counters. Updated atomically during a block.

use std::sync::atomic::{AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use crate::BuildSuffixHasher;

/// Snapshot of `SpecFence` counters after a parallel block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpecFenceMetrics {
    /// Times a transaction was blocked because a Wait-mode region was not yet
    /// written by the prior consensus-order writer (proactive PCC admission).
    pub wait_admissions: usize,
    /// Successful executions that ran against Speculate (OCC) hinted accounts.
    pub speculate_executions: usize,
    /// Intra-block Speculate → Wait promotions (wave updates).
    pub region_promotions: usize,
    /// Validation aborts (OCC / SpecFence / PCC).
    pub occ_aborts: usize,
    /// Higher txs forced into the validation cascade by an abort rewind
    /// (`block_size - rewind_to` when a dependent reader exists).
    pub cascade_validations_scheduled: usize,
    /// Higher txs between `aborted_idx+1` and the first dependent reader that
    /// were **not** forced into the abort cascade (SpecFence fence).
    pub independent_txs_skipped_by_fence: usize,
    /// Bayesian decide() chose Wait for a location/account access.
    pub bayes_wait_decisions: usize,
    /// Bayesian decide() chose Speculate.
    pub bayes_speculate_decisions: usize,
    /// `observe_conflict` updates applied.
    pub bayes_conflict_updates: usize,
    /// `observe_speculate_ok` updates applied.
    pub bayes_success_updates: usize,
    /// Wave counter bumps when regions flip Speculate→Wait from bayes.
    pub wave_promotions: usize,
    /// Final wave id after the block.
    pub wave_id: usize,
    /// Mean conflict posterior among Wait decisions this block.
    pub mean_wait_posterior: f64,
    /// Accounts that triggered a Wait admission in this block.
    pub wait_addresses: Vec<Address>,
    /// `from`/`to` accounts of speculative executions in this block.
    pub speculate_addresses: Vec<Address>,
}

/// Shared counters written by worker threads.
#[derive(Debug, Default)]
pub(crate) struct MetricsInner {
    wait_admissions: AtomicUsize,
    speculate_executions: AtomicUsize,
    region_promotions: AtomicUsize,
    occ_aborts: AtomicUsize,
    cascade_validations_scheduled: AtomicUsize,
    independent_txs_skipped_by_fence: AtomicUsize,
    bayes_wait_decisions: AtomicUsize,
    bayes_speculate_decisions: AtomicUsize,
    bayes_conflict_updates: AtomicUsize,
    bayes_success_updates: AtomicUsize,
    wave_promotions: AtomicUsize,
    wait_addresses: DashMap<Address, (), BuildSuffixHasher>,
    speculate_addresses: DashMap<Address, (), BuildSuffixHasher>,
    hot_accounts: DashMap<Address, (), BuildSuffixHasher>,
}

impl MetricsInner {
    pub(crate) fn record_wait(&self, address: Address) {
        self.wait_admissions.fetch_add(1, Ordering::Relaxed);
        self.wait_addresses.insert(address, ());
        self.hot_accounts.insert(address, ());
    }

    pub(crate) fn record_speculate(&self, from: Address, to: Option<Address>) {
        self.speculate_executions.fetch_add(1, Ordering::Relaxed);
        self.speculate_addresses.insert(from, ());
        if let Some(to) = to {
            self.speculate_addresses.insert(to, ());
        }
    }

    pub(crate) fn record_promotion(&self, address: Option<Address>) {
        self.region_promotions.fetch_add(1, Ordering::Relaxed);
        if let Some(address) = address {
            self.hot_accounts.insert(address, ());
        }
    }

    pub(crate) fn record_occ_abort(&self) {
        self.occ_aborts.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_fence_cascade(
        &self,
        cascade_scheduled: usize,
        independent_skipped: usize,
    ) {
        if cascade_scheduled > 0 {
            self.cascade_validations_scheduled
                .fetch_add(cascade_scheduled, Ordering::Relaxed);
        }
        if independent_skipped > 0 {
            self.independent_txs_skipped_by_fence
                .fetch_add(independent_skipped, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_bayes_wait(&self) {
        self.bayes_wait_decisions.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_speculate(&self) {
        self.bayes_speculate_decisions
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_conflict(&self) {
        self.bayes_conflict_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_success(&self) {
        self.bayes_success_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_wave_promotion(&self) {
        self.wave_promotions.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn mark_hot(&self, address: Address) {
        self.hot_accounts.insert(address, ());
    }

    pub(crate) fn hot_accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.hot_accounts.iter().map(|entry| *entry.key())
    }

    pub(crate) fn snapshot(&self, wave_id: usize, mean_wait_posterior: f64) -> SpecFenceMetrics {
        let mut wait_addresses: Vec<Address> =
            self.wait_addresses.iter().map(|e| *e.key()).collect();
        wait_addresses.sort_unstable();
        let mut speculate_addresses: Vec<Address> =
            self.speculate_addresses.iter().map(|e| *e.key()).collect();
        speculate_addresses.sort_unstable();
        SpecFenceMetrics {
            wait_admissions: self.wait_admissions.load(Ordering::Relaxed),
            speculate_executions: self.speculate_executions.load(Ordering::Relaxed),
            region_promotions: self.region_promotions.load(Ordering::Relaxed),
            occ_aborts: self.occ_aborts.load(Ordering::Relaxed),
            cascade_validations_scheduled: self
                .cascade_validations_scheduled
                .load(Ordering::Relaxed),
            independent_txs_skipped_by_fence: self
                .independent_txs_skipped_by_fence
                .load(Ordering::Relaxed),
            bayes_wait_decisions: self.bayes_wait_decisions.load(Ordering::Relaxed),
            bayes_speculate_decisions: self.bayes_speculate_decisions.load(Ordering::Relaxed),
            bayes_conflict_updates: self.bayes_conflict_updates.load(Ordering::Relaxed),
            bayes_success_updates: self.bayes_success_updates.load(Ordering::Relaxed),
            wave_promotions: self.wave_promotions.load(Ordering::Relaxed),
            wave_id,
            mean_wait_posterior,
            wait_addresses,
            speculate_addresses,
        }
    }
}
