//! Access-prefix certificate strips — not a tx-global bit from one OrderedAdmit.
//!
//! Protocol: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md` §3.3.
//!
//! `note_success` only after a **successful** Fence verb (OrderedAdmit after Data,
//! WaitFor armed, SerialLane exclusive progress). OrderedAdmit-decide then Data miss
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

    /// New incarnation. Repair-armed keeps a prefix certificate.
    /// **M5:** location strips survive WaitForDependency / same-incarnation resume
    /// (`incarnation == 0` must **not** wipe). Only [`Self::begin_block`]
    /// clears. Kept strips still do **not** cover sibling optimistic_read (`covers_all`).
    #[inline]
    pub(crate) fn begin_execute(&self, tx_idx: TxIdx, repair_armed: bool, incarnation: usize) {
        let _ = incarnation;
        if let Some(slot) = self.slots.get(tx_idx) {
            // SAFETY: one executor owns `tx_idx` at a time.
            let st = unsafe { &mut *slot.get() };
            if repair_armed {
                st.repair = true;
            } else {
                st.repair = false;
            }
        }
    }

    /// Block start — the only legal strip wipe.
    #[inline]
    pub(crate) fn begin_block(&self) {
        for slot in &self.slots {
            // SAFETY: begin_block is single-threaded before workers spawn.
            unsafe { &mut *slot.get() }.clear(false);
        }
    }

    /// rem journal / partial_abort legal iff any strip or repair prefix (merged kernel).
    #[inline]
    pub(crate) fn rem_legal(&self, tx_idx: TxIdx) -> bool {
        self.has_any(tx_idx)
    }

    #[inline]
    pub(crate) fn may_resolve(&self, tx_idx: TxIdx) -> bool {
        self.has_any(tx_idx)
    }

    #[inline]
    pub(crate) fn repair_armed(&self, tx_idx: TxIdx) -> bool {
        let Some(slot) = self.slots.get(tx_idx) else {
            return false;
        };
        unsafe { &*slot.get() }.repair
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

    /// partial_abort legal iff every fail location is on the admitted-prefix strip (or repair).
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

    /// Strip membership only — **not** repair_armed. PartialAbortRewind must not treat
    /// WaitForDependency RewindTo as covering sibling optimistic_read (Iter26 seq≠par).
    #[inline]
    pub(crate) fn covers_strips_all(&self, tx_idx: TxIdx, invalid: &[MemoryLocationHash]) -> bool {
        if invalid.is_empty() {
            return true;
        }
        let Some(slot) = self.slots.get(tx_idx) else {
            return false;
        };
        let st = unsafe { &*slot.get() };
        !st.locs.is_empty() && invalid.iter().all(|l| st.locs.contains(l))
    }

    /// Single-location cover (selective partial_abort on the admitted subset).
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
    fn one_ordered_admit_does_not_cover_sibling_spec() {
        let t = CertificateTable::new(1);
        t.begin_execute(0, false, 0);
        t.note_success(0, 7);
        assert!(t.covers_all(0, &[7]));
        assert!(
            !t.covers_all(0, &[7, 9]),
            "tx-global cert from one OrderedAdmit is forbidden"
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
    fn next_execute_keeps_strips_until_begin_block() {
        let t = CertificateTable::new(1);
        t.begin_execute(0, false, 0);
        t.note_success(0, 3);
        t.begin_execute(0, false, 0);
        assert!(
            t.has_any(0),
            "M5: same-incarnation resume must not wipe strips"
        );
        t.begin_block();
        assert!(!t.has_any(0));
    }

    #[test]
    fn rem_legal_is_certificate_not_second_table() {
        let t = CertificateTable::new(2);
        assert!(!t.rem_legal(0));
        assert!(!t.may_resolve(0));
        t.note_success(0, 1);
        assert!(t.rem_legal(0));
        assert!(t.may_resolve(0));
        assert!(!t.rem_legal(1));
        t.begin_execute(1, true, 0);
        assert!(t.repair_armed(1));
        assert!(t.rem_legal(1));
    }

    #[test]
    fn wait_resume_keeps_location_strips() {
        let t = CertificateTable::new(1);
        t.begin_execute(0, false, 0);
        t.note_success(0, 7);
        t.begin_execute(0, false, 1);
        assert!(
            t.has_any(0),
            "WaitFor/OrderedAdmit strip must survive inc>0 resume"
        );
        assert!(t.covers(0, 7));
        assert!(
            !t.covers_all(0, &[7, 9]),
            "kept strip must not cover sibling optimistic_read"
        );
    }

    #[test]
    fn repair_armed_does_not_strip_cover_siblings() {
        let t = CertificateTable::new(1);
        t.begin_execute(0, true, 0);
        t.note_success(0, 7);
        assert!(t.covers_all(0, &[7, 9]), "repair_armed covers_all");
        assert!(
            !t.covers_strips_all(0, &[7, 9]),
            "PartialAbortRewind must use strips only"
        );
        assert!(t.covers_strips_all(0, &[7]));
    }
}
