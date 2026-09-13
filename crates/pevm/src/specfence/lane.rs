//! SerialLane / OrderedAdmit **progress tokens** — not prefer_admit+Spec.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` §3.4.
//!
//! Token is held by the earliest unfinished producer (or admitted head).
//! Consumers are blocked in the ready-set or park via WaitFor — they must
//! **not** `occ_unfenced` while the holder is live. CC lane tokens co-own
//! PC ready membership.

use std::sync::atomic::{AtomicUsize, Ordering};

use dashmap::DashMap;
use rustc_hash::FxBuildHasher;

use crate::{MemoryLocationHash, TxIdx};

use super::edge::access_k_class;

/// Exclusive Execute permit on \((\ell, k_{\mathrm{class}})\).
#[derive(Debug, Default)]
pub(crate) struct LaneTable {
    holders: DashMap<(MemoryLocationHash, u8), AtomicUsize, FxBuildHasher>,
}

impl LaneTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Grant the token to `writer` (earliest unfinished / admitted head).
    #[inline]
    pub(crate) fn grant(&self, location: MemoryLocationHash, k: u32, writer: TxIdx) {
        if k == 0 {
            return;
        }
        self.holders
            .entry((location, access_k_class(k)))
            .or_insert_with(|| AtomicUsize::new(writer))
            .store(writer, Ordering::Release);
    }

    #[inline]
    pub(crate) fn holder(&self, location: MemoryLocationHash, k: u32) -> Option<TxIdx> {
        if k == 0 {
            return None;
        }
        self.holders
            .get(&(location, access_k_class(k)))
            .map(|e| e.load(Ordering::Acquire))
            .filter(|&w| w != usize::MAX)
    }

    #[inline]
    pub(crate) fn is_head(&self, tx_idx: TxIdx, location: MemoryLocationHash, k: u32) -> bool {
        self.holder(location, k) == Some(tx_idx)
    }

    /// Release when the holder is Done / published Data.
    #[inline]
    pub(crate) fn release(&self, location: MemoryLocationHash, k: u32, writer: TxIdx) {
        if k == 0 {
            return;
        }
        if let Some(e) = self.holders.get(&(location, access_k_class(k)))
            && e.load(Ordering::Relaxed) == writer
        {
            e.store(usize::MAX, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_is_exclusive_head() {
        let t = LaneTable::new();
        t.grant(7, 6, 1);
        assert!(t.is_head(1, 7, 6));
        assert!(!t.is_head(3, 7, 6));
        t.release(7, 6, 1);
        assert!(t.holder(7, 6).is_none());
    }
}
