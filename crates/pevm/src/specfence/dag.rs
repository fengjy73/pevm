//! FenceGraph (P2) — SoftWait / BindTarget / revoke / wake.
//!
//! SoftWait is the **source of truth** for SpecFence scheduling fences.
//! `RegionTable` location Wait bits and legacy wait-flags are mirrors/facades.
//! WaveParkTable remains the M2 tx-grain park executor.
//!
//! Evolved from Spec v1 SpecDag; `SpecDag` is a type alias for compatibility.

#![allow(dead_code)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use dashmap::{DashMap, DashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

use super::RegionMode;

/// Edge kind in Ĝ (optional bookkeeping).
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

/// SoftWait arm: do not progress `waiter` past observe `k` until writer Data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SoftWaitArm {
    pub waiter: TxIdx,
    pub armed_at_k: u64,
    pub expected_writer: Option<TxIdx>,
    /// Monotonic arm id for revoke bookkeeping.
    pub arm_id: u64,
}

/// Explicit FenceGraph: SoftWait lifecycle + wait-flag mirrors.
#[derive(Debug, Default)]
pub(crate) struct FenceGraph {
    /// SoftWait arms: ℓ → [{waiter, k, writer?}]. **Source of truth.**
    soft_waits:
        DashMap<MemoryLocationHash, Vec<SoftWaitArm>, BuildIdentityHasher>,
    /// Mirror: locations currently under WaitHard (sticky but revokeable).
    wait_locations: DashMap<MemoryLocationHash, (), BuildIdentityHasher>,
    /// Optional hard/soft edge count (metrics).
    edge_count: AtomicUsize,
    /// SoftWait arm counter.
    arm_seq: AtomicUsize,
    /// SoftWait arms created this block.
    soft_arm_count: AtomicUsize,
    /// SoftWait clears / revokes this block.
    soft_revoke_count: AtomicUsize,
    /// Wake notifications this block.
    wake_count: AtomicUsize,
    /// Txs whose known hard waits are clear (ready hint).
    ready_hints: DashSet<TxIdx, BuildIdentityHasher>,
    /// Arm timestamps (best-effort revoke age).
    arm_started: DashMap<u64, Instant, BuildIdentityHasher>,
}

/// Backward-compatible name used across pevm / SpecFenceCtx.
pub(crate) type SpecDag = FenceGraph;

impl FenceGraph {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    // --- SoftWait source of truth ---

    /// Arm SoftWait(ℓ, waiter_t, k). Returns true if newly armed for this waiter.
    pub(crate) fn arm_soft(
        &self,
        location: MemoryLocationHash,
        waiter: TxIdx,
        k: u64,
        expected_writer: Option<TxIdx>,
    ) -> bool {
        let arm_id = self.arm_seq.fetch_add(1, Ordering::Relaxed) as u64 + 1;
        let arm = SoftWaitArm {
            waiter,
            armed_at_k: k,
            expected_writer,
            arm_id,
        };
        let mut newly = true;
        self.soft_waits
            .entry(location)
            .and_modify(|v| {
                if let Some(existing) = v.iter_mut().find(|a| a.waiter == waiter) {
                    *existing = arm;
                    newly = false;
                } else {
                    v.push(arm);
                }
            })
            .or_insert_with(|| vec![arm]);
        // Mirror sticky Wait bit.
        self.wait_locations.insert(location, ());
        if newly {
            self.soft_arm_count.fetch_add(1, Ordering::Relaxed);
            self.edge_count.fetch_add(1, Ordering::Relaxed);
            self.arm_started.insert(arm_id, Instant::now());
        }
        newly
    }

    /// Clear / revoke all SoftWaits on ℓ. Returns number of arms cleared.
    pub(crate) fn clear(&self, location: MemoryLocationHash) -> usize {
        let n = if let Some((_, arms)) = self.soft_waits.remove(&location) {
            for a in &arms {
                self.arm_started.remove(&a.arm_id);
            }
            arms.len()
        } else {
            0
        };
        let mirrored = self.wait_locations.remove(&location).is_some();
        if n > 0 || mirrored {
            self.soft_revoke_count.fetch_add(n.max(1), Ordering::Relaxed);
        }
        n
    }

    /// Revoke SoftWait for one waiter on ℓ.
    pub(crate) fn clear_waiter(&self, location: MemoryLocationHash, waiter: TxIdx) -> bool {
        let mut cleared = false;
        let mut empty = false;
        if let Some(mut v) = self.soft_waits.get_mut(&location) {
            let before = v.len();
            v.retain(|a| {
                if a.waiter == waiter {
                    self.arm_started.remove(&a.arm_id);
                    false
                } else {
                    true
                }
            });
            cleared = v.len() < before;
            empty = v.is_empty();
        }
        if empty {
            self.soft_waits.remove(&location);
            self.wait_locations.remove(&location);
        }
        if cleared {
            self.soft_revoke_count.fetch_add(1, Ordering::Relaxed);
        }
        cleared
    }

