//! `SpecFence` multi-version memory.
//!
//! Read origins store the [`MemoryValue`] the interpreter consumed.
//! Validation compares both the writer identity and that value.
//! There is no value-only rebind.

use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use alloy_primitives::{Address, B256, U256};
use dashmap::DashMap;

use crate::{
    AccountBasic, BuildIdentityHasher, BuildSuffixHasher, MemoryEntry, MemoryLocationHash,
    MemoryValue, TxIdx, TxVersion, WriteSet,
};

use super::trace::Trace;

#[derive(Clone, Debug)]
pub(crate) struct SfOrigin {
    pub(crate) tx_idx: TxIdx,
    pub(crate) incarnation: usize,
    pub(crate) value: MemoryValue,
}

/// One commutative credit folded into a read before that transaction finished.
#[derive(Clone, Debug)]
pub(crate) struct DeltaFold {
    pub(crate) tx_idx: TxIdx,
    pub(crate) amount: U256,
}

/// Consumed account after adding input-determined credits to an RMW or storage base.
///
/// `usize::MAX` is not stored here. Validation treats a delta as final when
/// any incarnation of `tx_idx` is final (`is_final` is called with `usize::MAX`).
#[derive(Clone, Debug)]
pub(crate) struct FoldedRead {
    pub(crate) base_tx: Option<TxIdx>,
    pub(crate) base_incarnation: usize,
    /// Account before the lazy credits. Storage is a [`MemoryValue::Basic`].
    pub(crate) base: MemoryValue,
    pub(crate) deltas: smallvec::SmallVec<[DeltaFold; 4]>,
    pub(crate) consumed: AccountBasic,
}

#[derive(Clone, Debug)]
pub(crate) enum SfReadOrigin {
    Mv(SfOrigin),
    /// Writer identity without the value. A single worker commits a transaction
    /// before the next one reads, so a matching incarnation still names the
    /// value validation would have compared.
    MvId {
        tx_idx: TxIdx,
        incarnation: usize,
    },
    Storage,
    /// RMW base plus the delta credits the interpreter added.
    Folded(FoldedRead),
}

/// `ops` is highest transaction index first, matching the lazy read walk.
pub(crate) fn net_lazy(ops: &[(bool, U256)]) -> (bool, U256, u64) {
    let mut balance_addition = U256::ZERO;
    let mut positive_addition = true;
    let mut nonce_addition = 0u64;
    for &(is_sender, amount) in ops {
        if is_sender {
            if positive_addition {
                positive_addition = balance_addition >= amount;
                balance_addition = balance_addition.abs_diff(amount);
            } else {
                balance_addition = balance_addition.saturating_add(amount);
            }
            nonce_addition += 1;
        } else if positive_addition {
            balance_addition = balance_addition.saturating_add(amount);
        } else {
            positive_addition = amount >= balance_addition;
            balance_addition = balance_addition.abs_diff(amount);
        }
    }
    (positive_addition, balance_addition, nonce_addition)
}

