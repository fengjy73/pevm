//! Per-incarnation compute kernel — OccKernel ≡ OCC, PccKernel owns rem/Resolve.
//!
//! Plant SoT: `lab/notes/specfence-parallel-compute-architecture.md`.
//! Frozen π is unchanged. `inc` is not an Avoid key.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::TxIdx;

/// Which **computer** this incarnation runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IncarnationKernel {
    /// OCC-identical execute + bool validate + B0. No rem / CallEntry / SF repair.
    Occ,
    /// PCC Fire or PrefixSkip armed. rem journal + Resolve legal.
    Pcc,
}

const OCC: u8 = 0;
const PCC: u8 = 1;

/// Per-tx kernel flags for one block. Single-executor then validate (Block-STM).
#[derive(Debug)]
pub(crate) struct KernelTable {
    flags: Vec<AtomicU8>,
}

impl KernelTable {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            flags: (0..block_size).map(|_| AtomicU8::new(OCC)).collect(),
        }
    }

    /// Start an incarnation. Repair-armed (rewind / FF-head) ⇒ PccKernel.
    /// Otherwise OccKernel; first PCC Fire upgrades via [`mark_pcc`].
    #[inline]
    pub(crate) fn begin_execute(&self, tx_idx: TxIdx, repair_armed: bool) -> IncarnationKernel {
        let flag = if repair_armed { PCC } else { OCC };
        if let Some(slot) = self.flags.get(tx_idx) {
            slot.store(flag, Ordering::Relaxed);
        }
        if repair_armed {
            IncarnationKernel::Pcc
        } else {
            IncarnationKernel::Occ
        }
    }

    /// Upgrade this incarnation after Bind / WaitFor Fire.
    #[inline]
    pub(crate) fn mark_pcc(&self, tx_idx: TxIdx) {
        if let Some(slot) = self.flags.get(tx_idx) {
            slot.store(PCC, Ordering::Release);
        }
    }

    #[inline]
    pub(crate) fn is_occ(&self, tx_idx: TxIdx) -> bool {
        self.flags
            .get(tx_idx)
            .is_none_or(|s| s.load(Ordering::Acquire) == OCC)
    }

    #[inline]
    pub(crate) fn is_pcc(&self, tx_idx: TxIdx) -> bool {
        !self.is_occ(tx_idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_occ_kernel() {
        let t = KernelTable::new(4);
        assert!(t.is_occ(0));
        assert!(t.is_occ(3));
    }

    #[test]
    fn repair_armed_starts_pcc() {
        let t = KernelTable::new(2);
        assert_eq!(t.begin_execute(1, true), IncarnationKernel::Pcc);
        assert!(t.is_pcc(1));
        assert!(t.is_occ(0));
    }

    #[test]
    fn mark_pcc_upgrades_incarnation() {
        let t = KernelTable::new(1);
        t.begin_execute(0, false);
        assert!(t.is_occ(0));
        t.mark_pcc(0);
        assert!(t.is_pcc(0));
    }

    #[test]
    fn next_execute_resets_to_occ_unless_repair() {
        let t = KernelTable::new(1);
        t.begin_execute(0, false);
        t.mark_pcc(0);
        t.begin_execute(0, false);
        assert!(t.is_occ(0));
        t.begin_execute(0, true);
        assert!(t.is_pcc(0));
    }
}