    /// Publish Data on ℓ by `writer_t`: clear SoftWaits with waiter > writer.
    /// Returns waiter tx indices to unpark (caller drives WavePark).
    pub(crate) fn wake_on_publish(
        &self,
        location: MemoryLocationHash,
        writer_t: TxIdx,
    ) -> Vec<TxIdx> {
        let mut woken = Vec::new();
        let mut keep = Vec::new();
        if let Some(mut v) = self.soft_waits.get_mut(&location) {
            for a in v.drain(..) {
                let match_writer = a
                    .expected_writer
                    .map(|w| w == writer_t)
                    .unwrap_or(true);
                if match_writer && a.waiter > writer_t {
                    self.arm_started.remove(&a.arm_id);
                    if !woken.contains(&a.waiter) {
                        woken.push(a.waiter);
                    }
                } else {
                    keep.push(a);
                }
            }
            *v = keep;
        }
        if self
            .soft_waits
            .get(&location)
            .is_none_or(|v| v.is_empty())
        {
            self.soft_waits.remove(&location);
            self.wait_locations.remove(&location);
        }
        if !woken.is_empty() {
            self.wake_count.fetch_add(woken.len(), Ordering::Relaxed);
        }
        woken
    }

    /// Clear SoftWaits armed against `writer` (Publish / finish_execution wake).
    pub(crate) fn clear_for_writer(&self, writer: TxIdx) -> Vec<(MemoryLocationHash, TxIdx)> {
        let mut cleared = Vec::new();
        let keys: Vec<MemoryLocationHash> = self.soft_waits.iter().map(|e| *e.key()).collect();
        for loc in keys {
            let woken = self.wake_on_publish(loc, writer);
            for w in woken {
                cleared.push((loc, w));
            }
        }
        cleared
    }

    /// Iterate SoftWait waiters on ℓ.
    pub(crate) fn iter_waiters(&self, location: MemoryLocationHash) -> Vec<SoftWaitArm> {
        self.soft_waits
            .get(&location)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub(crate) fn has_soft_wait(&self, location: MemoryLocationHash) -> bool {
        self.soft_waits
            .get(&location)
            .is_some_and(|v| !v.is_empty())
    }

    pub(crate) fn soft_arm_count(&self) -> usize {
        self.soft_arm_count.load(Ordering::Relaxed)
    }

    pub(crate) fn soft_revoke_count(&self) -> usize {
        self.soft_revoke_count.load(Ordering::Relaxed)
    }

    pub(crate) fn wake_count(&self) -> usize {
        self.wake_count.load(Ordering::Relaxed)
    }

    // --- Legacy SpecDag facade (mirrors SoftWait) ---

    pub(crate) fn set_wait(&self, location: MemoryLocationHash) -> bool {
        self.wait_locations.insert(location, ()).is_none()
    }

    pub(crate) fn clear_wait(&self, location: MemoryLocationHash) -> bool {
        let soft = self.clear(location);
        soft > 0 || self.wait_locations.remove(&location).is_some()
    }

    pub(crate) fn is_wait(&self, location: MemoryLocationHash) -> bool {
        self.has_soft_wait(location) || self.wait_locations.contains_key(&location)
    }

    pub(crate) fn mode(&self, location: MemoryLocationHash) -> RegionMode {
        if self.is_wait(location) {
            RegionMode::Wait
        } else {
            RegionMode::Speculate
        }
    }

    /// Legacy: register SoftWait without k (k=0). Prefer [`arm_soft`].
    pub(crate) fn note_soft_wait(&self, location: MemoryLocationHash, waiter: TxIdx) {
        let _ = self.arm_soft(location, waiter, 0, None);
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
        self.iter_waiters(location)
            .into_iter()
            .map(|a| a.waiter)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_soft_wake_on_publish() {
        let g = FenceGraph::new();
        assert!(g.arm_soft(1, 5, 3, Some(2)));
        assert!(g.has_soft_wait(1));
        assert!(g.is_wait(1));
        let woken = g.wake_on_publish(1, 2);
        assert_eq!(woken, vec![5]);
        assert!(!g.has_soft_wait(1));
    }

    #[test]
    fn clear_revokes_soft_wait() {
        let g = FenceGraph::new();
        g.arm_soft(9, 1, 0, None);
        g.arm_soft(9, 2, 0, None);
        assert_eq!(g.clear(9), 2);
        assert!(!g.is_wait(9));
        assert!(g.soft_revoke_count() >= 1);
    }

    #[test]
    fn iter_waiters_lists_arms() {
        let g = FenceGraph::new();
        g.arm_soft(3, 10, 7, Some(4));
        let arms = g.iter_waiters(3);
        assert_eq!(arms.len(), 1);
        assert_eq!(arms[0].waiter, 10);
        assert_eq!(arms[0].armed_at_k, 7);
    }
}