pub(crate) fn apply_net(account: &mut AccountBasic, positive: bool, addition: U256, nonce: u64) {
    account.nonce = account.nonce.saturating_add(nonce);
    if positive {
        account.balance = account.balance.saturating_add(addition);
    } else {
        account.balance = account.balance.saturating_sub(addition);
    }
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

pub(crate) struct ReadMismatch {
    pub(crate) location: MemoryLocationHash,
    pub(crate) origin_tx: Option<TxIdx>,
    pub(crate) live_tx: Option<TxIdx>,
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
    /// `record` has run for this incarnation. An absent location then contributes 0.
    sealed: bool,
}

type LazyAddresses = HashSet<Address, BuildSuffixHasher>;

pub(crate) struct SfMv {
    pub(crate) data: DashMap<MemoryLocationHash, BTreeMap<TxIdx, MemoryEntry>, BuildIdentityHasher>,
    last_locations: Vec<Mutex<LastLocations>>,
    /// Readers whose recorded origin names this writer. Not drained: a failed
    /// close must be able to try again.
    final_waiters: Vec<Mutex<Vec<TxIdx>>>,
    /// Transactions that recorded a read of this location, in either order.
    read_index: DashMap<MemoryLocationHash, Vec<TxIdx>, BuildIdentityHasher>,
    lazy_addresses: Mutex<LazyAddresses>,
    pub(crate) new_bytecodes: DashMap<B256, revm::state::Bytecode, BuildSuffixHasher>,
    /// Analyzed code shared by every worker. Empty when the flag is off.
    pub(crate) codes: super::share::CodeShare,
    /// Read-mostly base state. Empty when the flag is off.
    pub(crate) base: super::share::BaseShare,
    /// Keep per-transaction read origins. Wall-clock serial turns this off.
    track_reads: AtomicBool,
    /// Insert readers into the location index. Serial never validates concurrently.
    index_reads: AtomicBool,
    /// Locations inserted into [`Self::data`]. A miss skips the map.
    present: super::bloom::LocBloom,
    /// Locations a transaction has read. Writers publish these.
    readers: super::bloom::LocBloom,
}

impl SfMv {
    /// Location chain under the shard lock. The hold is a wait, not interpreter time.
    pub(crate) fn read_location(
        &self,
        location: &MemoryLocationHash,
    ) -> Option<dashmap::mapref::one::Ref<'_, MemoryLocationHash, BTreeMap<TxIdx, MemoryEntry>>>
    {
        let _wait = super::buckets::WaitGuard::start(super::buckets::WAIT_LOCK);
        self.data.get(location)
    }

    pub(crate) fn new(
        block_size: usize,
        estimated_locations: impl IntoIterator<Item = (MemoryLocationHash, Vec<TxIdx>)>,
        lazy_addresses: impl IntoIterator<Item = Address>,
    ) -> Self {
        let data = DashMap::default();
        let present = super::bloom::LocBloom::new(1 << 16);
        for (location_hash, estimated_tx_idxs) in estimated_locations {
            present.insert(location_hash);
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
            final_waiters: (0..block_size).map(|_| Mutex::new(Vec::new())).collect(),
            read_index: DashMap::default(),
            lazy_addresses: Mutex::new(LazyAddresses::from_iter(lazy_addresses)),
            new_bytecodes: DashMap::default(),
            codes: super::share::CodeShare::new(),
            base: super::share::BaseShare::new(),
            track_reads: AtomicBool::new(true),
            index_reads: AtomicBool::new(true),
            present,
            readers: super::bloom::LocBloom::new(1 << 16),
        }
    }

    /// A miss means no write of this location has been published into the map.
    pub(crate) fn might_hold(&self, location: MemoryLocationHash) -> bool {
        self.present.may_contain(location)
    }

    pub(crate) fn note_reader(&self, location: MemoryLocationHash) {
        self.readers.insert(location);
    }

    pub(crate) fn reader_seen(&self, location: MemoryLocationHash) -> bool {
        self.readers.may_contain(location)
    }

    pub(crate) fn set_track_reads(&self, on: bool) {
        self.track_reads.store(on, Ordering::Relaxed);
    }

    pub(crate) fn set_index_reads(&self, on: bool) {
        self.index_reads.store(on, Ordering::Relaxed);
    }

    pub(crate) fn tracks_reads(&self) -> bool {
        self.track_reads.load(Ordering::Relaxed)
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
        last_locations.sealed = true;
        if self.track_reads.load(Ordering::Relaxed) {
            last_locations.read = read_set;
        } else {
            last_locations.read.clear();
        }

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

        if self.index_reads.load(Ordering::Relaxed) {
            for (location, origins) in &last_locations.read {
                self.note_read(tx_version.tx_idx, *location, origins);
            }
        }

        let mut wrote_new_location = false;
        for (location, value) in write_set {
            self.present.insert(location);
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

    fn note_read(&self, reader: TxIdx, location: MemoryLocationHash, origins: &SfReadOrigins) {
        let mut readers = self.read_index.entry(location).or_default();
        if !readers.contains(&reader) {
            readers.push(reader);
        }
        drop(readers);
        for writer in origin_dependencies(origins) {
            if writer >= reader || writer >= self.final_waiters.len() {
                continue;
            }
            let mut waiters = index_mutex!(self.final_waiters, writer);
            if !waiters.contains(&reader) {
                waiters.push(reader);
            }
        }
    }

    /// Executed, and this location is not in the write set. The credit is 0.
    pub(crate) fn sealed_without(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> bool {
        if tx_idx >= self.last_locations.len() {
            return false;
        }
        let last = index_mutex!(self.last_locations, tx_idx);
        last.sealed && !last.write.contains(&location)
    }

    pub(crate) fn waiters_of(&self, tx_idx: TxIdx) -> Vec<TxIdx> {
        if tx_idx >= self.final_waiters.len() {
            return Vec::new();
        }
        index_mutex!(self.final_waiters, tx_idx).clone()
    }

    pub(crate) fn readers_of(&self, location: MemoryLocationHash) -> Vec<TxIdx> {
        self.read_index
            .get(&location)
            .map(|readers| readers.clone())
            .unwrap_or_default()
    }

    /// `(location, read-from writer)`. `None` means the read saw storage.
    pub(crate) fn read_floors(&self, tx_idx: TxIdx) -> Vec<(MemoryLocationHash, Option<TxIdx>)> {
        if tx_idx >= self.last_locations.len() {
            return Vec::new();
        }
        let last = index_mutex!(self.last_locations, tx_idx);
        last.read
            .iter()
            .map(|(location, origins)| {
                let floor = origins.iter().filter_map(origin_tx).max();
                (*location, floor)
            })
            .collect()
    }

    /// Every multi-version origin names a final incarnation. Storage is final.
    /// Does not itself walk the writer chain; the caller does that.
    pub(crate) fn origins_final(
        &self,
        tx_idx: TxIdx,
        mut is_final: impl FnMut(TxIdx, usize) -> bool,
    ) -> bool {
        if tx_idx >= self.last_locations.len() {
            return false;
        }
        let last = index_mutex!(self.last_locations, tx_idx);
        for origins in last.read.values() {
            for origin in origins {
                match origin {
                    SfReadOrigin::Storage => {}
                    SfReadOrigin::Mv(mv) => {
                        if !is_final(mv.tx_idx, mv.incarnation) {
                            return false;
                        }
                    }
                    SfReadOrigin::MvId {
                        tx_idx,
                        incarnation,
                    } => {
                        if !is_final(*tx_idx, *incarnation) {
                            return false;
                        }
                    }
                    SfReadOrigin::Folded(fold) => {
                        if let Some(tx) = fold.base_tx
                            && !is_final(tx, fold.base_incarnation)
                        {
                            return false;
                        }
                        for delta in &fold.deltas {
                            // `usize::MAX` asks the caller for any final incarnation.
                            if !is_final(delta.tx_idx, usize::MAX) {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        true
    }

    pub(crate) fn write_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        index_mutex!(self.last_locations, tx_idx).write.clone()
    }

    /// `None` when every recorded origin still matches the live entry's identity and value.
    /// Why a folded origin does not match. `0` when it matches or was not folded.
    pub(crate) fn fold_reason(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> u8 {
        if tx_idx >= self.last_locations.len() {
            return 0;
        }
        let fold = {
            let last = index_mutex!(self.last_locations, tx_idx);
            match last.read.get(&location).and_then(|origins| origins.first()) {
                Some(SfReadOrigin::Folded(fold)) => fold.clone(),
                _ => return 0,
            }
        };
        fold_mismatch(self, location, tx_idx, &fold)
            .map(|(_, _, reason)| reason)
            .unwrap_or(0)
    }

    pub(crate) fn origin_is_folded(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> bool {
        if tx_idx >= self.last_locations.len() {
            return false;
        }
        let last = index_mutex!(self.last_locations, tx_idx);
        last.read
            .get(&location)
            .is_some_and(|origins| matches!(origins.first(), Some(SfReadOrigin::Folded(_))))
    }

    pub(crate) fn failing_location(&self, tx_idx: TxIdx) -> Option<MemoryLocationHash> {
        self.first_mismatch(tx_idx).map(|m| m.location)
    }

    /// First read whose origin is not the live write. `origin_tx` / `live_tx`
    /// are `None` when that side is storage or absent.
    pub(crate) fn first_mismatch(&self, tx_idx: TxIdx) -> Option<ReadMismatch> {
        let last = index_mutex!(self.last_locations, tx_idx);
        for (location, prior_origins) in &last.read {
            if let Some((origin_tx, live_tx)) =
                mismatch_writers(self, *location, tx_idx, prior_origins)
            {
                return Some(ReadMismatch {
                    location: *location,
                    origin_tx,
                    live_tx,
                });
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

    /// Read origins and write locations for the attribution DAG.
    ///
    /// `on_read` receives `u32::MAX` when the origin is storage. `on_write`
    /// marks lazy balance updates, which commute and are not writer-chain edges.
    pub(crate) fn visit_io(
        &self,
        n: usize,
        mut on_read: impl FnMut(usize, u64, u32),
        mut on_write: impl FnMut(usize, u64, bool),
    ) {
        for tx in 0..n.min(self.last_locations.len()) {
            let last = index_mutex!(self.last_locations, tx);
            for (location, origins) in &last.read {
                for origin in origins {
                    let writer = match origin {
                        SfReadOrigin::Mv(mv) => mv.tx_idx as u32,
                        SfReadOrigin::MvId { tx_idx, .. } => *tx_idx as u32,
                        SfReadOrigin::Storage => u32::MAX,
                        SfReadOrigin::Folded(fold) => {
                            fold.base_tx.map(|tx| tx as u32).unwrap_or(u32::MAX)
                        }
                    };
                    on_read(tx, *location, writer);
                }
            }
            for location in &last.write {
                let lazy = self.data.get(location).is_some_and(|written| {
                    matches!(
                        written.get(&tx),
                        Some(MemoryEntry::Data(_, value)) if is_lazy_value(value)
                    )
                });
                on_write(tx, *location, lazy);
            }
        }
    }

    /// Record RAW edges from the committed read set. Trace-only.
    pub(crate) fn collect_edges(&self, tx_idx: TxIdx, trace: &Trace) {
        if !trace.enabled() {
            return;
        }
        let last = index_mutex!(self.last_locations, tx_idx);
        for origins in last.read.values() {
            for origin in origins {
                match origin {
                    SfReadOrigin::Mv(mv) => trace.note_edge(mv.tx_idx, tx_idx),
                    SfReadOrigin::MvId { tx_idx: writer, .. } => trace.note_edge(*writer, tx_idx),
                    SfReadOrigin::Storage => {}
                    SfReadOrigin::Folded(fold) => {
                        if let Some(writer) = fold.base_tx {
                            trace.note_edge(writer, tx_idx);
                        }
                    }
                }
            }
        }
    }
}

const fn origin_tx(origin: &SfReadOrigin) -> Option<TxIdx> {
    match origin {
        SfReadOrigin::Mv(mv) => Some(mv.tx_idx),
        SfReadOrigin::MvId { tx_idx, .. } => Some(*tx_idx),
        SfReadOrigin::Storage => None,
        SfReadOrigin::Folded(fold) => fold.base_tx,
    }
}

fn origin_dependencies(origins: &SfReadOrigins) -> Vec<TxIdx> {
    let mut out = Vec::new();
    for origin in origins {
        match origin {
            SfReadOrigin::Mv(mv) => out.push(mv.tx_idx),
            SfReadOrigin::MvId { tx_idx, .. } => out.push(*tx_idx),
            SfReadOrigin::Storage => {}
            SfReadOrigin::Folded(fold) => {
                if let Some(tx) = fold.base_tx {
                    out.push(tx);
                }
                for delta in &fold.deltas {
                    out.push(delta.tx_idx);
                }
            }
        }
    }
    out
}

fn mismatch_writers(
    mv: &SfMv,
    location: MemoryLocationHash,
    tx_idx: TxIdx,
    prior_origins: &SfReadOrigins,
) -> Option<(Option<TxIdx>, Option<TxIdx>)> {
    if let Some(SfReadOrigin::Folded(fold)) = prior_origins.first() {
        return fold_mismatch(mv, location, tx_idx, fold).map(|(origin, live, _)| (origin, live));
    }
    let data = &mv.data;
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
                            return Some((Some(prior.tx_idx), Some(*closest_idx)));
                        }
                    }
                    Some((closest_idx, _)) => {
                        return Some((Some(prior.tx_idx), Some(*closest_idx)));
                    }
                    None => return Some((Some(prior.tx_idx), None)),
                },
                SfReadOrigin::MvId {
                    tx_idx,
                    incarnation,
                } => match iter.next_back() {
                    Some((closest_idx, MemoryEntry::Data(tx_incarnation, _))) => {
                        if closest_idx != tx_idx || tx_incarnation != incarnation {
                            return Some((Some(*tx_idx), Some(*closest_idx)));
                        }
                    }
                    Some((closest_idx, _)) => {
                        return Some((Some(*tx_idx), Some(*closest_idx)));
                    }
                    None => return Some((Some(*tx_idx), None)),
                },
                SfReadOrigin::Storage => {
                    if let Some((closest_idx, _)) = iter.next_back() {
                        return Some((None, Some(*closest_idx)));
                    }
                }
                SfReadOrigin::Folded(_) => {
                    return Some((origin_tx(prior_origin), None));
                }
            }
        }
        None
    } else if prior_origins.len() == 1
        && matches!(prior_origins.last(), Some(SfReadOrigin::Storage))
    {
        None
    } else {
        let origin_tx = prior_origins.iter().find_map(origin_tx);
        Some((origin_tx, None))
    }
}

pub(crate) const DELTA_BASE: u8 = 1;
pub(crate) const DELTA_NON_LAZY: u8 = 2;
pub(crate) const DELTA_AMOUNT: u8 = 3;
pub(crate) const DELTA_ESTIMATE: u8 = 4;
pub(crate) const DELTA_SEALED: u8 = 5;

fn fold_mismatch(
    mv: &SfMv,
    location: MemoryLocationHash,
    reader: TxIdx,
    fold: &FoldedRead,
) -> Option<(Option<TxIdx>, Option<TxIdx>, u8)> {
    let start = fold.base_tx.map(|tx| tx.saturating_add(1)).unwrap_or(0);
    if let Some(base_tx) = fold.base_tx {
        let ok = mv.data.get(&location).is_some_and(|written| {
            matches!(
                written.get(&base_tx),
                Some(MemoryEntry::Data(inc, value))
                    if *inc == fold.base_incarnation && memory_value_eq(value, &fold.base)
            )
        });
        if !ok {
            let live = mv
                .data
                .get(&location)
                .and_then(|written| written.range(..reader).next_back().map(|(tx, _)| *tx));
            return Some((Some(base_tx), live, DELTA_BASE));
        }
    }
    let mut ops: Vec<(TxIdx, bool, U256)> = Vec::new();
    if let Some(written) = mv.data.get(&location) {
        for (tx, entry) in written.range(start..reader) {
            match entry {
                MemoryEntry::Data(_, MemoryValue::LazyRecipient(amount)) => {
                    ops.push((*tx, false, *amount));
                }
                MemoryEntry::Data(_, MemoryValue::LazySender(amount)) => {
                    ops.push((*tx, true, *amount));
                }
                MemoryEntry::Estimate => return Some((fold.base_tx, Some(*tx), DELTA_ESTIMATE)),
                MemoryEntry::Data(_, _) => return Some((fold.base_tx, Some(*tx), DELTA_NON_LAZY)),
            }
        }
    }
    let mut sealed = false;
    for delta in &fold.deltas {
        if ops.iter().any(|(tx, _, _)| *tx == delta.tx_idx) {
            continue;
        }
        let amount = if mv.sealed_without(delta.tx_idx, location) {
            sealed = true;
            U256::ZERO
        } else {
            delta.amount
        };
        ops.push((delta.tx_idx, false, amount));
    }
    ops.sort_by(|left, right| right.0.cmp(&left.0));
    let net_ops: Vec<(bool, U256)> = ops
        .iter()
        .map(|(_, sender, amount)| (*sender, *amount))
        .collect();
    let (positive, addition, nonce) = net_lazy(&net_ops);
    let MemoryValue::Basic(mut account) = fold.base.clone() else {
        return Some((fold.base_tx, None, DELTA_BASE));
    };
    apply_net(&mut account, positive, addition, nonce);
    if account == fold.consumed {
        None
    } else {
        let live = ops.first().map(|(tx, _, _)| *tx).or(fold.base_tx);
        Some((
            fold.deltas
                .first()
                .map(|delta| delta.tx_idx)
                .or(fold.base_tx),
            live,
            if sealed { DELTA_SEALED } else { DELTA_AMOUNT },
        ))
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

    fn basic(balance: u64) -> MemoryValue {
        MemoryValue::Basic(AccountBasic {
            balance: U256::from(balance),
            nonce: 0,
        })
    }

    fn folded(
        base_tx: Option<usize>,
        base_balance: u64,
        deltas: &[(usize, u64)],
        consumed: u64,
    ) -> SfReadOrigin {
        SfReadOrigin::Folded(FoldedRead {
            base_tx,
            base_incarnation: 0,
            base: basic(base_balance),
            deltas: deltas
                .iter()
                .map(|(tx, amount)| DeltaFold {
                    tx_idx: *tx,
                    amount: U256::from(*amount),
                })
                .collect(),
            consumed: AccountBasic {
                balance: U256::from(consumed),
                nonce: 0,
            },
        })
    }

    fn read_folded(mv: &SfMv, reader: usize, origin: SfReadOrigin) {
        let mut reads = SfReadSet::default();
        reads.insert(loc(), smallvec::smallvec![origin]);
        mv.record(
            &TxVersion {
                tx_idx: reader,
                tx_incarnation: 0,
            },
            reads,
            vec![],
        );
    }

    #[test]
    fn failed_delta_does_not_match_the_prediction() {
        let mv = SfMv::new(3, [], []);
        mv.record(
            &TxVersion {
                tx_idx: 0,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), basic(100))],
        );
        // tx 1 was predicted to credit 40 and then sealed without that write.
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![],
        );
        read_folded(&mv, 2, folded(Some(0), 100, &[(1, 40)], 140));
        assert!(mv.failing_location(2).is_some());
    }

    #[test]
    fn predicted_delta_that_becomes_rmw_mismatches() {
        let mv = SfMv::new(3, [], []);
        mv.record(
            &TxVersion {
                tx_idx: 0,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), basic(100))],
        );
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), basic(999))],
        );
        read_folded(&mv, 2, folded(Some(0), 100, &[(1, 40)], 140));
        assert!(mv.failing_location(2).is_some());
    }

    #[test]
    fn rmw_writer_appearing_below_mismatches_the_folded_base() {
        let mv = SfMv::new(4, [], []);
        mv.record(
            &TxVersion {
                tx_idx: 0,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), basic(100))],
        );
        read_folded(&mv, 3, folded(Some(0), 100, &[(2, 5)], 105));
        assert!(mv.failing_location(3).is_none());
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), basic(50))],
        );
        assert!(mv.failing_location(3).is_some());
    }

    #[test]
    fn beneficiary_lazy_reward_interleaves_with_a_predicted_delta() {
        let mv = SfMv::new(4, [], []);
        mv.record(
            &TxVersion {
                tx_idx: 0,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), basic(100))],
        );
        // Gas reward landed on this account before the predicted transfer.
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), MemoryValue::LazyRecipient(U256::from(7)))],
        );
        read_folded(&mv, 3, folded(Some(0), 100, &[(1, 7), (2, 5)], 112));
        assert!(
            mv.failing_location(3).is_none(),
            "executed reward plus predicted credit"
        );
        mv.record(
            &TxVersion {
                tx_idx: 1,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(loc(), MemoryValue::LazyRecipient(U256::from(9)))],
        );
        assert!(mv.failing_location(3).is_some());
    }
}
