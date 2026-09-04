//! Minimal speculative dependency graph Ĝ for SpecFence Spec v1.
//!
//! Phase-1 maintains revokeable wait-flags per location and ready hints.
//! Hard/soft edges are optional bookkeeping for later wave scheduling.

#![allow(dead_code)]
use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::RegionMode;

/// Edge kind in Ĝ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeKind {
    HardWr,
    HardWw,
    SoftWr,
    SoftWw,
}

/// One speculative edge `from → to` on a location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DagEdge {
    pub from: TxIdx,
    pub to: TxIdx,
    pub location: MemoryLocationHash,
    pub kind: EdgeKind,
}

/// Minimal Ĝ: revokeable Wait flags + optional edges + ready hints.
#[derive(Debug, Default)]
pub(crate) struct SpecDag {
    /// Locations currently under WaitHard (sticky but revokeable).
    wait_locations: DashMap<MemoryLocationHash, (), BuildIdentityHasher>,
    /// Soft WR edges registered for revoke bookkeeping.
    soft_waits: DashMap<MemoryLocationHash, DashSet<TxIdx, BuildIdentityHasher>, BuildIdentityHasher>,
    /// Optional hard/soft edge count (metrics / Phase-2 prep).
    edge_count: AtomicUsize,
    /// Txs whose known hard waits are clear (ready hint).
    ready_hints: DashSet<TxIdx, BuildIdentityHasher>,
}

impl SpecDag {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set_wait(&self, location: MemoryLocationHash) -> bool {
        self.wait_locations.insert(location, ()).is_none()
    }

    pub(crate) fn clear_wait(&self, location: MemoryLocationHash) -> bool {
        self.wait_locations.remove(&location).is_some()
    }

    pub(crate) fn is_wait(&self, location: MemoryLocationHash) -> bool {
        self.wait_locations.contains_key(&location)
    }

    pub(crate) fn mode(&self, location: MemoryLocationHash) -> RegionMode {
        if self.is_wait(location) {
            RegionMode::Wait
        } else {
            RegionMode::Speculate
        }
    }

    pub(crate) fn note_soft_wait(&self, location: MemoryLocationHash, waiter: TxIdx) {
        self.soft_waits
            .entry(location)
            .or_default()
            .insert(waiter);
        self.edge_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_hard_edge(&self) {
        self.edge_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn mark_ready(&self, tx_idx: TxIdx) {
        self.ready_hints.insert(tx_idx);
    }

    pub(crate) fn clear_ready(&self, tx_idx: TxIdx) {
        self.ready_hints.remove(&tx_idx);
    }

    pub(crate) fn is_ready_hint(&self, tx_idx: TxIdx) -> bool {
        self.ready_hints.contains(&tx_idx)
    }

    pub(crate) fn edge_count(&self) -> usize {
        self.edge_count.load(Ordering::Relaxed)
    }

    /// Soft waiters registered on `location` (for wake / revoke).
    pub(crate) fn soft_waiters(&self, location: MemoryLocationHash) -> Vec<TxIdx> {
        self.soft_waits
            .get(&location)
            .map(|s| s.iter().map(|e| *e).collect())
            .unwrap_or_default()
    }
}
