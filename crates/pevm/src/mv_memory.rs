use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use alloy_primitives::{Address, B256};
use dashmap::DashMap;
use revm::state::Bytecode;

use smallvec::SmallVec;

use crate::{
    BuildIdentityHasher, BuildSuffixHasher, MemoryEntry, MemoryLocationHash, MemoryValue,
    ReadOrigin, ReadSet, TxIdx, TxIncarnation, TxVersion, WriteSet, specfence::RegionTable,
};

#[derive(Default, Debug)]
struct LastLocations {
    read: ReadSet,
    // Consider [SmallVec] since most transactions explicitly write to 2 locations!
    write: Vec<MemoryLocationHash>,
}

type LazyAddresses = HashSet<Address, BuildSuffixHasher>;

thread_local! {
    static DATA_NEST: Cell<u32> = const { Cell::new(0) };
}

/// DashMap is not reentrant. Same-thread nested `data.get` is the
/// 19807137 / 19469101 `munmap_chunk` / `double free`.
pub(crate) struct DataNest(&'static str);

impl DataNest {
    pub(crate) fn enter(what: &'static str) -> Self {
        DATA_NEST.with(|c| {
            let n = c.get();
            if n > 0 {
                panic!("MvMemory.data re-enter via {what} nest={n}");
            }
            c.set(n + 1);
        });
        Self(what)
    }
}

impl Drop for DataNest {
    fn drop(&mut self) {
        DATA_NEST.with(|c| c.set(c.get().saturating_sub(1)));
    }
}

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
    /// SpecFence readers index: `readers[ℓ]` = txs that currently record a read origin on ℓ.
    readers: DashMap<MemoryLocationHash, BTreeSet<TxIdx>, BuildIdentityHasher>,
    /// Aborted incarnation numbers: reading Data with this (tx,inc) is invalid.
    aborted_incarnations: DashMap<TxIdx, TxIncarnation, BuildIdentityHasher>,
    /// Prior incarnation write-set (Bohm-lite residual) for OrderedAdmit/WaitHard placeholders.
    residual_write_sets: DashMap<TxIdx, Vec<MemoryLocationHash>, BuildIdentityHasher>,
    /// Values copied out before Estimate so a WAR reader can re-read the tip
    /// it bound. The origin stores those bytes. Validation fails while the
    /// live entry is Estimate, or when a later Data under the same incarnation
    /// carries a different value.
    retained_history:
        DashMap<MemoryLocationHash, Vec<(TxIdx, TxIncarnation, MemoryValue)>, BuildIdentityHasher>,
    /// Seqlock for publishes. Odd means a write is in progress. Exit confirmation
    /// retries when the epoch changes so it cannot miss the last write.
    write_epoch: AtomicU64,
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
            readers: DashMap::default(),
            aborted_incarnations: DashMap::default(),
            residual_write_sets: DashMap::default(),
            retained_history: DashMap::default(),
            write_epoch: AtomicU64::new(0),
        }
    }

    pub(crate) fn write_epoch(&self) -> u64 {
        self.write_epoch.load(Ordering::Acquire)
    }

    fn begin_publish(&self) {
        self.write_epoch.fetch_add(1, Ordering::Release);
    }

    fn end_publish(&self) {
        self.write_epoch.fetch_add(1, Ordering::Release);
    }

    /// Copy one aborted Data value. The caller still installs Estimate.
    pub(crate) fn pin_aborted_value(
        &self,
        location: MemoryLocationHash,
        tx_idx: TxIdx,
        inc: TxIncarnation,
        value: MemoryValue,
    ) {
        let mut slot = self.retained_history.entry(location).or_default();
        if slot.len() < 64 {
            slot.push((tx_idx, inc, value));
        }
    }

    /// Historical tip for `tx_idx` if RetainHistory snapshotted it.
    pub(crate) fn pinned_entry(
        &self,
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    ) -> Option<MemoryEntry> {
        let slot = self.retained_history.get(&location)?;
        slot.iter()
            .rev()
            .find(|(t, _, _)| *t == tx_idx)
            .map(|(_, inc, value)| MemoryEntry::Data(*inc, value.clone()))
    }

    pub(crate) fn add_lazy_addresses(&self, new_lazy_addresses: impl IntoIterator<Item = Address>) {
        let mut lazy_addresses = self.lazy_addresses.lock().unwrap();
        for address in new_lazy_addresses {
            lazy_addresses.insert(address);
        }
    }

    /// True if incarnation `inc` of `tx_idx` was aborted (Data must not be trusted).
    pub(crate) fn is_aborted_incarnation(&self, tx_idx: TxIdx, incarnation: TxIncarnation) -> bool {
        self.aborted_incarnations
            .get(&tx_idx)
            .is_some_and(|a| *a == incarnation)
    }

    /// Mark an incarnation aborted so late readers detect dangling Data.
    pub(crate) fn mark_incarnation_aborted(&self, tx_idx: TxIdx, incarnation: TxIncarnation) {
        self.aborted_incarnations.insert(tx_idx, incarnation);
    }

    /// Clear aborted stamp when a newer incarnation publishes.
    pub(crate) fn clear_aborted_incarnation(&self, tx_idx: TxIdx) {
        self.aborted_incarnations.remove(&tx_idx);
    }

    /// Residual write locations from the last aborted incarnation (Bohm-lite).
    #[allow(dead_code)]
    /// Data value still published by `tx_idx` at `location`, if any.
    pub(crate) fn published_data_value(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> Option<crate::MemoryValue> {
        let _nest = DataNest::enter("published_data_value");
        let written = self.data.get(&location)?;
        match written.get(&tx_idx)? {
            MemoryEntry::Data(_, value) => Some(value.clone()),
            MemoryEntry::Estimate => None,
        }
    }

    pub(crate) fn residual_writes(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        self.residual_write_sets
            .get(&tx_idx)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// M3: optional block-local WŜ publish (kept for experiments; default path
    /// uses process `RwPriorMap` + abort residual only — see pevm validate success).
    #[allow(dead_code)]
    pub(crate) fn publish_ws_prior(&self, tx_idx: TxIdx) {
        let writes = self.write_locations(tx_idx);
        self.residual_write_sets.insert(tx_idx, writes);
    }

    /// True if some prior tx `t_w < me` lists `location` in its residual write-set.
    pub(crate) fn residual_writer_before(
        &self,
        location: MemoryLocationHash,
        me: TxIdx,
    ) -> Option<TxIdx> {
        let mut best = None;
        for entry in self.residual_write_sets.iter() {
            let tw = *entry.key();
            if tw < me && entry.value().iter().any(|l| *l == location) {
                best = Some(match best {
                    Some(prev) if prev > tw => prev,
                    _ => tw,
                });
            }
        }
        best
    }

    fn unregister_reader(&self, tx_idx: TxIdx, location: MemoryLocationHash) {
        if let Some(mut set) = self.readers.get_mut(&location) {
            set.remove(&tx_idx);
            if set.is_empty() {
                drop(set);
                self.readers.remove(&location);
            }
        }
    }

    fn register_reader(&self, tx_idx: TxIdx, location: MemoryLocationHash) {
        self.readers.entry(location).or_default().insert(tx_idx);
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

        // Update readers index: clear prior read locations, register new ones.
        for old_loc in last_locations.read.keys() {
            self.unregister_reader(tx_version.tx_idx, *old_loc);
        }
        for loc in read_set.keys() {
            self.register_reader(tx_version.tx_idx, *loc);
        }
        last_locations.read = read_set;

        // Successful publish clears aborted stamp for this tx.
        self.clear_aborted_incarnation(tx_version.tx_idx);

        // TODO: Group updates by shard to avoid locking operations.
        // Remove old locations that aren't written to anymore.
        let _nest = DataNest::enter("record");
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

        self.begin_publish();
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
        self.end_publish();

        // M3: residual WŜ publish moved to pevm success path for SpecFence only
        // (avoid OCC/PCC behavior change). See MvMemory::publish_ws_prior.

        (wrote_new_location, contended)
    }

    // Obtain the last read set recorded by an execution of [tx_idx] and check
    // that re-reading each memory location in the read set still yields the
    // same read origins.
    pub(crate) fn validate_read_locations(&self, tx_idx: TxIdx) -> bool {
        for (location, prior_origins) in &index_mutex!(self.last_locations, tx_idx).read {
            if !self.origin_still_valid(tx_idx, *location, prior_origins) {
                return false;
            }
        }
        true
    }

    /// Per-location validate API (SpecFence Spec v1): true iff origin still matches.
    #[allow(dead_code)]
    pub(crate) fn validate_location(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> bool {
        let locs = index_mutex!(self.last_locations, tx_idx);
        let Some(prior_origins) = locs.read.get(&location) else {
            return true;
        };
        self.origin_still_valid(tx_idx, location, prior_origins)
    }

    /// EarlyVal helper: validate origins from the in-flight VmDb read set.
    pub(crate) fn origins_still_valid(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        prior_origins: &crate::ReadOrigins,
    ) -> bool {
        self.origin_still_valid(tx_idx, location, prior_origins)
    }

    fn origin_still_valid(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        prior_origins: &crate::ReadOrigins,
    ) -> bool {
        let _nest = DataNest::enter("origin_still_valid");
        if let Some(written_transactions) = self.data.get(&location) {
            let mut iter = written_transactions.range(..tx_idx);
            for prior_origin in prior_origins {
                if let ReadOrigin::MvMemory(prior_version, prior_value) = prior_origin {
                    if let Some((closest_idx, MemoryEntry::Data(tx_incarnation, value))) =
                        iter.next_back()
                    {
                        if self.is_aborted_incarnation(*closest_idx, *tx_incarnation) {
                            return false;
                        }
                        if closest_idx != &prior_version.tx_idx
                            || &prior_version.tx_incarnation != tx_incarnation
                            || value != prior_value
                        {
                            return false;
                        }
                    } else {
                        return false;
                    }
                } else if iter.next_back().is_some() {
                    return false;
                }
            }
            true
        } else {
            prior_origins.len() == 1 && prior_origins.last() == Some(&ReadOrigin::Storage)
        }
    }

    /// Locations in the last recorded read set whose origins no longer match.
    pub(crate) fn collect_invalid_reads(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        let mut invalid = Vec::new();
        for (location, prior_origins) in &index_mutex!(self.last_locations, tx_idx).read {
            if !self.origin_still_valid(tx_idx, *location, prior_origins) {
                invalid.push(*location);
            }
        }
        invalid
    }

    /// C2: one `last_locations` lock — commute-check + value-stable rebind.
    /// No separate `collect_invalid_reads` Vec on the accept path.
    pub(crate) fn try_commute_rebind_invalid(
        &self,
        tx_idx: TxIdx,
        mut commute_loc: impl FnMut(MemoryLocationHash) -> bool,
    ) -> bool {
        let mut planned = Vec::new();
        {
            let locs = index_mutex!(self.last_locations, tx_idx);
            for (location, prior_origins) in &locs.read {
                if self.origin_still_valid(tx_idx, *location, prior_origins) {
                    continue;
                }
                if !commute_loc(*location) {
                    return false;
                }
                let Some(new_origins) = self.current_read_origins(tx_idx, *location) else {
                    return false;
                };
                if !self.origin_still_valid(tx_idx, *location, &new_origins) {
                    return false;
                }
                planned.push((*location, new_origins));
            }
        }
        if planned.is_empty() {
            return false;
        }
        let mut locs = index_mutex!(self.last_locations, tx_idx);
        for (location, new_origins) in planned {
            locs.read.insert(location, new_origins);
        }
        true
    }

    /// M1 RebindOnly: patch invalid read origins to the current valid version
    /// without aborting. Refuses multi-origin (lazy) reads — those need RewindTo
    /// unless [`Self::try_rebind_invalid_reads_value_stable`] (same-output).
    /// Returns true iff every invalid location was patched to a still-valid origin.
    /// Applies patches atomically (no partial mutate on failure).
    pub(crate) fn try_rebind_invalid_reads(
        &self,
        tx_idx: TxIdx,
        invalid: &[MemoryLocationHash],
    ) -> bool {
        self.try_rebind_invalid_reads_inner(tx_idx, invalid, false)
    }

    /// Value-stable RebindOnly: allow multi-origin (lazy) → current single Data
    /// when caller proved same-output republish. Still refuses Estimate.
    pub(crate) fn try_rebind_invalid_reads_value_stable(
        &self,
        tx_idx: TxIdx,
        invalid: &[MemoryLocationHash],
    ) -> bool {
        self.try_rebind_invalid_reads_inner(tx_idx, invalid, true)
    }

    fn try_rebind_invalid_reads_inner(
        &self,
        tx_idx: TxIdx,
        invalid: &[MemoryLocationHash],
        allow_multi_origin: bool,
    ) -> bool {
        if invalid.is_empty() {
            return true;
        }
        let mut planned = Vec::with_capacity(invalid.len());
        {
            let locs = index_mutex!(self.last_locations, tx_idx);
            for &location in invalid {
                let prior = locs.read.get(&location);
                if !allow_multi_origin && prior.is_some_and(|o| o.len() > 1) {
                    return false;
                }
                let Some(new_origins) = self.current_read_origins(tx_idx, location) else {
                    return false;
                };
                // Pre-check against MV (no mutate yet).
                if !self.origin_still_valid(tx_idx, location, &new_origins) {
                    return false;
                }
                planned.push((location, new_origins));
            }
        }
        let mut locs = index_mutex!(self.last_locations, tx_idx);
        for (location, new_origins) in planned {
            locs.read.insert(location, new_origins);
        }
        true
    }

    /// Value-stable across origin bump for `location` in `tx_idx`'s last read set:
    /// prior MvMemory origin's Data equals current published Data (balance+nonce /
    /// Storage U256). Iter6: last MvMemory origin in a multi-origin (lazy) chain
    /// is accepted; incarnation must still match (Estimate→Data relies on snap).
    /// Storage-origin / Estimate → false (snap path covers those).
    pub(crate) fn prior_read_value_stable(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> bool {
        let prior = {
            let locs = index_mutex!(self.last_locations, tx_idx);
            let Some(prior_origins) = locs.read.get(&location) else {
                return false;
            };
            // The value stored on the origin is what the interpreter consumed.
            // The live slot at that tx index may already have been rewritten
            // under the same incarnation, so it is not the read.
            let mv = prior_origins.iter().rev().find_map(|o| match o {
                ReadOrigin::MvMemory(v, value) => Some((v.clone(), value.clone())),
                _ => None,
            });
            match mv {
                Some(v) => v,
                None => return false,
            }
        };
        let Some(cur) = self.current_data_value(tx_idx, location) else {
            return false;
        };
        let (_prior, prior_val) = prior;
        match (&prior_val, &cur) {
            (MemoryValue::Storage(a), MemoryValue::Storage(b)) => a == b,
            (MemoryValue::Basic(a), MemoryValue::Basic(b)) => {
                a.balance == b.balance && a.nonce == b.nonce
            }
            _ => false,
        }
    }

    /// Closest live Data value below `tx_idx`, if any (Estimate → None).
    pub(crate) fn current_data_value(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> Option<crate::MemoryValue> {
        let _nest = DataNest::enter("current_data_value");
        let written = self.data.get(&location)?;
        match written.range(..tx_idx).next_back()? {
            (idx, MemoryEntry::Data(inc, val)) => {
                if self.is_aborted_incarnation(*idx, *inc) {
                    None
                } else {
                    Some(val.clone())
                }
            }
            (_, MemoryEntry::Estimate) => None,
        }
    }

    /// Current single-origin read for `location` as of `tx_idx` (for RebindOnly).
    fn current_read_origins(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> Option<crate::ReadOrigins> {
        use crate::{ReadOrigin, ReadOrigins};
        let mut origins = ReadOrigins::new();
        if let Some(written_transactions) = self.data.get(&location) {
            match written_transactions.range(..tx_idx).next_back() {
                Some((closest_idx, MemoryEntry::Data(tx_incarnation, value))) => {
                    if self.is_aborted_incarnation(*closest_idx, *tx_incarnation) {
                        return None;
                    }
                    origins.push(ReadOrigin::mv(*closest_idx, *tx_incarnation, value.clone()));
                }
                Some((_, MemoryEntry::Estimate)) => return None,
                None => origins.push(ReadOrigin::Storage),
            }
        } else {
            origins.push(ReadOrigin::Storage);
        }
        Some(origins)
    }

    /// Last writer with index strictly below `tx_idx`, if any.
    pub(crate) fn last_writer_before(
        &self,
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    ) -> Option<TxIdx> {
        let _nest = DataNest::enter("last_writer_before");
        self.data
            .get(&location)
            .and_then(|written| written.range(..tx_idx).next_back().map(|(idx, _)| *idx))
    }

    /// Research: MV entry kind for a concrete writer at `location` (`data`/`estimate`/`absent`).
    pub(crate) fn entry_kind_at(
        &self,
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    ) -> &'static str {
        let Some(written) = self.data.get(&location) else {
            return "absent";
        };
        match written.get(&tx_idx) {
            Some(MemoryEntry::Data(_, _)) => "data",
            Some(MemoryEntry::Estimate) => "estimate",
            None => "absent",
        }
    }

    /// Last live Storage Data below `tx_idx` inside an already-held location map.
    /// Callers that hold `data.get(location)` must use this instead of
    /// [`last_data_before`] — DashMap is not reentrant.
    pub(crate) fn last_live_storage_in(
        written: &BTreeMap<TxIdx, MemoryEntry>,
        tx_idx: TxIdx,
        is_aborted: impl Fn(TxIdx, TxIncarnation) -> bool,
    ) -> Option<(TxIdx, TxIncarnation, alloy_primitives::U256)> {
        for (idx, entry) in written.range(..tx_idx).rev() {
            if let MemoryEntry::Data(inc, MemoryValue::Storage(v)) = entry
                && !is_aborted(*idx, *inc)
            {
                return Some((*idx, *inc, *v));
            }
        }
        None
    }

    /// Last non-ESTIMATE Data version strictly below `tx_idx` (OrderedDirtyRead).
    /// Skips ESTIMATE markers and aborted incarnations (continue past both).
    ///
    /// Must not be called while the caller holds `data.get` on the same
    /// location (DashMap is not reentrant — 19807137 `double free`).
    pub(crate) fn last_data_before(
        &self,
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    ) -> Option<(TxIdx, TxIncarnation)> {
        let _nest = DataNest::enter("last_data_before");
        let written = self.data.get(&location)?;
        for (idx, entry) in written.range(..tx_idx).rev() {
            match entry {
                MemoryEntry::Data(inc, _) => {
                    if !self.is_aborted_incarnation(*idx, *inc) {
                        return Some((*idx, *inc));
                    }
                    // Aborted incarnation: keep scanning for older live Data.
                }
                MemoryEntry::Estimate => {
                    // OrderedDirtyRead: skip ESTIMATE marker, keep scanning.
                }
            }
        }
        None
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

    /// Readers currently registered on `location` with index > `after`.
    pub(crate) fn higher_readers_of(
        &self,
        location: MemoryLocationHash,
        after: TxIdx,
    ) -> Vec<TxIdx> {
        self.readers
            .get(&location)
            .map(|set| set.iter().copied().filter(|&t| t > after).collect())
            .unwrap_or_default()
    }

    /// True if any higher tx has this location in its last recorded read set.
    pub(crate) fn has_higher_reader(
        &self,
        aborted_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> bool {
        self.readers
            .get(&location)
            .is_some_and(|set| set.iter().any(|&t| t > aborted_idx))
    }

    /// Minimum higher transaction that read any location in `write_locations`.
    /// Prefer readers index; fall back to scanning last_locations.
    pub(crate) fn min_higher_reader_of(
        &self,
        aborted_idx: TxIdx,
        write_locations: &[MemoryLocationHash],
    ) -> Option<TxIdx> {
        if write_locations.is_empty() {
            return None;
        }
        let mut min_reader = None;
        for &loc in write_locations {
            for r in self.higher_readers_of(loc, aborted_idx) {
                min_reader = Some(match min_reader {
                    Some(m) if m < r => m,
                    _ => r,
                });
            }
        }
        if min_reader.is_some() {
            return min_reader;
        }
        // Fallback scan (readers index may lag concurrent unrecorded readers).
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
        self.convert_writes_to_estimates_keeping(tx_idx, |_| false);
    }

    /// Like [`Self::convert_writes_to_estimates`]. Locations where `keep` is
    /// true are snapshotted first. The live entry still becomes Estimate so a
    /// higher reader cannot validate the aborted incarnation.
    pub(crate) fn convert_writes_to_estimates_keeping(
        &self,
        tx_idx: TxIdx,
        keep: impl Fn(MemoryLocationHash) -> bool,
    ) {
        let writes = self.write_locations(tx_idx);
        self.residual_write_sets.insert(tx_idx, writes.clone());
        self.begin_publish();
        for location in &writes {
            if keep(*location) {
                let snap = self
                    .data
                    .get(location)
                    .and_then(|written| match written.get(&tx_idx) {
                        Some(MemoryEntry::Data(inc, value)) => Some((*inc, value.clone())),
                        _ => None,
                    });
                if let Some((inc, value)) = snap {
                    self.pin_aborted_value(*location, tx_idx, inc, value);
                }
            }
            if let Some(mut written_transactions) = self.data.get_mut(location) {
                written_transactions.insert(tx_idx, MemoryEntry::Estimate);
            }
        }
        self.end_publish();
    }

    /// SpecFence Spec v1 §6.1 selective invalidate.
    ///
    /// For each written location:
    /// - if `readers[ℓ]` contains any `t > t_w` → ESTIMATE
    /// - else keep Data but stamp `incarnation_aborted` so late readers detect mismatch
    ///
    /// Returns `(estimated_locations, used_fallback_full)`.
    /// If the aborted incarnation number is unknown, falls back to full ESTIMATE.
    pub(crate) fn invalidate_selective(
        &self,
        tx_idx: TxIdx,
        incarnation: Option<TxIncarnation>,
    ) -> (Vec<MemoryLocationHash>, bool) {
        let writes = self.write_locations(tx_idx);
        self.residual_write_sets.insert(tx_idx, writes.clone());

        let Some(inc) = incarnation else {
            self.begin_publish();
            for location in &writes {
                if let Some(mut written_transactions) = self.data.get_mut(location) {
                    written_transactions.insert(tx_idx, MemoryEntry::Estimate);
                }
            }
            self.end_publish();
            return (writes, true);
        };

        self.mark_incarnation_aborted(tx_idx, inc);

        let mut estimated = Vec::new();
        self.begin_publish();
        for location in writes {
            if self.has_higher_reader(tx_idx, location) {
                if let Some(mut written_transactions) = self.data.get_mut(&location) {
                    written_transactions.insert(tx_idx, MemoryEntry::Estimate);
                }
                estimated.push(location);
            }
            // else: keep Data; aborted stamp detects late readers.
        }
        self.end_publish();
        (estimated, false)
    }

    /// SpecFence finer ESTIMATE (legacy helper): mark ESTIMATE only on writes that
    /// have higher readers; remove other aborted writes.
    #[allow(dead_code)]
    pub(crate) fn convert_writes_to_estimates_selective(&self, tx_idx: TxIdx) {
        let (estimated, _) = self.invalidate_selective(tx_idx, None);
        let _ = estimated;
    }

    /// P2 PartialRetry: ESTIMATE only `suffix` write locations; leave prefix Data
    /// intact and **do not** stamp a global aborted incarnation (so higher readers
    /// of certified-prefix writes remain valid).
    pub(crate) fn invalidate_partial_suffix(
        &self,
        tx_idx: TxIdx,
        suffix: &[MemoryLocationHash],
    ) -> Vec<MemoryLocationHash> {
        let writes = self.write_locations(tx_idx);
        self.residual_write_sets.insert(tx_idx, writes);
        let mut estimated = Vec::with_capacity(suffix.len());
        self.begin_publish();
        for &location in suffix {
            if let Some(mut written_transactions) = self.data.get_mut(&location) {
                written_transactions.insert(tx_idx, MemoryEntry::Estimate);
            }
            estimated.push(location);
        }
        self.end_publish();
        estimated
    }

    pub(crate) fn consume_lazy_addresses(&self) -> impl IntoIterator<Item = Address> {
        std::mem::take(&mut *self.lazy_addresses.lock().unwrap()).into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReadOrigin, ReadOrigins, TxVersion};
    use alloy_primitives::U256;

    fn read_of(loc: MemoryLocationHash, origin: ReadOrigin) -> ReadSet {
        let mut origins = ReadOrigins::new();
        origins.push(origin);
        let mut reads = ReadSet::default();
        reads.insert(loc, origins);
        reads
    }

    #[test]
    fn same_incarnation_rewrite_fails_validation() {
        let mv = MvMemory::new(2, std::iter::empty(), std::iter::empty());
        let loc = 7u64;
        let writer = TxVersion {
            tx_idx: 0,
            tx_incarnation: 0,
        };
        mv.record(
            &writer,
            ReadSet::default(),
            vec![(loc, MemoryValue::LazySender(U256::from(100u64)))],
        );
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            read_of(
                loc,
                ReadOrigin::mv(0, 0, MemoryValue::LazySender(U256::from(100u64))),
            ),
            Vec::new(),
        );
        assert!(mv.validate_read_locations(1));
        mv.record(
            &writer,
            ReadSet::default(),
            vec![(loc, MemoryValue::LazySender(U256::from(50u64)))],
        );
        assert!(
            !mv.validate_read_locations(1),
            "same incarnation published a different lazy amount"
        );
    }

    #[test]
    fn estimate_replaced_under_same_incarnation_fails_validation() {
        let mv = MvMemory::new(2, std::iter::empty(), std::iter::empty());
        let loc = 9u64;
        let writer = TxVersion {
            tx_idx: 0,
            tx_incarnation: 0,
        };
        mv.record(
            &writer,
            ReadSet::default(),
            vec![(loc, MemoryValue::Storage(U256::from(3u64)))],
        );
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            read_of(
                loc,
                ReadOrigin::mv(0, 0, MemoryValue::Storage(U256::from(3u64))),
            ),
            Vec::new(),
        );
        mv.invalidate_partial_suffix(0, &[loc]);
        assert!(!mv.validate_read_locations(1));
        mv.record(
            &writer,
            ReadSet::default(),
            vec![(loc, MemoryValue::Storage(U256::from(8u64)))],
        );
        assert!(
            !mv.validate_read_locations(1),
            "Estimate replaced by a different Data value under the same incarnation"
        );
    }
}
