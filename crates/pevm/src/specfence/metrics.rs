//! Test-visible `SpecFence` counters. Updated atomically during a block.

use std::sync::atomic::{AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use crate::BuildSuffixHasher;

/// Snapshot of `SpecFence` counters after a parallel block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecFenceMetrics {
    /// Times a transaction was blocked because a Wait-mode region was not yet
    /// written by the prior consensus-order writer (proactive PCC admission).
    pub wait_admissions: usize,
    /// Successful executions that ran against Speculate (OCC) hinted accounts.
    pub speculate_executions: usize,
    /// Intra-block Speculate → Wait promotions (wave updates).
    pub region_promotions: usize,
    /// Validation aborts (OCC).
    pub occ_aborts: usize,
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

    pub(crate) fn mark_hot(&self, address: Address) {
        self.hot_accounts.insert(address, ());
    }

    pub(crate) fn hot_accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.hot_accounts.iter().map(|entry| *entry.key())
    }

    pub(crate) fn snapshot(&self) -> SpecFenceMetrics {
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
            wait_addresses,
            speculate_addresses,
        }
    }
}
