use std::{
    collections::{BTreeMap, HashSet},
    sync::Mutex,
};

use alloy_primitives::{Address, B256};
use dashmap::DashMap;
use revm::state::Bytecode;

use smallvec::SmallVec;

use crate::{
    BuildIdentityHasher, BuildSuffixHasher, MemoryEntry, MemoryLocationHash, ReadOrigin, ReadSet,
    TxIdx, TxVersion, WriteSet, specfence::RegionTable,
};

#[derive(Default, Debug)]
struct LastLocations {
    read: ReadSet,
    // Consider [SmallVec] since most transactions explicitly write to 2 locations!
    write: Vec<MemoryLocationHash>,
}

type LazyAddresses = HashSet<Address, BuildSuffixHasher>;

/// The `MvMemory` contains shared memory in a form of a multi-version data
/// structure for values written and read by different transactions. It stores
/// multiple writes for each memory location, along with a value and an associated
/// version of a corresponding transaction.
#[derive(Debug)]
pub struct MvMemory {
    /// The list of transaction incarnations and written values for each memory location
    // No more hashing is required as we already identify memory locations by their hash
    // in the read & write sets. [dashmap] having a dedicated interface for this use case
    // (that skips hashing for [u64] keys) would make our code cleaner and "faster".
    // Nevertheless, the compiler should be good enough to optimize these cases anyway.
    pub(crate) data: DashMap<MemoryLocationHash, BTreeMap<TxIdx, MemoryEntry>, BuildIdentityHasher>,
    /// Last read & written locations of each transaction
    last_locations: Vec<Mutex<LastLocations>>,
    /// Lazy addresses that need full evaluation at the end of the block
    lazy_addresses: Mutex<LazyAddresses>,
    /// New bytecodes deployed in this block
    pub(crate) new_bytecodes: DashMap<B256, Bytecode, BuildSuffixHasher>,
    /// Per-location / per-account Wait vs Speculate for `SpecFence`.
    pub(crate) regions: RegionTable,
}

