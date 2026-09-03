//! Inter-block EWMA heat on cheap account hints (`from` / `to`).

use alloy_primitives::Address;
use dashmap::DashMap;

use crate::BuildSuffixHasher;

/// Smoothing for `heat = α * 1 + (1-α) * heat`.
const ALPHA: f64 = 0.6;
/// Accounts at or above this score start the next block in Wait.
const HOT_THRESHOLD: f64 = 0.35;
/// Cap map size so a noisy block cannot grow heat without bound.
const MAX_ENTRIES: usize = 4096;

/// Thread-safe, resettable inter-block heat map.
#[derive(Debug, Default)]
pub(crate) struct HeatMap {
    scores: DashMap<Address, f64, BuildSuffixHasher>,
}

impl HeatMap {
    pub(crate) fn new() -> Self {
        Self {
            scores: DashMap::default(),
        }
    }

    /// Bump an account that was contended or multi-written in this block.
    pub(crate) fn observe(&self, address: Address) {
        self.scores
            .entry(address)
            .and_modify(|heat| *heat = ALPHA + (1.0 - ALPHA) * *heat)
            .or_insert(ALPHA);
        self.evict_if_needed();
    }

    pub(crate) fn is_hot(&self, address: &Address) -> bool {
        self.scores
            .get(address)
            .is_some_and(|heat| *heat >= HOT_THRESHOLD)
    }

    pub(crate) fn reset(&self) {
        self.scores.clear();
    }

    fn evict_if_needed(&self) {
        let extra = self.scores.len().saturating_sub(MAX_ENTRIES);
        if extra == 0 {
            return;
        }
        let mut coldest: Vec<(Address, f64)> = self
            .scores
            .iter()
            .map(|entry| (*entry.key(), *entry.value()))
            .collect();
        coldest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (address, _) in coldest.into_iter().take(extra) {
            self.scores.remove(&address);
        }
    }
}
