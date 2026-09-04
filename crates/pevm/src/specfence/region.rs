//! Per-region (memory location / account) Wait vs Speculate mode for the current block.
//! Spec v1: sticky Wait is revokeable when posterior < τ_revoke.

use alloy_primitives::Address;
use dashmap::DashMap;

use crate::{BuildIdentityHasher, BuildSuffixHasher, MemoryLocationHash};

/// Concurrency mode of one region for the rest of the current block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionMode {
    /// Optimistic read/write; validate and abort on conflict.
    Speculate,
    /// Later accessors wait for the last lower-idx writer (PCC).
    Wait,
}

/// Per-block table: each memory location and each account is Speculate or Wait.
#[derive(Debug)]
pub(crate) struct RegionTable {
    locations: DashMap<MemoryLocationHash, RegionMode, BuildIdentityHasher>,
    accounts: DashMap<Address, RegionMode, BuildSuffixHasher>,
}

impl RegionTable {
    pub(crate) fn new() -> Self {
        Self {
            locations: DashMap::default(),
            accounts: DashMap::default(),
        }
    }

    pub(crate) fn location_mode(&self, location: MemoryLocationHash) -> RegionMode {
        self.locations
            .get(&location)
            .map(|m| *m)
            .unwrap_or(RegionMode::Speculate)
    }

    pub(crate) fn account_mode(&self, address: &Address) -> RegionMode {
        self.accounts
            .get(address)
            .map(|m| *m)
            .unwrap_or(RegionMode::Speculate)
    }

    /// Location or its owning account is Wait.
    #[allow(dead_code)]
    pub(crate) fn should_wait(&self, location: MemoryLocationHash, address: &Address) -> bool {
        self.location_mode(location) == RegionMode::Wait
            || self.account_mode(address) == RegionMode::Wait
    }

    /// Promote a memory location Speculate → Wait. Returns true if this was a new promotion.
    pub(crate) fn promote_location(&self, location: MemoryLocationHash) -> bool {
        match self.locations.insert(location, RegionMode::Wait) {
            None | Some(RegionMode::Speculate) => true,
            Some(RegionMode::Wait) => false,
        }
    }

    /// Revoke sticky Wait → Speculate. Returns true if a Wait was cleared.
    pub(crate) fn clear_location_wait(&self, location: MemoryLocationHash) -> bool {
        match self.locations.insert(location, RegionMode::Speculate) {
            Some(RegionMode::Wait) => true,
            _ => false,
        }
    }

    /// Promote an account Speculate → Wait. Returns true if this was a new promotion.
    pub(crate) fn promote_account(&self, address: Address) -> bool {
        match self.accounts.insert(address, RegionMode::Wait) {
            None | Some(RegionMode::Speculate) => true,
            Some(RegionMode::Wait) => false,
        }
    }

    /// Revoke account-level Wait.
    pub(crate) fn clear_account_wait(&self, address: Address) -> bool {
        match self.accounts.insert(address, RegionMode::Speculate) {
            Some(RegionMode::Wait) => true,
            _ => false,
        }
    }

    /// Seed an account as Wait at block start (heat / PCC).
    pub(crate) fn seed_account_wait(&self, address: Address) {
        self.accounts.insert(address, RegionMode::Wait);
    }
}

impl Default for RegionTable {
    fn default() -> Self {
        Self::new()
    }
}
