//! `SpecFence` multi-version memory.
//!
//! Read origins store the [`MemoryValue`] the interpreter consumed.
//! Validation compares both the writer identity and that value.
//! There is no value-only rebind.

use std::{
    collections::{BTreeMap, HashSet},
    sync::Mutex,
};

use alloy_primitives::{Address, B256, U256};
use dashmap::DashMap;

use crate::{
    BuildIdentityHasher, BuildSuffixHasher, MemoryEntry, MemoryLocationHash, MemoryValue, TxIdx,
    TxVersion, WriteSet,
};

use super::trace::Trace;

#[derive(Clone, Debug)]
pub(crate) struct SfOrigin {
    pub(crate) tx_idx: TxIdx,
    pub(crate) incarnation: usize,
    pub(crate) value: MemoryValue,
}

#[derive(Clone, Debug)]
pub(crate) enum SfReadOrigin {
    Mv(SfOrigin),
    Storage,
}

pub(crate) type SfReadOrigins = smallvec::SmallVec<[SfReadOrigin; 1]>;
pub(crate) type SfReadSet =
    hashbrown::HashMap<MemoryLocationHash, SfReadOrigins, BuildIdentityHasher>;

#[allow(clippy::missing_const_for_fn)]
pub(crate) fn memory_value_eq(a: &MemoryValue, b: &MemoryValue) -> bool {
    match (a, b) {
        (MemoryValue::Basic(x), MemoryValue::Basic(y)) => x == y,
        (MemoryValue::CodeHash(x), MemoryValue::CodeHash(y)) => x == y,
        (MemoryValue::Storage(x), MemoryValue::Storage(y))
        | (MemoryValue::LazyRecipient(x), MemoryValue::LazyRecipient(y))
        | (MemoryValue::LazySender(x), MemoryValue::LazySender(y)) => x == y,
        (MemoryValue::SelfDestructed, MemoryValue::SelfDestructed) => true,
        _ => false,
    }
}

pub(crate) const fn is_lazy_value(value: &MemoryValue) -> bool {
    matches!(
        value,
        MemoryValue::LazyRecipient(_) | MemoryValue::LazySender(_)
    )
}

#[derive(Default)]
struct LastLocations {
    read: SfReadSet,
    write: Vec<MemoryLocationHash>,
}

type LazyAddresses = HashSet<Address, BuildSuffixHasher>;

pub(crate) struct SfMv {
    pub(crate) data: DashMap<MemoryLocationHash, BTreeMap<TxIdx, MemoryEntry>, BuildIdentityHasher>,
    last_locations: Vec<Mutex<LastLocations>>,
    lazy_addresses: Mutex<LazyAddresses>,
    pub(crate) new_bytecodes: DashMap<B256, revm::state::Bytecode, BuildSuffixHasher>,
}

impl SfMv {
    pub(crate) fn new(
        block_size: usize,
        estimated_locations: impl IntoIterator<Item = (MemoryLocationHash, Vec<TxIdx>)>,
        lazy_addresses: impl IntoIterator<Item = Address>,
    ) -> Self {
        let data = DashMap::default();
        for (location_hash, estimated_tx_idxs) in estimated_locations {
            data.insert(
                location_hash,
                estimated_tx_idxs
                    .into_iter()
                    .map(|tx_idx| (tx_idx, MemoryEntry::Estimate))
                    .collect(),
            );
        }
        Self {
            data,
            last_locations: (0..block_size).map(|_| Mutex::default()).collect(),
            lazy_addresses: Mutex::new(LazyAddresses::from_iter(lazy_addresses)),
            new_bytecodes: DashMap::default(),
        }
    }

    pub(crate) fn add_lazy_addresses(&self, new_lazy_addresses: impl IntoIterator<Item = Address>) {
        let mut lazy_addresses = self.lazy_addresses.lock().unwrap();
        for address in new_lazy_addresses {
            lazy_addresses.insert(address);
        }
    }

    pub(crate) fn record(
        &self,
        tx_version: &TxVersion,
        read_set: SfReadSet,
        write_set: WriteSet,
    ) -> bool {
        let mut last_locations = index_mutex!(self.last_locations, tx_version.tx_idx);
        last_locations.read = read_set;

        let mut last_location_idx = 0;
        while last_location_idx < last_locations.write.len() {
            let prev_location = last_locations.write[last_location_idx];
            if write_set.iter().all(|(l, _)| *l != prev_location) {
                if let Some(mut written_transactions) = self.data.get_mut(&prev_location) {
                    written_transactions.remove(&tx_version.tx_idx);
                }
                last_locations.write.swap_remove(last_location_idx);
            } else {
                last_location_idx += 1;
            }
        }

        let mut wrote_new_location = false;
        for (location, value) in write_set {
            self.data.entry(location).or_default().insert(
                tx_version.tx_idx,
                MemoryEntry::Data(tx_version.tx_incarnation, value),
            );
            if !last_locations.write.contains(&location) {
                last_locations.write.push(location);
                wrote_new_location = true;
            }
        }
        wrote_new_location
    }

