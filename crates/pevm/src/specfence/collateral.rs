//! D-Collateral: first-conflict ℓ classify + CC-X1 commute + CC-R3 batch.
//!
//! SpecFence is adaptive OCC on one Block-STM spine. A0 = OptimisticRead
//! (same cost class as harness OCC). A1 only when effective-conflict EV wins.
//! Soft=0. Do not A1 an entire lazy same-from spine.

use crate::mv_memory::MvMemory;
use crate::{MemoryLocation, MemoryLocationHash, MemoryValue, TxIdx, hash_deterministic};

use super::AccountHints;

/// C1 class of the first validation-fail location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictClass {
    /// Empty-input transfers whose envelopes do not share a sender.
    /// Recipient / beneficiary lazy-adds commute.
    CommuteCandidate,
    /// Storage or non-lazy Basic with a real prior writer — short-edge A1.
    EffectiveWAW,
    /// basic_lazy / LazyRecipient-only (or LazySender same-from noise).
    LazyNoise,
}

/// First conflict ℓ + peer recorded on a would-reexec validate fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FirstConflict {
    pub location: MemoryLocationHash,
    pub peer: Option<TxIdx>,
    pub class: ConflictClass,
    pub lazy: bool,
}

/// Empty-input value transfer (21k class; gas limit may be 21k or 90k).
#[inline]
pub(crate) fn is_value_transfer(hints: &AccountHints, idx: TxIdx) -> bool {
    hints.is_empty_calldata(idx)
}

#[inline]
fn envelope_addrs(
    hints: &AccountHints,
    idx: TxIdx,
) -> (alloy_primitives::Address, Option<alloy_primitives::Address>) {
    (hints.from_of(idx), hints.to_of(idx))
}

/// True when `{from,to}` of `a` and `b` do not intersect.
#[inline]
pub(crate) fn envelopes_disjoint(hints: &AccountHints, a: TxIdx, b: TxIdx) -> bool {
    let (fa, ta) = envelope_addrs(hints, a);
    let (fb, tb) = envelope_addrs(hints, b);
    if fa == fb {
        return false;
    }
    if ta.is_some() && ta == tb {
        return false;
    }
    if ta == Some(fb) || tb == Some(fa) {
        return false;
    }
    true
}

#[inline]
pub(crate) fn location_is_lazy(
    mv_memory: &MvMemory,
    tx_idx: TxIdx,
    loc: MemoryLocationHash,
) -> bool {
    matches!(
        mv_memory.current_data_value(tx_idx, loc),
        Some(MemoryValue::LazyRecipient(_)) | Some(MemoryValue::LazySender(_))
    )
}

/// One invalid ℓ is a commutative lazy add (beneficiary / recipient).
#[inline]
pub(crate) fn commute_location_ok(
    hints: &AccountHints,
    mv_memory: &MvMemory,
    beneficiary: alloy_primitives::Address,
    tx_idx: TxIdx,
    loc: MemoryLocationHash,
) -> bool {
    let from = hints.from_of(tx_idx);
    let from_loc = hash_deterministic(MemoryLocation::Basic(from));
    if loc == from_loc {
        return false;
    }
    let ben_loc = hash_deterministic(MemoryLocation::Basic(beneficiary));
    if loc == ben_loc {
        return true;
    }
    let val = mv_memory.current_data_value(tx_idx, loc);
    if matches!(val, Some(MemoryValue::LazyRecipient(_))) {
        return true;
    }
    let Some(peer) = mv_memory.last_writer_before(loc, tx_idx) else {
        return false;
    };
    if peer >= tx_idx || !is_value_transfer(hints, peer) {
        return false;
    }
    if hints.from_of(peer) == from {
        return false;
    }
    matches!(
        val,
        Some(MemoryValue::LazyRecipient(_)) | Some(MemoryValue::LazySender(_))
    )
}