impl MvMemory {
    pub(crate) fn new(
        block_size: usize,
        estimated_locations: impl IntoIterator<Item = (MemoryLocationHash, Vec<TxIdx>)>,
        lazy_addresses: impl IntoIterator<Item = Address>,
    ) -> Self {
        // TODO: Fine-tune the number of shards, like to the next number of two from the
        // number of worker threads.
        let data = DashMap::default();
        // We preallocate estimated locations to avoid restructuring trees at runtime
        // while holding a write lock. Ideally [dashmap] would have a lock-free
        // construction API. This is acceptable for now as it's a non-congested one-time
        // cost.
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
            // TODO: Fine-tune the number of shards, like to the next number of two from the
            // number of worker threads.
            new_bytecodes: DashMap::default(),
            regions: RegionTable::new(),
        }
    }

    pub(crate) fn add_lazy_addresses(&self, new_lazy_addresses: impl IntoIterator<Item = Address>) {
        let mut lazy_addresses = self.lazy_addresses.lock().unwrap();
        for address in new_lazy_addresses {
            lazy_addresses.insert(address);
        }
    }

    // Apply a new pair of read & write sets to the multi-version data structure.
    // Return whether a write occurred to a memory location not written to by
    // the previous incarnation of the same transaction. This determines whether
    // the executed higher transactions need re-validation.
    pub(crate) fn record(
        &self,
        tx_version: &TxVersion,
        read_set: ReadSet,
        write_set: WriteSet,
    ) -> (bool, SmallVec<[MemoryLocationHash; 4]>) {
        let mut last_locations = index_mutex!(self.last_locations, tx_version.tx_idx);
        last_locations.read = read_set;

        // TODO: Group updates by shard to avoid locking operations.
        // Remove old locations that aren't written to anymore.
        let mut last_location_idx = 0;
        while last_location_idx < last_locations.write.len() {
            let prev_location = unsafe { last_locations.write.get_unchecked(last_location_idx) };
            if write_set.iter().all(|(l, _)| l != prev_location) {
                if let Some(mut written_transactions) = self.data.get_mut(prev_location) {
                    written_transactions.remove(&tx_version.tx_idx);
                }
                last_locations.write.swap_remove(last_location_idx);
            } else {
                last_location_idx += 1;
            }
        }

        // Register new writes.
        let mut wrote_new_location = false;
        let mut contended = SmallVec::<[MemoryLocationHash; 4]>::new();

        for (location, value) in write_set {
            {
                let mut written_transactions = self.data.entry(location).or_default();
                if written_transactions
                    .keys()
                    .any(|&idx| idx != tx_version.tx_idx)
                {
                    contended.push(location);
                }
                written_transactions.insert(
                    tx_version.tx_idx,
                    MemoryEntry::Data(tx_version.tx_incarnation, value),
                );
            }
            if !last_locations.write.contains(&location) {
                last_locations.write.push(location);
                wrote_new_location = true;
            }
        }

        (wrote_new_location, contended)
    }

    // Obtain the last read set recorded by an execution of [tx_idx] and check
    // that re-reading each memory location in the read set still yields the
    // same read origins.
    // This is invoked during validation, when the incarnation being validated is
    // already executed and has recorded the read set. However, if the thread
    // performing a validation for incarnation i of a transaction is slow, it is
    // possible that this function invocation observes a read set recorded by a
    // latter (> i) incarnation. In this case, incarnation i is guaranteed to be
    // already aborted (else higher incarnations would never start), and the
    // validation task will have no effect regardless of the outcome (only
    // validations that successfully abort affect the state and each incarnation
    // can be aborted at most once).
    pub(crate) fn validate_read_locations(&self, tx_idx: TxIdx) -> bool {
        self.collect_invalid_reads(tx_idx).is_empty()
    }

    /// Locations in the last recorded read set whose origins no longer match.
    pub(crate) fn collect_invalid_reads(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        let mut invalid = Vec::new();
        for (location, prior_origins) in &index_mutex!(self.last_locations, tx_idx).read {
            let still_valid = if let Some(written_transactions) = self.data.get(location) {
                let mut iter = written_transactions.range(..tx_idx);
                let mut ok = true;
                for prior_origin in prior_origins {
                    if let ReadOrigin::MvMemory(prior_version) = prior_origin {
                        // Found something: Must match version.
                        if let Some((closest_idx, MemoryEntry::Data(tx_incarnation, ..))) =
                            iter.next_back()
                        {
                            if closest_idx != &prior_version.tx_idx
                                || &prior_version.tx_incarnation != tx_incarnation
                            {
                                ok = false;
                                break;
                            }
                        }
                        // The previously read value is now cleared
                        // or marked with ESTIMATE.
                        else {
                            ok = false;
                            break;
                        }
                    }
                    // Read from storage but there is now something
                    // in between!
                    else if iter.next_back().is_some() {
                        ok = false;
                        break;
                    }
                }
                ok
            } else {
                // Read from multi-version data but now it's cleared.
                prior_origins.len() == 1 && prior_origins.last() == Some(&ReadOrigin::Storage)
            };
            if !still_valid {
                invalid.push(*location);
            }
        }
        invalid
    }

    /// Last writer with index strictly below `tx_idx`, if any.
    pub(crate) fn last_writer_before(
        &self,
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    ) -> Option<TxIdx> {
        self.data
            .get(&location)
            .and_then(|written| written.range(..tx_idx).next_back().map(|(idx, _)| *idx))
    }


    /// Write locations recorded by the last incarnation of `tx_idx`.
    pub(crate) fn write_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        index_mutex!(self.last_locations, tx_idx).write.clone()
    }

    /// Read location hashes recorded by the last incarnation of `tx_idx`.
    pub(crate) fn read_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        index_mutex!(self.last_locations, tx_idx)
            .read
            .keys()
            .copied()
            .collect()
    }

    /// True if any higher tx has this location in its last recorded read set.
    #[allow(dead_code)]
    pub(crate) fn has_higher_reader(&self, aborted_idx: TxIdx, location: MemoryLocationHash) -> bool {
        for idx in (aborted_idx + 1)..self.last_locations.len() {
            let locs = index_mutex!(self.last_locations, idx);
            if locs.read.contains_key(&location) {
                return true;
            }
        }
        false
    }

    /// Minimum higher transaction that read any location in `write_locations`.
    /// Used by SpecFence to fence the validation cascade to dependent readers only.
    pub(crate) fn min_higher_reader_of(
        &self,
        aborted_idx: TxIdx,
        write_locations: &[MemoryLocationHash],
    ) -> Option<TxIdx> {
        if write_locations.is_empty() {
            return None;
        }
        for idx in (aborted_idx + 1)..self.last_locations.len() {
            let locs = index_mutex!(self.last_locations, idx);
            if locs
                .read
                .keys()
                .any(|loc| write_locations.iter().any(|w| w == loc))
            {
                return Some(idx);
            }
        }
        None
    }

    // Replace the write set of the aborted version in the shared memory data
    // structure with special ESTIMATE markers to quickly abort higher transactions
    // that read them.
    pub(crate) fn convert_writes_to_estimates(&self, tx_idx: TxIdx) {
        for location in &index_mutex!(self.last_locations, tx_idx).write {
            if let Some(mut written_transactions) = self.data.get_mut(location) {
                written_transactions.insert(tx_idx, MemoryEntry::Estimate);
            }
        }
    }

    /// SpecFence finer ESTIMATE: mark ESTIMATE only on writes that have at least
    /// one higher recorded reader; **remove** other aborted writes so independent
    /// readers of unrelated slots are not poisoned by leftover Data versions.
    /// (Leaving Data from an aborted incarnation would be incorrect.)
    ///
    /// Lock order: inspect `last_locations` first, then touch `data` — same order
    /// as [`Self::record`], to avoid deadlocking with concurrent recorders.
    #[allow(dead_code)]
    pub(crate) fn convert_writes_to_estimates_selective(&self, tx_idx: TxIdx) {
        let writes = self.write_locations(tx_idx);
        let mut estimate = Vec::new();
        let mut remove = Vec::new();
        for location in writes {
            if self.has_higher_reader(tx_idx, location) {
                estimate.push(location);
            } else {
                remove.push(location);
            }
        }
        for location in estimate {
            if let Some(mut written_transactions) = self.data.get_mut(&location) {
                written_transactions.insert(tx_idx, MemoryEntry::Estimate);
            }
        }
        for location in remove {
            if let Some(mut written_transactions) = self.data.get_mut(&location) {
                written_transactions.remove(&tx_idx);
            }
        }
    }

    pub(crate) fn consume_lazy_addresses(&self) -> impl IntoIterator<Item = Address> {
        std::mem::take(&mut *self.lazy_addresses.lock().unwrap()).into_iter()
    }
}
