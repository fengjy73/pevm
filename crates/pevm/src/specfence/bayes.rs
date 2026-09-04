//! Per-region Beta-Bernoulli conflict posterior for SpecFence Wait vs Speculate.
//!
//! Control unit is a memory location (`MemoryLocationHash`), with optional
//! address-level posteriors for cold-start when the slot is unknown pre-exec.
//! Cascade fence remains a correctness shield; this module drives decisions.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, BuildSuffixHasher, MemoryLocationHash};

use super::RegionMode;

/// Prior: mild low-conflict (`α=1`, `β=9` → P≈0.1).
const PRIOR_ALPHA: f64 = 1.0;
const PRIOR_BETA: f64 = 9.0;
/// Wait when P(conflict) ≥ τ.
pub(crate) const DEFAULT_TAU: f64 = 0.30;
/// Per-block decay of excess mass toward the prior: `(α-1),(β-1) *= λ`.
const DECAY_LAMBDA: f64 = 0.95;
const MAX_ENTRIES: usize = 8192;

#[derive(Debug, Clone, Copy)]
struct BetaPosterior {
    alpha: f64,
    beta: f64,
}

impl BetaPosterior {
    const fn prior() -> Self {
        Self {
            alpha: PRIOR_ALPHA,
            beta: PRIOR_BETA,
        }
    }

    fn mean(self) -> f64 {
        let s = self.alpha + self.beta;
        if s <= f64::EPSILON {
            PRIOR_ALPHA / (PRIOR_ALPHA + PRIOR_BETA)
        } else {
            self.alpha / s
        }
    }

    fn observe_conflict(&mut self) {
        self.alpha += 1.0;
    }

    fn observe_ok(&mut self) {
        self.beta += 1.0;
    }

    /// Shrink excess counts toward the prior each block.
    fn decay(&mut self) {
        self.alpha = 1.0 + (self.alpha - 1.0).max(0.0) * DECAY_LAMBDA;
        self.beta = 1.0 + (self.beta - 1.0).max(0.0) * DECAY_LAMBDA;
        // Floor near prior so maps stay informative but bounded.
        if self.alpha + self.beta < PRIOR_ALPHA + PRIOR_BETA {
            *self = Self::prior();
        }
    }
}

/// Inter-block Bayesian region model (persists on [`crate::Pevm`] like HeatMap).
#[derive(Debug, Default)]
pub(crate) struct BayesMap {
    locations: DashMap<MemoryLocationHash, BetaPosterior, BuildIdentityHasher>,
    accounts: DashMap<Address, BetaPosterior, BuildSuffixHasher>,
    wave_id: AtomicUsize,
    /// Sum of conflict posteriors at Wait decisions (for mean metric).
    wait_posterior_sum_bits: AtomicU64,
    wait_posterior_count: AtomicUsize,
    success_seen: DashSet<MemoryLocationHash, BuildIdentityHasher>,
    conflict_seen: DashSet<MemoryLocationHash, BuildIdentityHasher>,
}

impl BayesMap {
    pub(crate) fn new() -> Self {
        Self {
            locations: DashMap::default(),
            accounts: DashMap::default(),
            wave_id: AtomicUsize::new(0),
            wait_posterior_sum_bits: AtomicU64::new(0),
            wait_posterior_count: AtomicUsize::new(0),
            success_seen: DashSet::default(),
            conflict_seen: DashSet::default(),
        }
    }

    pub(crate) fn wave_id(&self) -> usize {
        self.wave_id.load(Ordering::Relaxed)
    }

