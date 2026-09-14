//! Lightweight AccessOrdinalLog — Spec-safe \((\ell,k)\) without rem journal.
//!
//! Plant SoT v6 §5/§6: HashMap first_k **only when PE nonempty or learning
//! arm**. Quiet empty-PE must not call [`AccessOrdinalLog::note`].
//! Single-executor per `tx_idx` (same invariant as rem `state_mut`).

use std::cell::UnsafeCell;

use hashbrown::HashMap;

use crate::{BuildSuffixHasher, MemoryLocationHash, TxIdx};

struct TxOrdinals {
    k: u32,
    first: HashMap<MemoryLocationHash, u32, BuildSuffixHasher>,
}

impl TxOrdinals {
    fn new() -> Self {
        Self {
            k: 0,
            first: HashMap::with_hasher(BuildSuffixHasher::default()),
        }
    }

    fn clear(&mut self) {
        self.k = 0;
        self.first.clear();
    }
}

/// Per-block access ordinal log. Not a rem certificate; Spec and Fence both write.
pub(crate) struct AccessOrdinalLog {
    slots: Vec<UnsafeCell<TxOrdinals>>,
}

impl std::fmt::Debug for AccessOrdinalLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessOrdinalLog")
            .field("n", &self.slots.len())
            .finish()
    }
}

unsafe impl Sync for AccessOrdinalLog {}

impl AccessOrdinalLog {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            slots: (0..block_size)
                .map(|_| UnsafeCell::new(TxOrdinals::new()))
                .collect(),
        }
    }

    /// New incarnation — drop the previous \((\ell,k)\) map.
    #[inline]
    pub(crate) fn begin_incarnation(&self, tx_idx: TxIdx) {
        if let Some(slot) = self.slots.get(tx_idx) {
            // SAFETY: one executor owns `tx_idx` at a time.
            unsafe { &mut *slot.get() }.clear();
        }
    }

    /// Record this program read; return the incarnation-local ordinal \(k\).
    #[inline]
    pub(crate) fn note(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> u32 {
        let Some(slot) = self.slots.get(tx_idx) else {
            return 0;
        };
        // SAFETY: one executor owns `tx_idx` at a time.
        let st = unsafe { &mut *slot.get() };
        st.k = st.k.saturating_add(1);
        st.first.entry(location).or_insert(st.k);
        st.k
    }

    /// First \(k\) this incarnation touched \(\ell\) — abort PE train (never residual 1).
    #[inline]
    pub(crate) fn first_k(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> Option<u32> {
        let slot = self.slots.get(tx_idx)?;
        // SAFETY: validate of `tx_idx` does not race an executor on the same slot
        // after publish (incarnation finished).
        unsafe { &*slot.get() }.first.get(&location).copied()
    }

    /// First-touches with \(k < before_k\) — WaitForDependency rem prefix (completed reads).
    #[inline]
    pub(crate) fn prefix_before(
        &self,
        tx_idx: TxIdx,
        before_k: u32,
    ) -> Vec<(MemoryLocationHash, u32)> {
        let Some(slot) = self.slots.get(tx_idx) else {
            return Vec::new();
        };
        // SAFETY: waiter still owns this incarnation (WaitForDependency, not yet reset).
        let st = unsafe { &*slot.get() };
        st.first
            .iter()
            .filter(|&(_, &k)| k > 0 && k < before_k)
            .map(|(&loc, &k)| (loc, k))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_true_k_not_residual_one() {
        let log = AccessOrdinalLog::new(2);
        assert_eq!(log.note(0, 7), 1);
        assert_eq!(log.note(0, 8), 2);
        assert_eq!(log.note(0, 7), 3);
        assert_eq!(log.first_k(0, 7), Some(1));
        assert_eq!(log.first_k(0, 8), Some(2));
        log.begin_incarnation(0);
        assert_eq!(log.first_k(0, 7), None);
        assert_eq!(log.note(0, 7), 1);
    }

    #[test]
    fn prefix_before_excludes_fail_k() {
        let log = AccessOrdinalLog::new(1);
        assert_eq!(log.note(0, 7), 1);
        assert_eq!(log.note(0, 8), 2);
        assert_eq!(log.note(0, 9), 3);
        let p = log.prefix_before(0, 3);
        assert_eq!(p.len(), 2);
        assert!(p.contains(&(7, 1)));
        assert!(p.contains(&(8, 2)));
        assert!(!p.iter().any(|(l, _)| *l == 9));
    }
}
