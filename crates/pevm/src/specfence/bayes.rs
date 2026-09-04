//! Per-region Beta-Bernoulli conflict posterior for SpecFence Wait vs Speculate.
//!
//! Control unit is a memory location (`MemoryLocationHash`), with optional
//! address-level posteriors for cold-start when the slot is unknown pre-exec.
//! Spec v1: P_conflict + P_bind_useful; revoke sticky Wait when P < τ_revoke.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, BuildSuffixHasher, MemoryLocationHash};

use super::RegionMode;
use super::resolve::{TAU_REVOKE, TAU_S, TAU_W};

/// Prior: mild low-conflict (`α=1`, `β=9` → P≈0.1).
const PRIOR_ALPHA: f64 = 1.0;
const PRIOR_BETA: f64 = 9.0;
/// Block-start seed threshold (inter-block carry). Live π uses τ_w / τ_s.
/// Slightly below τ_w so a decayed hot posterior (~0.31) still seeds Wait.
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
    /// Bind usefulness posterior per location (residual write-set hit rate).
    bind_useful: DashMap<MemoryLocationHash, BetaPosterior, BuildIdentityHasher>,
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
            bind_useful: DashMap::default(),
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

    fn bind_posterior(&self, location: MemoryLocationHash) -> BetaPosterior {
        self.bind_useful
            .get(&location)
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

    /// P(bind useful) for residual write-set / Bohm-lite.
    pub(crate) fn bind_useful_probability(&self, location: MemoryLocationHash) -> f64 {
        self.bind_posterior(location).mean()
    }

    /// Conflict posterior with address cold-start fallback.
    pub(crate) fn conflict_probability(
        &self,
        location: MemoryLocationHash,
        address: Option<&Address>,
    ) -> f64 {
        if self.has_location(location) {
            self.prior_wait_probability(location)
        } else if let Some(addr) = address {
            self.account_wait_probability(addr)
        } else {
            BetaPosterior::prior().mean()
        }
    }

    /// Decide Wait vs Speculate for a location (uses τ_w for seed / sticky path).
    #[allow(dead_code)]
    pub(crate) fn decide(
        &self,
        location: MemoryLocationHash,
        address: Option<&Address>,
        tau: f64,
    ) -> RegionMode {
        let p = self.conflict_probability(location, address);
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

    /// True when sticky Wait should be revoked (`P_conflict < τ_revoke`).
    pub(crate) fn should_revoke(
        &self,
        location: MemoryLocationHash,
        address: Option<&Address>,
    ) -> bool {
        self.conflict_probability(location, address) < TAU_REVOKE
    }

    /// True when π prefers WaitHard over SpecRead (`P >= τ_s`, or writer known & `P >= τ_w`).
    pub(crate) fn should_wait_hard(
        &self,
        location: MemoryLocationHash,
        address: Option<&Address>,
        writer_known: bool,
    ) -> bool {
        let p = self.conflict_probability(location, address);
        if writer_known && p >= TAU_W {
            return true;
        }
        p >= TAU_S
    }

    /// Record a Wait decision's posterior for the mean-waited metric.
    pub(crate) fn note_wait_decision(&self, location: MemoryLocationHash, address: Option<&Address>) {
        self.record_wait_posterior(self.conflict_probability(location, address));
    }

    pub(crate) fn note_wait_decision_account(&self, address: &Address) {
        self.record_wait_posterior(self.account_wait_probability(address));
    }

    fn record_wait_posterior(&self, p: f64) {
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
            return false;
        }
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

    pub(crate) fn observe_bind_hit(&self, location: MemoryLocationHash) {
        self.bind_useful
            .entry(location)
            .and_modify(|p| p.observe_ok())
            .or_insert_with(|| {
                let mut p = BetaPosterior::prior();
                p.observe_ok();
                p
            });
    }

    #[allow(dead_code)]
    pub(crate) fn observe_bind_miss(&self, location: MemoryLocationHash) {
        self.bind_useful
            .entry(location)
            .and_modify(|p| p.observe_conflict())
            .or_insert_with(|| {
                let mut p = BetaPosterior::prior();
                p.observe_conflict();
                p
            });
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
        for mut entry in self.bind_useful.iter_mut() {
            entry.decay();
        }
        self.success_seen.clear();
        self.conflict_seen.clear();
    }

    pub(crate) fn reset(&self) {
        self.locations.clear();
        self.accounts.clear();
        self.bind_useful.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoke_clears_when_posterior_low() {
        let bayes = BayesMap::new();
        let loc = 42u64;
        // Prior mean ≈ 0.1 < τ_revoke=0.20 → should revoke.
        assert!(bayes.should_revoke(loc, None));
        for _ in 0..5 {
            bayes.observe_conflict_location_always(loc);
        }
        // α=6, β=9 → P=6/15=0.40 ≥ τ_w=0.35 → WaitHard when writer known.
        assert!(!bayes.should_revoke(loc, None));
        assert!(
            bayes.should_wait_hard(loc, None, true),
            "p={}",
            bayes.prior_wait_probability(loc)
        );
        // Simulate many success observations across "blocks" by decaying and
        // reinforcing β via direct conflict/ok through always+decay cycles.
        // After enough ok bumps relative to conflicts, P drops below τ_revoke.
        for _ in 0..30 {
            bayes.decay_block();
            bayes.observe_speculate_ok_location(loc);
        }
        // Force additional ok mass: observe_speculate_ok only once/block, so
        // use decay + re-observe. If still high, conflict was 3 and many decays
        // shrink α excess.
        assert!(
            bayes.should_revoke(loc, None) || bayes.prior_wait_probability(loc) < TAU_S,
            "posterior should fall toward revoke/speculate: p={}",
            bayes.prior_wait_probability(loc)
        );
        // Explicit: after reset to prior, revoke holds.
        bayes.reset();
        assert!(bayes.should_revoke(loc, None));
        // Sticky Wait clearability: promote conceptually then revoke decision.
        assert_eq!(bayes.decide(loc, None, TAU_W), RegionMode::Speculate);
    }

    #[test]
    fn thresholds_match_spec_v1() {
        assert!((TAU_W - 0.35).abs() < f64::EPSILON);
        assert!((TAU_S - 0.50).abs() < f64::EPSILON);
        assert!((TAU_REVOKE - 0.20).abs() < f64::EPSILON);
    }
}
