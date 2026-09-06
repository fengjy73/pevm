//! Plant v2 M3 — online WŜ / RŜ learning for Bind-before-touch.
//!
//! Learning ∉ TCB: wrong priors only change π (more SpecRead / Wait / validate
//! fail / repair). Sequential equivalence still holds via validate.
//!
//! Structures:
//! - Process-local write / co-access frequencies per location (inter-block).
//! - Optional account → hot write-locations (cold-start proxy).
//! Block-local WŜ(t) is published into MvMemory::residual_write_sets on every
//! successful `record` (and still on abort), so later txs / reincarnations see
//! predicted writers without waiting for another abort.

use std::sync::atomic::{AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use crate::{BuildIdentityHasher, BuildSuffixHasher, MemoryLocationHash};

/// Minimum write observations before a location is treated as a prior Bind hint.
const WRITE_HIT_FLOOR: u32 = 1;
/// Soft cap so decay stays meaningful.
const MAX_ENTRIES: usize = 8192;
/// Per-block decay of counters toward zero.
const DECAY_NUM: u32 = 19;
const DECAY_DEN: u32 = 20;

#[derive(Debug, Clone, Copy, Default)]
struct LocStats {
    /// Times a completed incarnation wrote this location.
    writes: u32,
    /// Times a reader saw / predicted a lower writer on this location.
    co_access: u32,
}

/// Process-persistent online RW prior (lives on [`crate::Pevm`] like BayesMap).
#[derive(Debug, Default)]
pub(crate) struct RwPriorMap {
    locations: DashMap<MemoryLocationHash, LocStats, BuildIdentityHasher>,
    /// Account-level: locations commonly written when this account is hot.
    account_writes: DashMap<Address, Vec<MemoryLocationHash>, BuildSuffixHasher>,
    /// Diagnostics: how many locations currently above floor.
    hot_writes: AtomicUsize,
}

impl RwPriorMap {
    pub(crate) fn new() -> Self {
        Self {
            locations: DashMap::default(),
            account_writes: DashMap::default(),
            hot_writes: AtomicUsize::new(0),
        }
    }

    /// True when process prior believes ℓ is frequently written (Bind/Wait hint).
    pub(crate) fn predicts_write(&self, location: MemoryLocationHash) -> bool {
        self.locations
            .get(&location)
            .is_some_and(|s| s.writes >= WRITE_HIT_FLOOR)
    }

    /// Confidence in [0,1] from write + co-access mass (for π biasing).
    pub(crate) fn write_confidence(&self, location: MemoryLocationHash) -> f64 {
        let Some(s) = self.locations.get(&location) else {
            return 0.0;
        };
        let mass = (s.writes as f64) + 0.5 * (s.co_access as f64);
        (mass / (mass + 3.0)).clamp(0.0, 1.0)
    }

    /// Observe a completed incarnation's write-set (success or abort residual).
    pub(crate) fn observe_write_set(
        &self,
        writes: &[MemoryLocationHash],
        account: Option<Address>,
    ) {
        for &loc in writes {
            self.locations
                .entry(loc)
                .and_modify(|s| s.writes = s.writes.saturating_add(1))
                .or_insert(LocStats {
                    writes: 1,
                    co_access: 0,
                });
        }
        if let Some(addr) = account {
            if writes.is_empty() {
                return;
            }
            self.account_writes
                .entry(addr)
                .and_modify(|v| {
                    for &loc in writes {
                        if !v.contains(&loc) {
                            v.push(loc);
                        }
                    }
                    // Cap per-account list.
                    if v.len() > 64 {
                        let drop = v.len() - 64;
                        v.drain(0..drop);
                    }
                })
                .or_insert_with(|| writes.to_vec());
        }
        self.evict_if_needed();
        self.refresh_hot_count();
    }

    /// Observe that a reader predicted / hit a lower writer on ℓ (co-access).
    pub(crate) fn observe_co_access(&self, location: MemoryLocationHash) {
        self.locations
            .entry(location)
            .and_modify(|s| s.co_access = s.co_access.saturating_add(1))
            .or_insert(LocStats {
                writes: 0,
                co_access: 1,
            });
    }

    /// Account-level prior: any known hot write location for this account.
    #[allow(dead_code)]
    pub(crate) fn account_predicted_writes(&self, address: &Address) -> Vec<MemoryLocationHash> {
        self.account_writes
            .get(address)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Soft decay after each block (Spec v1 process-local prior).
    pub(crate) fn decay_block(&self) {
        for mut entry in self.locations.iter_mut() {
            // Keep a floor of 1 write once learned so next block still Bind-hints.
            let w = (entry.writes * DECAY_NUM) / DECAY_DEN;
            entry.writes = if entry.writes > 0 { w.max(1) } else { 0 };
            let c = (entry.co_access * DECAY_NUM) / DECAY_DEN;
            entry.co_access = if entry.co_access > 0 { c.max(0) } else { 0 };
        }
        self.locations.retain(|_, s| s.writes > 0 || s.co_access > 0);
        self.refresh_hot_count();
    }

    pub(crate) fn reset(&self) {
        self.locations.clear();
        self.account_writes.clear();
        self.hot_writes.store(0, Ordering::Relaxed);
    }

    pub(crate) fn hot_write_count(&self) -> usize {
        self.hot_writes.load(Ordering::Relaxed)
    }

    fn refresh_hot_count(&self) {
        let n = self
            .locations
            .iter()
            .filter(|e| e.writes >= WRITE_HIT_FLOOR)
            .count();
        self.hot_writes.store(n, Ordering::Relaxed);
    }

    fn evict_if_needed(&self) {
        let extra = self.locations.len().saturating_sub(MAX_ENTRIES);
        if extra == 0 {
            return;
        }
        let mut coldest: Vec<(MemoryLocationHash, u32)> = self
            .locations
            .iter()
            .map(|e| (*e.key(), e.writes + e.co_access))
            .collect();
        coldest.sort_by_key(|(_, m)| *m);
        for (loc, _) in coldest.into_iter().take(extra) {
            self.locations.remove(&loc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_observation_enables_predict() {
        let p = RwPriorMap::new();
        let loc = 99u64;
        assert!(!p.predicts_write(loc));
        p.observe_write_set(&[loc], None);
        assert!(p.predicts_write(loc));
        assert!(p.write_confidence(loc) > 0.0);
    }

    #[test]
    fn decay_keeps_floor_once_learned() {
        let p = RwPriorMap::new();
        let loc = 7u64;
        p.observe_write_set(&[loc], None);
        for _ in 0..80 {
            p.decay_block();
        }
        // Floor keeps a Bind hint across blocks (confidence may shrink via mass).
        assert!(p.predicts_write(loc));
    }
}
