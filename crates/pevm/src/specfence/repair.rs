//! Repair grain — partial_abort at first failed pessimistic-admit \(a\);
//! optimistic-read-only fail → full_abort_reexecute.
//!
//! Protocol: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` §4.
//! Vocabulary: `lab/notes/specfence-cc-glossary.md`.

use crate::MemoryLocationHash;
use crate::TxIdx;

use super::certificate::CertificateTable;

/// Validate fail grain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepairGrain {
    /// All fail locations are on the admitted-prefix strip → PartialAbortRebind/PartialAbortRewind.
    PartialAbort,
    /// Admitted RAW subset may partial_abort; optimistic residual stays full abort unless rebind heals RS.
    PartialAbortSelective,
    /// Optimistic-read-only fail → OCC full_abort_reexecute + PE(true k).
    FullAbortReexecute,
}

/// Selective grain: PartialAbort if strip covers every invalid; PartialAbortSelective if any admitted.
#[inline]
pub(crate) fn repair_grain(
    cert: &CertificateTable,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> RepairGrain {
    if invalid.is_empty() || cert.covers_all(tx_idx, invalid) {
        RepairGrain::PartialAbort
    } else if invalid.iter().any(|&l| cert.covers(tx_idx, l)) {
        RepairGrain::PartialAbortSelective
    } else {
        RepairGrain::FullAbortReexecute
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimistic_only_fail_is_full_abort_reexecute() {
        let c = CertificateTable::new(1);
        c.begin_execute(0, false, 0);
        assert_eq!(repair_grain(&c, 0, &[7]), RepairGrain::FullAbortReexecute);
    }

    #[test]
    fn admitted_fail_is_partial_abort() {
        let c = CertificateTable::new(1);
        c.begin_execute(0, false, 0);
        c.note_success(0, 7);
        assert_eq!(repair_grain(&c, 0, &[7]), RepairGrain::PartialAbort);
        assert_eq!(
            repair_grain(&c, 0, &[7, 8]),
            RepairGrain::PartialAbortSelective,
            "mixed: admitted RAW prefix is partial-abort-selective, not sticky-all"
        );
    }
}
