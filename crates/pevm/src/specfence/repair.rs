//! Repair grain — R1 at first failed Fenced \(a\); Spec-only fail → B0.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v6-essence.md` §5.

use crate::MemoryLocationHash;
use crate::TxIdx;

use super::certificate::CertificateTable;

/// Validate fail grain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepairGrain {
    /// All fail locations are on the Fenced-prefix strip → R1a/R1b museum.
    R1,
    /// Spec-only (or mixed) fail → OCC B0 + PE(true k).
    B0,
}

/// Selective grain: R1 iff the certificate strip covers every invalid read.
#[inline]
pub(crate) fn repair_grain(
    cert: &CertificateTable,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> RepairGrain {
    if invalid.is_empty() || cert.covers_all(tx_idx, invalid) {
        RepairGrain::R1
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
        c.begin_execute(0, false);
        assert_eq!(repair_grain(&c, 0, &[7]), RepairGrain::B0);
    }

    #[test]
    fn fenced_fail_is_r1() {
        let c = CertificateTable::new(1);
        c.begin_execute(0, false);
        c.note_success(0, 7);
        assert_eq!(repair_grain(&c, 0, &[7]), RepairGrain::R1);
        assert_eq!(
            repair_grain(&c, 0, &[7, 8]),
            RepairGrain::B0,
            "mixed Spec fail must not sticky-cert"
        );
    }
}
