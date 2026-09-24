//! Debug rem-legal mirror — **not** the v6 certificate SoT.
//!
//! Protocol: `lab/notes/specfence-complete-architecture-v6-essence.md`.
//! Strips live in [`crate::specfence::CertificateTable`]. This table only
//! mirrors "any successful Fence this incarnation" for rem journal legality.
//! Frozen π is unchanged. `inc` is not an Avoid key.
//!
//! `Mode(a)` is computed by [`crate::specfence::decide_access`]. This table
//! only records **Repair / rem certificates**:
//! - Fence event (OrderedAdmit / WaitFor / serial-lane) this incarnation
//! - Repair-armed (rewind / FF-head prefix)
//!
//! Rem journal and partial_abort Resolve are legal iff a certificate exists.
//! Spec-only incarnations stay OCC-cost (bool validate + full_abort_reexecute).

use std::sync::atomic::{AtomicU8, Ordering};

use crate::TxIdx;

const OFF: u8 = 0;
const ON: u8 = 1;

/// Per-tx Fence / prefix certificates for one block.
///
/// Not a compute-kernel enum. One incarnation may mix Spec and Fence accesses.
#[derive(Debug)]
pub(crate) struct KernelTable {
    fenced: Vec<AtomicU8>,
    repair: Vec<AtomicU8>,
}

impl KernelTable {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            fenced: (0..block_size).map(|_| AtomicU8::new(OFF)).collect(),
            repair: (0..block_size).map(|_| AtomicU8::new(OFF)).collect(),
        }
    }

    /// Start an incarnation. Repair-armed (rewind / FF-head) keeps the prefix
    /// certificate. Fence events reset so a new Spec incarnation does not
    /// inherit a sibling Fire.
    #[inline]
    pub(crate) fn begin_execute(&self, tx_idx: TxIdx, repair_armed: bool) {
        if let Some(slot) = self.fenced.get(tx_idx) {
            slot.store(OFF, Ordering::Relaxed);
        }
        if let Some(slot) = self.repair.get(tx_idx) {
            slot.store(if repair_armed { ON } else { OFF }, Ordering::Relaxed);
        }
    }

    /// Certificate: this incarnation Fenced **this** access (OrderedAdmit / WaitFor / lane).
    /// Not a tx-kernel upgrade.
    #[inline]
    pub(crate) fn note_fence(&self, tx_idx: TxIdx) {
        if let Some(slot) = self.fenced.get(tx_idx) {
            slot.store(ON, Ordering::Release);
        }
    }

    #[inline]
    pub(crate) fn had_fence(&self, tx_idx: TxIdx) -> bool {
        self.fenced
            .get(tx_idx)
            .is_some_and(|s| s.load(Ordering::Acquire) == ON)
    }

    #[inline]
    pub(crate) fn repair_armed(&self, tx_idx: TxIdx) -> bool {
        self.repair
            .get(tx_idx)
            .is_some_and(|s| s.load(Ordering::Acquire) == ON)
    }

    /// rem journal / Data-wake / write_replay legal.
    #[inline]
    pub(crate) fn rem_legal(&self, tx_idx: TxIdx) -> bool {
        self.had_fence(tx_idx) || self.repair_armed(tx_idx)
    }

    /// PartialAbortRebind/PartialAbortRewind legal. Spec-only miss must not enter Resolve.
    #[inline]
    pub(crate) fn may_resolve(&self, tx_idx: TxIdx) -> bool {
        self.rem_legal(tx_idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_certificate() {
        let t = KernelTable::new(4);
        assert!(!t.had_fence(0));
        assert!(!t.may_resolve(3));
        assert!(!t.rem_legal(1));
    }

    #[test]
    fn repair_armed_is_prefix_certificate() {
        let t = KernelTable::new(2);
        t.begin_execute(1, true);
        assert!(t.repair_armed(1));
        assert!(t.may_resolve(1));
        assert!(!t.had_fence(1));
        assert!(!t.may_resolve(0));
    }

    #[test]
    fn note_fence_is_certificate_not_kernel_fork() {
        let t = KernelTable::new(1);
        t.begin_execute(0, false);
        assert!(!t.may_resolve(0));
        t.note_fence(0);
        assert!(t.had_fence(0));
        assert!(t.may_resolve(0));
    }

    #[test]
    fn next_execute_clears_fence_unless_repair() {
        let t = KernelTable::new(1);
        t.begin_execute(0, false);
        t.note_fence(0);
        t.begin_execute(0, false);
        assert!(!t.had_fence(0));
        assert!(!t.may_resolve(0));
        t.begin_execute(0, true);
        assert!(t.may_resolve(0));
    }
}
