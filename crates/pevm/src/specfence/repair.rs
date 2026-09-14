//! Repair grain — R1 at first failed Fenced \(a\); Spec-only fail → B0.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` §4.

use crate::MemoryLocationHash;
use crate::TxIdx;

use super::certificate::CertificateTable;

/// Validate fail grain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepairGrain {
    /// All fail locations are on the Fenced-prefix strip → R1a/R1b.
    R1,
    /// Fenced RAW subset may R1; Spec residual stays B0 unless rebind heals RS.
    R1Selective,
    /// Spec-only fail → OCC B0 + PE(true k).
    B0,
}

/// Selective grain: R1 if strip covers every invalid; R1Selective if any fenced.
#[inline]
pub(crate) fn repair_grain(
    cert: &CertificateTable,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> RepairGrain {
    if invalid.is_empty() || cert.covers_all(tx_idx, invalid) {
        RepairGrain::R1
    } else if invalid.iter().any(|&l| cert.covers(tx_idx, l)) {
        RepairGrain::R1Selective
    } else {
        RepairGrain::B0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_only_fail_is_b0() {
        let c = CertificateTable::new(1);
        c.begin_execute(0, false, 0);
        assert_eq!(repair_grain(&c, 0, &[7]), RepairGrain::B0);
    }

    #[test]
    fn fenced_fail_is_r1() {
        let c = CertificateTable::new(1);
        c.begin_execute(0, false, 0);
        c.note_success(0, 7);
        assert_eq!(repair_grain(&c, 0, &[7]), RepairGrain::R1);
        assert_eq!(
            repair_grain(&c, 0, &[7, 8]),
            RepairGrain::R1Selective,
            "mixed: fenced RAW prefix is R1-selective, not sticky-all"
        );
    }
}