    pub(crate) fn write_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        index_mutex!(self.last_locations, tx_idx).write.clone()
    }

    /// `None` when every recorded origin still matches the live entry's identity and value.
    pub(crate) fn failing_location(&self, tx_idx: TxIdx) -> Option<MemoryLocationHash> {
        let last = index_mutex!(self.last_locations, tx_idx);
        for (location, prior_origins) in &last.read {
            if !origins_still_valid(&self.data, *location, tx_idx, prior_origins) {
                return Some(*location);
            }
        }
        None
    }

    pub(crate) fn convert_writes_to_estimates(&self, tx_idx: TxIdx) {
        for location in &index_mutex!(self.last_locations, tx_idx).write {
            if let Some(mut written_transactions) = self.data.get_mut(location) {
                written_transactions.insert(tx_idx, MemoryEntry::Estimate);
            }
        }
    }

    pub(crate) fn consume_lazy_addresses(&self) -> impl IntoIterator<Item = Address> {
        std::mem::take(&mut *self.lazy_addresses.lock().unwrap()).into_iter()
    }

    /// Record RAW edges from the committed read set. Trace-only.
    pub(crate) fn collect_edges(&self, tx_idx: TxIdx, trace: &Trace) {
        if !trace.enabled() {
            return;
        }
        let last = index_mutex!(self.last_locations, tx_idx);
        for origins in last.read.values() {
            for origin in origins {
                if let SfReadOrigin::Mv(mv) = origin {
                    trace.note_edge(mv.tx_idx, tx_idx);
                }
            }
        }
    }
}

fn origins_still_valid(
    data: &DashMap<MemoryLocationHash, BTreeMap<TxIdx, MemoryEntry>, BuildIdentityHasher>,
    location: MemoryLocationHash,
    tx_idx: TxIdx,
    prior_origins: &SfReadOrigins,
) -> bool {
    if let Some(written_transactions) = data.get(&location) {
        let mut iter = written_transactions.range(..tx_idx);
        for prior_origin in prior_origins {
            match prior_origin {
                SfReadOrigin::Mv(prior) => match iter.next_back() {
                    Some((closest_idx, MemoryEntry::Data(tx_incarnation, value))) => {
                        if closest_idx != &prior.tx_idx
                            || &prior.incarnation != tx_incarnation
                            || !memory_value_eq(value, &prior.value)
                        {
                            return false;
                        }
                    }
                    _ => return false,
                },
                SfReadOrigin::Storage => {
                    if iter.next_back().is_some() {
                        return false;
                    }
                }
            }
        }
        true
    } else {
        prior_origins.len() == 1 && matches!(prior_origins.last(), Some(SfReadOrigin::Storage))
    }
}

/// Value the interpreter may keep using after the live entry becomes an Estimate.
pub(crate) fn retained_storage(origins: &SfReadOrigins) -> Option<U256> {
    match origins.last() {
        Some(SfReadOrigin::Mv(origin)) => match &origin.value {
            MemoryValue::Storage(value) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryValue;

    fn loc() -> u64 {
        7
    }

    #[test]
    fn same_incarnation_rewrite_fails_validation() {
        let mv = SfMv::new(2, [], []);
        let v1 = MemoryValue::LazySender(U256::from(100));
        let v2 = MemoryValue::LazySender(U256::from(50));
        mv.record(
            &TxVersion {
                tx_idx: 0,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), v1.clone())],
        );
        let mut reads = SfReadSet::default();
        reads.insert(
            loc(),
            smallvec::smallvec![SfReadOrigin::Mv(SfOrigin {
                tx_idx: 0,
                incarnation: 0,
                value: v1,
            })],
        );
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            reads,
            vec![],
        );
        assert!(mv.failing_location(1).is_none());
        mv.record(
            &TxVersion {
                tx_idx: 0,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), v2)],
        );
        assert!(mv.failing_location(1).is_some());
    }

    #[test]
    fn estimate_replaced_under_same_incarnation_fails_validation() {
        let mv = SfMv::new(2, [], []);
        mv.record(
            &TxVersion {
                tx_idx: 0,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), MemoryValue::Storage(U256::from(3)))],
        );
        let mut reads = SfReadSet::default();
        reads.insert(
            loc(),
            smallvec::smallvec![SfReadOrigin::Mv(SfOrigin {
                tx_idx: 0,
                incarnation: 0,
                value: MemoryValue::Storage(U256::from(3)),
            })],
        );
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            reads,
            vec![],
        );
        mv.convert_writes_to_estimates(0);
        assert!(mv.failing_location(1).is_some());
        mv.record(
            &TxVersion {
                tx_idx: 0,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), MemoryValue::Storage(U256::from(8)))],
        );
        assert!(mv.failing_location(1).is_some());
    }
}