    pub(crate) fn bump_wave(&self) -> usize {
        self.wave_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub(crate) fn has_location(&self, location: MemoryLocationHash) -> bool {
        self.locations.contains_key(&location)
    }

    #[allow(dead_code)]
    pub(crate) fn has_account(&self, address: &Address) -> bool {
        self.accounts.contains_key(address)
    }

    fn location_posterior(&self, location: MemoryLocationHash) -> BetaPosterior {
        self.locations
            .get(&location)
            .map(|e| *e)
            .unwrap_or_else(BetaPosterior::prior)
    }

    fn account_posterior(&self, address: &Address) -> BetaPosterior {
        self.accounts
            .get(address)
            .map(|e| *e)
            .unwrap_or_else(BetaPosterior::prior)
    }

    /// P(conflict) for a known location hash.
    pub(crate) fn prior_wait_probability(&self, location: MemoryLocationHash) -> f64 {
        self.location_posterior(location).mean()
    }

    /// P(conflict) for an account (cold-start / Basic-location proxy).
    pub(crate) fn account_wait_probability(&self, address: &Address) -> f64 {
        self.account_posterior(address).mean()
    }

    /// Decide Wait vs Speculate for a location, falling back to address posterior
    /// when the location has never been observed.
    pub(crate) fn decide(
        &self,
        location: MemoryLocationHash,
        address: Option<&Address>,
        tau: f64,
    ) -> RegionMode {
        let p = if self.has_location(location) {
            self.prior_wait_probability(location)
        } else if let Some(addr) = address {
            self.account_wait_probability(addr)
        } else {
            BetaPosterior::prior().mean()
        };
        if p >= tau {
            RegionMode::Wait
        } else {
            RegionMode::Speculate
        }
    }

    pub(crate) fn decide_account(&self, address: &Address, tau: f64) -> RegionMode {
        if self.account_wait_probability(address) >= tau {
            RegionMode::Wait
        } else {
            RegionMode::Speculate
        }
    }

    /// Record a Wait decision's posterior for the mean-waited metric.
    pub(crate) fn note_wait_decision(&self, location: MemoryLocationHash, address: Option<&Address>) {
        let p = if self.has_location(location) {
            self.prior_wait_probability(location)
        } else if let Some(addr) = address {
            self.account_wait_probability(addr)
        } else {
            BetaPosterior::prior().mean()
        };
        self.record_wait_posterior(p);
    }

    pub(crate) fn note_wait_decision_account(&self, address: &Address) {
        self.record_wait_posterior(self.account_wait_probability(address));
    }

    fn record_wait_posterior(&self, p: f64) {
        // Best-effort sum via CAS loop on bits-as-u64 float payload.
        let mut cur = self.wait_posterior_sum_bits.load(Ordering::Relaxed);
        loop {
            let next = (f64::from_bits(cur) + p).to_bits();
            match self.wait_posterior_sum_bits.compare_exchange_weak(
                cur,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(v) => cur = v,
            }
        }
        self.wait_posterior_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn take_mean_wait_posterior(&self) -> f64 {
        let count = self.wait_posterior_count.swap(0, Ordering::Relaxed);
        let sum = f64::from_bits(self.wait_posterior_sum_bits.swap(0, Ordering::Relaxed));
        if count == 0 {
            0.0
        } else {
            sum / count as f64
        }
    }

    /// Returns true if this was the first conflict observation for `location` this block.
    pub(crate) fn observe_conflict_location(&self, location: MemoryLocationHash) -> bool {
        if !self.conflict_seen.insert(location) {
            // Still bump α on validation aborts that call this repeatedly? No —
            // callers that need every abort should use observe_conflict_location_always.
            return false;
        }
        // First WW/touch observation in a block: +3 so P clears τ=0.30 from the
        // low-conflict prior without needing every WW incarnation to update α.
        self.locations
            .entry(location)
            .and_modify(|p| {
                p.observe_conflict();
                p.observe_conflict();
                p.observe_conflict();
            })
            .or_insert_with(|| {
                let mut p = BetaPosterior::prior();
                p.observe_conflict();
                p.observe_conflict();
                p.observe_conflict();
                p
            });
        self.evict_locations_if_needed();
        true
    }

    /// Always bump α (validation abort path — each abort is an observation).
    pub(crate) fn observe_conflict_location_always(&self, location: MemoryLocationHash) {
        let _ = self.conflict_seen.insert(location);
        self.locations
            .entry(location)
            .and_modify(|p| p.observe_conflict())
            .or_insert_with(|| {
                let mut p = BetaPosterior::prior();
                p.observe_conflict();
                p
            });
        self.evict_locations_if_needed();
    }

    pub(crate) fn observe_speculate_ok_location(&self, location: MemoryLocationHash) {
        // At most one success reinforcement per location per block.
        if !self.success_seen.insert(location) {
            return;
        }
        self.locations
            .entry(location)
            .and_modify(|p| p.observe_ok())
            .or_insert_with(|| {
                let mut p = BetaPosterior::prior();
                p.observe_ok();
                p
            });
        self.evict_locations_if_needed();
    }

    pub(crate) fn observe_conflict_account(&self, address: Address) {
        self.observe_conflict_account_n(address, 1);
    }

    pub(crate) fn observe_conflict_account_n(&self, address: Address, n: u32) {
        self.accounts
            .entry(address)
            .and_modify(|p| {
                for _ in 0..n {
                    p.observe_conflict();
                }
            })
            .or_insert_with(|| {
                let mut p = BetaPosterior::prior();
                for _ in 0..n {
                    p.observe_conflict();
                }
                p
            });
        self.evict_accounts_if_needed();
    }

    #[allow(dead_code)]
    pub(crate) fn observe_speculate_ok_account(&self, address: Address) {
        self.accounts
            .entry(address)
            .and_modify(|p| p.observe_ok())
            .or_insert_with(|| {
                let mut p = BetaPosterior::prior();
                p.observe_ok();
                p
            });
        self.evict_accounts_if_needed();
    }

    /// Soft inter-block decay so posteriors adapt across blocks.
    pub(crate) fn decay_block(&self) {
        for mut entry in self.locations.iter_mut() {
            entry.decay();
        }
        for mut entry in self.accounts.iter_mut() {
            entry.decay();
        }
        self.success_seen.clear();
        self.conflict_seen.clear();
    }

    pub(crate) fn reset(&self) {
        self.locations.clear();
        self.accounts.clear();
        self.success_seen.clear();
        self.conflict_seen.clear();
        self.wave_id.store(0, Ordering::Relaxed);
        self.wait_posterior_sum_bits.store(0, Ordering::Relaxed);
        self.wait_posterior_count.store(0, Ordering::Relaxed);
    }

    fn evict_locations_if_needed(&self) {
        let extra = self.locations.len().saturating_sub(MAX_ENTRIES);
        if extra == 0 {
            return;
        }
        let mut coldest: Vec<(MemoryLocationHash, f64)> = self
            .locations
            .iter()
            .map(|e| (*e.key(), e.mean()))
            .collect();
        coldest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (loc, _) in coldest.into_iter().take(extra) {
            self.locations.remove(&loc);
        }
    }

    fn evict_accounts_if_needed(&self) {
        let extra = self.accounts.len().saturating_sub(MAX_ENTRIES);
        if extra == 0 {
            return;
        }
        let mut coldest: Vec<(Address, f64)> = self
            .accounts
            .iter()
            .map(|e| (*e.key(), e.mean()))
            .collect();
        coldest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (addr, _) in coldest.into_iter().take(extra) {
            self.accounts.remove(&addr);
        }
    }
}