/// CC-X1: skip full abort when every invalid ℓ is a commutative lazy add
/// (beneficiary / recipient / LazyRecipient) and no sender-nonce WAW.
pub(crate) fn commute_ok(
    hints: &AccountHints,
    mv_memory: &MvMemory,
    beneficiary: alloy_primitives::Address,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> bool {
    if invalid.is_empty() || !is_value_transfer(hints, tx_idx) {
        return false;
    }
    invalid
        .iter()
        .all(|&loc| commute_location_ok(hints, mv_memory, beneficiary, tx_idx, loc))
}

/// C1: first invalid ℓ + peer → CommuteCandidate / EffectiveWAW / LazyNoise.
pub(crate) fn classify_first_conflict(
    hints: &AccountHints,
    mv_memory: &MvMemory,
    beneficiary: alloy_primitives::Address,
    tx_idx: TxIdx,
    invalid: &[MemoryLocationHash],
) -> Option<FirstConflict> {
    let &location = invalid.first()?;
    let peer = mv_memory
        .last_writer_before(location, tx_idx)
        .filter(|&w| w < tx_idx);
    let lazy = location_is_lazy(mv_memory, tx_idx, location);
    let val = mv_memory.current_data_value(tx_idx, location);
    let class = if commute_ok(hints, mv_memory, beneficiary, tx_idx, invalid) {
        ConflictClass::CommuteCandidate
    } else if matches!(val, Some(MemoryValue::Storage(_))) {
        ConflictClass::EffectiveWAW
    } else if lazy
        || matches!(
            val,
            Some(MemoryValue::LazyRecipient(_)) | Some(MemoryValue::LazySender(_))
        )
    {
        // Evaluated Lazy→Basic must not become EffectiveWAW (lazy-update chain).
        ConflictClass::LazyNoise
    } else if is_value_transfer(hints, tx_idx)
        && peer.is_some_and(|p| {
            is_value_transfer(hints, p) && hints.from_of(p) == hints.from_of(tx_idx)
        })
    {
        // Same-from 21k nonce race: lazy-accumulate, do not promote the spine.
        ConflictClass::LazyNoise
    } else if matches!(val, Some(MemoryValue::Basic(_))) {
        ConflictClass::EffectiveWAW
    } else {
        ConflictClass::EffectiveWAW
    };
    Some(FirstConflict {
        location,
        peer,
        class,
        lazy,
    })
}

/// A0-majority: force lazy on empty-input EOA that share from/to with another
/// tx in this block. Avoids first-touch Basic WAW on 2-tx same-from pairs
/// without a ReadyEdge on the whole spine.
#[inline]
pub(crate) fn optimistic_majority_hinted_lazy(
    hints: &AccountHints,
    from: alloy_primitives::Address,
    to: Option<alloy_primitives::Address>,
    empty_input: bool,
    eoa: bool,
    optimistic_majority_block: bool,
    queued: bool,
) -> bool {
    if !optimistic_majority_block || queued || !empty_input || !eoa {
        return false;
    }
    // Short same-from / same-to only. Long spines (≥16) stay eager so HotSet /
    // Bayes still see nonce WAW (unit tests + fan-out). 3356896 pairs are 2-tx;
    // 0x2a65 is 9. Do not A1 those spines (C5) — lazy-accumulate instead.
    const SHORT_SPINE: usize = 16;
    let n_from = hints.from_txs(&from).len();
    if (2..SHORT_SPINE).contains(&n_from) {
        return true;
    }
    if let Some(to) = to {
        let n_to = hints.to_txs(&to).len();
        if (2..SHORT_SPINE).contains(&n_to) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;

    #[test]
    fn commute_ok_matches_per_location() {
        let from = Address::repeat_byte(0x11);
        let hints = AccountHints::from_account_txs(from, vec![0, 1]);
        let mv = MvMemory::new(2, [], []);
        let ben = Address::ZERO;
        assert!(
            !commute_ok(&hints, &mv, ben, 0, &[]),
            "empty invalid is not a commute"
        );
        assert_eq!(
            commute_ok(&hints, &mv, ben, 0, &[7]),
            commute_location_ok(&hints, &mv, ben, 0, 7)
        );
    }

    #[test]
    fn value_transfer_is_empty_input_not_exact_21k() {
        let from = Address::repeat_byte(0x11);
        let hints = AccountHints::from_account_txs(from, vec![0, 1]);
        assert!(is_value_transfer(&hints, 0));
        assert!(is_value_transfer(&hints, 1));
    }

    #[test]
    fn same_from_envelopes_are_not_disjoint() {
        let from = Address::repeat_byte(0x2a);
        let to = Address::repeat_byte(0x11);
        let hints = AccountHints::from_from_and_to(from, to, vec![5, 6]);
        assert!(!envelopes_disjoint(&hints, 5, 6));
    }

    #[test]
    fn disjoint_envelopes() {
        let h = AccountHints::from_two_transfers(
            Address::repeat_byte(0xaa),
            Address::repeat_byte(0xbb),
            Address::repeat_byte(0xcc),
            Address::repeat_byte(0xdd),
        );
        assert!(envelopes_disjoint(&h, 0, 1));
        assert!(!envelopes_disjoint(&h, 0, 0));
    }

    #[test]
    fn optimistic_majority_hinted_lazy_same_from_only() {
        let from = Address::repeat_byte(0x2a);
        let to = Address::repeat_byte(0x11);
        let hints = AccountHints::from_from_and_to(from, to, vec![5, 6]);
        assert!(optimistic_majority_hinted_lazy(
            &hints,
            from,
            Some(to),
            true,
            true,
            true,
            false
        ));
        assert!(
            !optimistic_majority_hinted_lazy(&hints, from, Some(to), true, true, true, true),
            "A1-queued must not take the A0 lazy commute"
        );
        let solo = Address::repeat_byte(0x99);
        let h1 = AccountHints::from_from_and_to(solo, Address::repeat_byte(0x88), vec![3]);
        assert!(
            !optimistic_majority_hinted_lazy(
                &h1,
                solo,
                Some(Address::repeat_byte(0x88)),
                true,
                true,
                true,
                false
            ),
            "unique 21k stays eager (no single-entry lazy tax)"
        );
        // Long same-from spines stay eager so HotSet / Bayes still see nonce WAW.
        let long: Vec<TxIdx> = (0..32).collect();
        let h32 = AccountHints::from_from_and_to(from, to, long);
        assert!(
            !optimistic_majority_hinted_lazy(&h32, from, Some(to), true, true, true, false),
            "n_from=32 ≥ SHORT_SPINE must not force-lazy"
        );
    }
}
