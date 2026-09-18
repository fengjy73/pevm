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
    let from = hints.from_of(tx_idx);
    let from_loc = hash_deterministic(MemoryLocation::Basic(from));
    let to_loc = hints
        .to_of(tx_idx)
        .map(|t| hash_deterministic(MemoryLocation::Basic(t)));
    let ben_loc = hash_deterministic(MemoryLocation::Basic(beneficiary));
    for &loc in invalid {
        if loc == from_loc {
            return false;
        }
        if loc == ben_loc {
            continue;
        }
        let val = mv_memory.current_data_value(tx_idx, loc);
        if matches!(val, Some(MemoryValue::LazyRecipient(_))) {
            continue;
        }
        if to_loc == Some(loc) && matches!(val, Some(MemoryValue::LazyRecipient(_))) {
            continue;
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
        if !matches!(
            val,
            Some(MemoryValue::LazyRecipient(_)) | Some(MemoryValue::LazySender(_))
        ) {
            return false;
        }
    }
    true
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
    } else if matches!(val, Some(MemoryValue::Storage(_)))
        || matches!(val, Some(MemoryValue::Basic(_)))
    {
        ConflictClass::EffectiveWAW
    } else if lazy || matches!(val, Some(MemoryValue::LazyRecipient(_))) {
        ConflictClass::LazyNoise
    } else if is_value_transfer(hints, tx_idx)
        && peer.is_some_and(|p| {
            is_value_transfer(hints, p) && hints.from_of(p) == hints.from_of(tx_idx)
        })
    {
        // Same-from 21k nonce race: lazy-accumulate, do not promote the spine.
        ConflictClass::LazyNoise
    } else if matches!(val, Some(MemoryValue::LazySender(_))) {
        ConflictClass::LazyNoise
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

/// Thin-shell A0: force lazy on empty-input EOA that share from/to with another
/// tx in this block. Avoids first-touch Basic WAW on 2-tx same-from pairs
/// without planting a ReadyEdge on the whole spine.
#[inline]
pub(crate) fn thin_a0_hinted_lazy(
    hints: &AccountHints,
    from: alloy_primitives::Address,
    to: Option<alloy_primitives::Address>,
    empty_input: bool,
    eoa: bool,
    thin_shell: bool,
    queued: bool,
) -> bool {
    if !thin_shell || queued || !empty_input || !eoa {
        return false;
    }
    if hints.from_txs(&from).len() >= 2 {
        return true;
    }
    if let Some(to) = to
        && hints.to_txs(&to).len() >= 2
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;

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
    fn thin_a0_hinted_lazy_same_from_only() {
        let from = Address::repeat_byte(0x2a);
        let to = Address::repeat_byte(0x11);
        let hints = AccountHints::from_from_and_to(from, to, vec![5, 6]);
        assert!(thin_a0_hinted_lazy(
            &hints,
            from,
            Some(to),
            true,
            true,
            true,
            false
        ));
        assert!(
            !thin_a0_hinted_lazy(&hints, from, Some(to), true, true, true, true),
            "A1-queued must not take the A0 lazy commute"
        );
        let solo = Address::repeat_byte(0x99);
        let h1 = AccountHints::from_from_and_to(solo, Address::repeat_byte(0x88), vec![3]);
        assert!(
            !thin_a0_hinted_lazy(
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
    }
}
