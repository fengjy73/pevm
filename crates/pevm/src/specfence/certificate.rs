//! Access-prefix certificate strips — not a tx-global bit from one Bind.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` §3.3.
//!
//! `note_success` only after a **successful** Fence verb (Bind after Data,
//! WaitFor armed, SerialLane exclusive progress). Bind-decide then Data miss
//! must not write a strip. Spec never certifies.
//!
//! `may_resolve` ≡ strip covers the **failed locations**, not `had_fence(tx)`.

use std::cell::UnsafeCell;

use hashbrown::HashSet;

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

struct TxStrip {
    locs: HashSet<MemoryLocationHash, BuildIdentityHasher>,
    repair: bool,
}

impl TxStrip {
    fn new() -> Self {
        Self {
            locs: HashSet::with_hasher(BuildIdentityHasher::default()),
            repair: false,
        }
    }

    fn clear(&mut self, repair: bool) {
        self.locs.clear();
        self.repair = repair;
    }
}

/// Per-tx Fenced-prefix strips for one block.
pub(crate) struct CertificateTable {
    slots: Vec<UnsafeCell<TxStrip>>,
}

impl std::fmt::Debug for CertificateTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertificateTable")
            .field("n", &self.slots.len())
            .finish()
    }
}

unsafe impl Sync for CertificateTable {}

impl CertificateTable {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            slots: (0..block_size)
                .map(|_| UnsafeCell::new(TxStrip::new()))
                .collect(),
        }
    }

    /// New incarnation. Repair-armed keeps a prefix certificate (no locs).
    /// Incarnation 0 clears strips. Later incarnations **keep location
    /// strips** so WaitFor/Bind certs survive Blocking resume (CC → R1).
    /// They still do **not** cover sibling Spec misses (`covers_all`).
    #[inline]
    pub(crate) fn begin_execute(
        &self,
        tx_idx: TxIdx,
        repair_armed: bool,
        incarnation: usize,
    ) {
        if let Some(slot) = self.slots.get(tx_idx) {
            // SAFETY: one executor owns `tx_idx` at a time.
            let st = unsafe { &mut *slot.get() };
            if repair_armed {
                st.clear(true);
            } else if incarnation == 0 {
                st.clear(false);
            } else {
                st.repair = false;
            }
        }
    }

    /// Successful Fence verb on \(\ell\) — the only legal `note_fence` equivalent.
    #[inline]
    pub(crate) fn note_success(&self, tx_idx: TxIdx, location: MemoryLocationHash) {
        let Some(slot) = self.slots.get(tx_idx) else {
            return;
        };
        // SAFETY: one executor owns `tx_idx` at a time.
        unsafe { &mut *slot.get() }.locs.insert(location);
    }

    #[inline]
    pub(crate) fn has_any(&self, tx_idx: TxIdx) -> bool {
        let Some(slot) = self.slots.get(tx_idx) else {
            return false;
        };
        // SAFETY: validate of `tx_idx` does not race an executor on the same slot
        // after publish (incarnation finished).
        let st = unsafe { &*slot.get() };
        st.repair || !st.locs.is_empty()
    }

    /// R1 legal iff every fail location is on the Fenced-prefix strip (or repair).
    #[inline]
    pub(crate) fn covers_all(&self, tx_idx: TxIdx, invalid: &[MemoryLocationHash]) -> bool {
        if invalid.is_empty() {
            return true;
        }
        let Some(slot) = self.slots.get(tx_idx) else {
            return false;
        };
        let st = unsafe { &*slot.get() };
        if st.repair {
            return true;
        }
        invalid.iter().all(|l| st.locs.contains(l))
    }

    /// Single-location cover (selective R1 on the fenced subset).
    #[inline]
    pub(crate) fn covers(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> bool {
        let Some(slot) = self.slots.get(tx_idx) else {
            return false;
        };
        let st = unsafe { &*slot.get() };
        st.repair || st.locs.contains(&location)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_strip_without_success() {
        let t = CertificateTable::new(2);
        t.begin_execute(0, false, 0);
        assert!(!t.has_any(0));
        assert!(!t.covers_all(0, &[7]));
    }

    #[test]
    fn one_bind_does_not_cover_sibling_spec() {
        let t = CertificateTable::new(1);
        t.begin_execute(0, false, 0);
        t.note_success(0, 7);
        assert!(t.covers_all(0, &[7]));
        assert!(
            !t.covers_all(0, &[7, 9]),
            "tx-global cert from one Bind is forbidden"
        );
        assert!(!t.covers_all(0, &[9]));
    }

    #[test]
    fn repair_armed_is_prefix_certificate() {
        let t = CertificateTable::new(1);
        t.begin_execute(0, true, 0);
        assert!(t.has_any(0));
        assert!(t.covers_all(0, &[1, 2]));
    }

    #[test]
    fn next_execute_clears_unless_repair() {
        let t = CertificateTable::new(1);
        t.begin_execute(0, false, 0);
        t.note_success(0, 3);
        t.begin_execute(0, false, 0);
        assert!(!t.has_any(0));
    }

    #[test]
    fn wait_resume_keeps_location_strips() {
        let t = CertificateTable::new(1);
        t.begin_execute(0, false, 0);
        t.note_success(0, 7);
        t.begin_execute(0, false, 1);
        assert!(t.has_any(0), "WaitFor/Bind strip must survive inc>0 resume");
        assert!(t.covers(0, 7));
        assert!(
            !t.covers_all(0, &[7, 9]),
            "kept strip must not cover sibling Spec"
        );
    }
}
