//! `SpecFence` transaction executor.
//!
//! This is an SF-owned copy of the upstream VM. Read origins carry the value
//! the interpreter consumed. A read of a chained location waits for the
//! nearest lower writer's final publish when the cost model says to wait.
//! A retained pre-abort value lets an in-flight reader finish; it is not a
//! commit origin because validation still compares identity and value.

use std::cell::Cell;
use std::time::Instant;

use alloy_primitives::{Address, B256, TxKind, U256};
use hashbrown::HashMap;
use revm::{
    Database,
    context::{
        BlockEnv, ContextSetters, ContextTr, JournalTr, TxEnv,
        result::{EVMError, InvalidTransaction},
    },
    handler::{EvmTr, FrameResult, Handler},
    primitives::KECCAK_EMPTY,
    state::{AccountInfo, Bytecode},
};
use smallvec::SmallVec;

use crate::{
    AccountBasic, BuildIdentityHasher, MemoryEntry, MemoryLocation, MemoryLocationHash,
    MemoryValue, TxIdx, TxVersion, WriteSet,
    chain::PevmChain,
    hash_deterministic,
    vm::{
        ExecutionError, PevmTxExecutionResult, ReadError, VmExecutionError, receipt_from_revm,
        state_transitions_from_revm,
    },
};

use super::live_chain::LiveChain;
use super::mv::{
    SfMv, SfOrigin, SfReadOrigin, SfReadOrigins, SfReadSet, is_lazy_value, memory_value_eq,
    net_lazy, retained_storage,
};
use super::rt::Runtime;
use super::trace::Trace;

pub(crate) enum Step {
    Done,
    Block(TxIdx),
    /// The attempt did not finish and should not be retried ahead of the prefix.
    Yield,
    Retry,
    Fallback,
    Fatal(ExecutionError),
}

struct VmDb<'a, S: crate::Storage> {
    storage: &'a S,
    mv: &'a SfMv,
    chain: &'a LiveChain,
    rt: &'a Runtime,
    trace: &'a Trace,
    tx_idx: TxIdx,
    tx: &'a TxEnv,
    from_hash: MemoryLocationHash,
    to_hash: Option<MemoryLocationHash>,
    to_code_hash: Option<B256>,
    is_lazy: bool,
    has_nonce: bool,
    read_set: SfReadSet,
    read_accounts: HashMap<MemoryLocationHash, (AccountBasic, Option<B256>), BuildIdentityHasher>,
    /// Nearest lower transaction from the same sender, if any.
    prev_sender: &'a [Option<TxIdx>],
    /// Nearest lower writer accepted before the multi-version read.
    /// `u32::MAX` means there was none.
    accepted_pred: Cell<u32>,
    /// One worker commits in index order. Chain waits are skipped.
    serial: bool,
    /// Record read origins. Wall-clock serial skips the map.
    track: bool,
    /// Worker-local copy. Timed runs leave this false and skip the timeline clock.
    tl_on: bool,
    /// Base state this worker has already fetched. Used when the location
    /// has no writer in the block. Not consulted for a location the filter hit.
    basic_cache: HashMap<Address, Option<AccountBasic>, rustc_hash::FxBuildHasher>,
    code_hash_cache: HashMap<Address, Option<B256>, rustc_hash::FxBuildHasher>,
    code_cache: HashMap<B256, Bytecode, rustc_hash::FxBuildHasher>,
    slot_cache: HashMap<(Address, U256), U256, rustc_hash::FxBuildHasher>,
    /// `new_bytecodes` length last time the code caches were filled.
    code_seen: usize,
}

impl<'a, S: crate::Storage> VmDb<'a, S> {
    /// Armed locations settle on the final published incarnation, not on commit.
    /// A published incarnation that has not closed can still be aborted.
    /// Unarmed locations keep the publish mark.
    fn settled(&self, location: u64, pred: TxIdx) -> bool {
        if !self.chain.writer_published(location, pred) {
            return false;
        }
        if !self.chain.is_armed(location) {
            return true;
        }
        let (_, inc) = self.chain.writer_state(location, pred);
        self.rt.is_final_inc(pred, inc)
    }

    fn finished_without_write(&self, location: u64, pred: TxIdx) -> bool {
        let (state, _) = self.chain.writer_state(location, pred);
        if state == 0 || state == 3 {
            return false;
        }
        // Predicted or running, and this transaction's incarnation is final
        // without a publish. Abort clears the flag, so a retry can still write.
        self.rt.is_final_any(pred)
    }

    #[inline(always)]
    fn coordinate(&self, location: u64) -> Result<(), ReadError> {
        // One worker commits in order, so a read cannot observe an uncommitted
        // lower write. Kept inline so the directory lock is not a call.
        if self.serial {
            return Ok(());
        }
        self.coordinate_chain(location)
    }

    fn coordinate_chain(&self, location: u64) -> Result<(), ReadError> {
        let c0 = super::timeline::cyc_enter(self.tl_on);
        let _b = super::buckets::Guard::start(super::buckets::COORD);
        if self.trace.enabled() && self.chain.is_armed(location) {
            self.trace.note_read_after_arm();
        }
        let result = self.wait_writer(location);
        let accepted = self
            .chain
            .nearest_blocker(location, self.tx_idx)
            .map(|tx| tx as u32)
            .unwrap_or(u32::MAX);
        self.accepted_pred.set(accepted);
        if self.trace.diag() {
            let (nearest, state, reason) = match result {
                Ok(()) => match self.chain.nearest_blocker(location, self.tx_idx) {
                    Some(pred) => {
                        let state = self.chain.writer_state(location, pred).0 as u8;
                        let reason = if self.settled(location, pred) {
                            super::trace::coord_reason::READY_PUB
                        } else {
                            super::trace::coord_reason::SKIP_COST
                        };
                        (pred as u32, state, reason)
                    }
                    None => (u32::MAX, 0, super::trace::coord_reason::NO_LOWER),
                },
                Err(_) => {
                    let pred = self.chain.nearest_blocker(location, self.tx_idx);
                    let state = pred
                        .map(|tx| self.chain.writer_state(location, tx).0 as u8)
                        .unwrap_or(0);
                    (
                        pred.map(|tx| tx as u32).unwrap_or(u32::MAX),
                        state,
                        super::trace::coord_reason::BLOCK,
                    )
                }
            };
            self.trace.note_coord(
                self.tx_idx,
                location,
                self.chain.is_armed(location),
                nearest,
                state,
                reason,
            );
        }
        super::timeline::cyc_leave(super::timeline::CYC_COORD, c0);
        result
    }

    /// Armed locations always wait. Unarmed locations still consult the cost model.
    fn wait_writer(&self, location: u64) -> Result<(), ReadError> {
        if !self.chain.any() {
            return Ok(());
        }
        // Every final hole in front of the read is dropped before parking.
        // A fixed cap here requeued the reader once per handful of holes and
        // put the preseeded recipient chain back on the critical path.
        let mut holes = 0usize;
        loop {
            let Some(pred) = self.chain.nearest_blocker(location, self.tx_idx) else {
                return Ok(());
            };
            if self.settled(location, pred) {
                return Ok(());
            }
            if self.finished_without_write(location, pred) {
                self.chain.clear_hole(location, pred);
                holes += 1;
                if holes > self.tx_idx {
                    return Ok(());
                }
                continue;
            }
            let executing = self.rt.is_executing(pred);
            let armed = self.chain.is_armed(location);
            if !armed && !self.chain.should_wait(location, pred, executing) {
                return Ok(());
            }
            let reason = if armed {
                super::timeline::ARMED
            } else {
                super::timeline::UNARMED
            };
            if !executing {
                super::timeline::set_block(reason, location);
                return Err(ReadError::Blocking(pred));
            }
            // Overlap only while the predecessor is inside the interpreter.
            // 40 × 50µs is the cap; a longer spin holds the worker off the prefix.
            let t_inline = if self.tl_on {
                super::timeline::stamp()
            } else {
                0
            };
            self.rt.waiting_add(1);
            for _ in 0..40 {
                if self.settled(location, pred)
                    || self.finished_without_write(location, pred)
                    || !self.rt.is_executing(pred)
                    || self.rt.all_executors_waiting()
                {
                    break;
                }
                self.rt.wait_brief();
            }
            self.rt.waiting_add(-1);
            super::timeline::inline_wait(self.tx_idx, pred, location, reason, t_inline);
            if self.settled(location, pred) {
                return Ok(());
            }
            if self.finished_without_write(location, pred) {
                self.chain.clear_hole(location, pred);
                continue;
            }
            super::timeline::set_block(reason, location);
            return Err(ReadError::Blocking(pred));
        }
    }

    /// Nonce and balance checks block on the previous same-sender transaction.
    /// Blocking on `tx-1` retries forever once that unrelated transaction has committed.
    fn sender_block(&self) -> ReadError {
        match self.prev_sender.get(self.tx_idx).copied().flatten() {
            Some(pred) if !self.rt.is_committed(pred) && !self.rt.is_final_any(pred) => {
                super::timeline::set_block(super::timeline::NONCE, self.from_hash);
                ReadError::Blocking(pred)
            }
            None => ReadError::InvalidNonce(self.tx_idx),
            Some(_) => ReadError::InconsistentRead,
        }
    }

    fn set_tx(
        &mut self,
        tx_idx: TxIdx,
        tx: &'a TxEnv,
        from_hash: MemoryLocationHash,
        to_hash: Option<MemoryLocationHash>,
        has_nonce: bool,
    ) -> Result<(), ReadError> {
        self.tx_idx = tx_idx;
        self.tx = tx;
        self.from_hash = from_hash;
        self.to_hash = to_hash;
        self.to_code_hash = None;
        self.is_lazy = false;
        self.has_nonce = has_nonce;
        self.read_set.clear();
        self.read_accounts.clear();
        if let TxKind::Call(to) = tx.kind {
            self.to_code_hash = self.get_code_hash(to)?;
            self.is_lazy = self.to_code_hash.is_none()
                && (self.mv.data.contains_key(&from_hash)
                    || self.mv.data.contains_key(&to_hash.unwrap()));
            if !self.serial
                && let Some(loc) = to_hash
            {
                self.chain.note_lazy_decision(tx_idx, loc, self.is_lazy);
            }
        }
        Ok(())
    }

    fn hash_basic(&self, address: &Address) -> MemoryLocationHash {
        if address == &self.tx.caller {
            return self.from_hash;
        }
        if let TxKind::Call(to) = &self.tx.kind
            && to == address
        {
            return self.to_hash.unwrap();
        }
        hash_deterministic(MemoryLocation::Basic(*address))
    }

    fn push_origin(
        read_origins: &mut SfReadOrigins,
        origin: SfReadOrigin,
    ) -> Result<(), ReadError> {
        if let Some(prev) = read_origins.last() {
            if !origin_eq(prev, &origin) {
                return Err(ReadError::InconsistentRead);
            }
        } else {
            read_origins.push(origin);
        }
        Ok(())
    }

    fn get_code_hash(&mut self, address: Address) -> Result<Option<B256>, ReadError> {
        if !self.track {
            return self.code_hash_untracked(address);
        }
        let location_hash = hash_deterministic(MemoryLocation::CodeHash(address));
        self.mv.note_reader(location_hash);
        if self.cold_location(location_hash) {
            super::buckets::hit(super::buckets::READ_COLD);
            self.note_storage_origin(location_hash)?;
            let hash = if let Some(hit) = self.code_hash_cache.get(&address) {
                *hit
            } else {
                let _b = super::buckets::Guard::start(super::buckets::READ_BASE);
                let hash = self
                    .storage
                    .code_hash(&address)
                    .map_err(|err| ReadError::StorageError(err.to_string()))?;
                self.code_hash_cache.insert(address, hash);
                hash
            };
            if self.cold_location(location_hash) {
                return Ok(hash);
            }
            self.read_set.remove(&location_hash);
        }
        let serial = self.serial;
        self.coordinate(location_hash)?;
        let read_origins = self.read_set.entry(location_hash).or_default();
        if let Some(written_transactions) = self.mv.data.get(&location_hash)
            && let Some((tx_idx, entry)) = written_transactions.range(..self.tx_idx).next_back()
        {
            match entry {
                MemoryEntry::Estimate => {
                    super::timeline::set_block(super::timeline::ESTIMATE, location_hash);
                    return Err(ReadError::Blocking(*tx_idx));
                }
                MemoryEntry::Data(_, MemoryValue::SelfDestructed) => {
                    return Err(ReadError::SelfDestructedAccount);
                }
                MemoryEntry::Data(tx_incarnation, MemoryValue::CodeHash(code_hash)) => {
                    confirm_read(
                        self.chain,
                        self.rt,
                        self.tx_idx,
                        self.accepted_pred.get(),
                        location_hash,
                        serial,
                    )?;
                    Self::push_origin(
                        read_origins,
                        mv_origin(
                            serial,
                            *tx_idx,
                            *tx_incarnation,
                            &MemoryValue::CodeHash(*code_hash),
                        ),
                    )?;
                    return Ok(Some(*code_hash));
                }
                MemoryEntry::Data(_, _) => {}
            }
        }
        confirm_read(
            self.chain,
            self.rt,
            self.tx_idx,
            self.accepted_pred.get(),
            location_hash,
            serial,
        )?;
        Self::push_origin(read_origins, SfReadOrigin::Storage)?;
        self.storage
            .code_hash(&address)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }

    /// Code hash without a read-set entry. Serial execution already published
    /// every lower write.
    fn code_hash_untracked(&mut self, address: Address) -> Result<Option<B256>, ReadError> {
        let location_hash = hash_deterministic(MemoryLocation::CodeHash(address));
        if self.tx_idx > 0
            && let Some(written) = self.mv.data.get(&location_hash)
            && let Some((_, entry)) = written.range(..self.tx_idx).next_back()
        {
            match entry {
                MemoryEntry::Data(_, MemoryValue::SelfDestructed) => {
                    return Err(ReadError::SelfDestructedAccount);
                }
                MemoryEntry::Data(_, MemoryValue::CodeHash(code_hash)) => {
                    return Ok(Some(*code_hash));
                }
                MemoryEntry::Estimate => {}
                MemoryEntry::Data(_, _) => {}
            }
        }
        self.storage
            .code_hash(&address)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }
}

impl<'a, S: crate::Storage> VmDb<'a, S> {
    /// Account read that does not record an origin. Used when one worker runs
    /// the block in order and the harness is not building the ideal DAG.
    fn basic_untracked(
        &mut self,
        address: Address,
        location_hash: MemoryLocationHash,
    ) -> Result<Option<AccountInfo>, ReadError> {
        let mut balance_addition = U256::ZERO;
        let mut positive_addition = true;
        let mut nonce_addition = 0u64;
        let mut final_account = None;
        if self.tx_idx > 0
            && let Some(written) = self.mv.data.get(&location_hash)
        {
            let mut iter = written.range(..self.tx_idx);
            loop {
                match iter.next_back() {
                    Some((_, MemoryEntry::Estimate)) => break,
                    Some((_, MemoryEntry::Data(_, value))) => match value {
                        MemoryValue::Basic(basic) => {
                            final_account = Some(basic.clone());
                            break;
                        }
                        MemoryValue::LazyRecipient(addition) => {
                            if positive_addition {
                                balance_addition = balance_addition.saturating_add(*addition);
                            } else {
                                positive_addition = *addition >= balance_addition;
                                balance_addition = balance_addition.abs_diff(*addition);
                            }
                        }
                        MemoryValue::LazySender(subtraction) => {
                            if positive_addition {
                                positive_addition = balance_addition >= *subtraction;
                                balance_addition = balance_addition.abs_diff(*subtraction);
                            } else {
                                balance_addition = balance_addition.saturating_add(*subtraction);
                            }
                            nonce_addition += 1;
                        }
                        _ => return Err(ReadError::InvalidMemoryValueType),
                    },
                    None => break,
                }
            }
        }
        if final_account.is_none() {
            final_account = match self.storage.basic(&address) {
                Ok(Some(basic)) => Some(basic),
                Ok(None) => (balance_addition > U256::ZERO).then(AccountBasic::default),
                Err(err) => return Err(ReadError::StorageError(err.to_string())),
            };
        }
        let Some(mut account) = final_account else {
            return Ok(None);
        };
        account.nonce += nonce_addition;
        if self.has_nonce && location_hash == self.from_hash && self.tx.nonce != account.nonce {
            return Err(self.sender_block());
        }
        if positive_addition {
            account.balance = account.balance.saturating_add(balance_addition);
        } else {
            account.balance = account.balance.saturating_sub(balance_addition);
        }
        let code_hash = if Some(location_hash) == self.to_hash {
            self.to_code_hash
        } else {
            self.code_hash_untracked(address)?
        };
        let code = self.cached_code(code_hash)?;
        self.read_accounts
            .insert(location_hash, (account.clone(), code_hash));
        Ok(Some(AccountInfo {
            balance: account.balance,
            nonce: account.nonce,
            code_hash: code_hash.unwrap_or(KECCAK_EMPTY),
            code,
            account_id: None,
        }))
    }

    fn storage_untracked(
        &mut self,
        address: Address,
        index: U256,
        location_hash: MemoryLocationHash,
    ) -> Result<U256, ReadError> {
        if self.tx_idx > 0
            && let Some(written) = self.mv.data.get(&location_hash)
            && let Some((_, entry)) = written.range(..self.tx_idx).next_back()
        {
            match entry {
                MemoryEntry::Data(_, MemoryValue::Storage(value)) => return Ok(*value),
                MemoryEntry::Estimate => {}
                _ => return Err(ReadError::InvalidMemoryValueType),
            }
        }
        self.storage
            .storage(&address, &index)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }
}

impl<'a, S: crate::Storage> VmDb<'a, S> {
    /// No chain slot and no multi-version write has been published for `location`.
    fn cold_location(&self, location: u64) -> bool {
        if self.serial || !self.track {
            return false;
        }
        !self.mv.might_hold(location) && !self.chain.might_hold(location)
    }

    /// Storage origin for a speculative cold read.
    ///
    /// A committed prefix cannot grow a lower write, so the origin would only
    /// name storage. Validation still compares a recorded storage origin if a
    /// lower transaction publishes after this read.
    fn note_storage_origin(&mut self, location: u64) -> Result<(), ReadError> {
        if self.rt.committed() >= self.tx_idx {
            return Ok(());
        }
        let _b = super::buckets::Guard::start(super::buckets::READ_ORIGIN);
        let origins = self.read_set.entry(location).or_default();
        if origins.is_empty() {
            origins.push(SfReadOrigin::Storage);
        } else if !matches!(origins.last(), Some(SfReadOrigin::Storage)) {
            return Err(ReadError::InconsistentRead);
        }
        Ok(())
    }

    fn cached_basic(&mut self, address: &Address) -> Result<Option<AccountBasic>, ReadError> {
        if let Some(hit) = self.basic_cache.get(address) {
            return Ok(hit.clone());
        }
        let _b = super::buckets::Guard::start(super::buckets::READ_BASE);
        let basic = self
            .storage
            .basic(address)
            .map_err(|err| ReadError::StorageError(err.to_string()))?;
        self.basic_cache.insert(*address, basic.clone());
        Ok(basic)
    }

    fn cached_code(&mut self, code_hash: Option<B256>) -> Result<Option<Bytecode>, ReadError> {
        let Some(code_hash) = code_hash else {
            return Ok(None);
        };
        let len = self.mv.new_bytecodes.len();
        if len != self.code_seen {
            self.code_cache.clear();
            self.code_hash_cache.clear();
            self.code_seen = len;
        }
        if let Some(code) = self.code_cache.get(&code_hash) {
            return Ok(Some(code.clone()));
        }
        let _b = super::buckets::Guard::start(super::buckets::READ_CODE);
        if let Some(code) = self.mv.new_bytecodes.get(&code_hash) {
            let code = code.clone();
            self.code_cache.insert(code_hash, code.clone());
            return Ok(Some(code));
        }
        match self.storage.code_by_hash(&code_hash) {
            Ok(Some(evm_code)) => {
                let code = Bytecode::from(evm_code);
                self.code_cache.insert(code_hash, code.clone());
                Ok(Some(code))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(ReadError::StorageError(err.to_string())),
        }
    }

    fn basic_cold(
        &mut self,
        address: Address,
        location_hash: MemoryLocationHash,
    ) -> Result<Option<AccountInfo>, ReadError> {
        self.note_storage_origin(location_hash)?;
        let Some(account) = self.cached_basic(&address)? else {
            return Ok(None);
        };
        if self.has_nonce && location_hash == self.from_hash && self.tx.nonce != account.nonce {
            return Err(self.sender_block());
        }
        let code_hash = if Some(location_hash) == self.to_hash {
            self.to_code_hash
        } else {
            self.code_hash_cold(address)?
        };
        let code = self.cached_code(code_hash)?;
        self.read_accounts
            .insert(location_hash, (account.clone(), code_hash));
        Ok(Some(AccountInfo {
            balance: account.balance,
            nonce: account.nonce,
            code_hash: code_hash.unwrap_or(KECCAK_EMPTY),
            code,
            account_id: None,
        }))
    }

    fn code_hash_cold(&mut self, address: Address) -> Result<Option<B256>, ReadError> {
        let location_hash = hash_deterministic(MemoryLocation::CodeHash(address));
        self.mv.note_reader(location_hash);
        if !self.cold_location(location_hash) {
            return self.get_code_hash(address);
        }
        super::buckets::hit(super::buckets::READ_COLD);
        self.note_storage_origin(location_hash)?;
        if let Some(hit) = self.code_hash_cache.get(&address) {
            return Ok(*hit);
        }
        let _b = super::buckets::Guard::start(super::buckets::READ_BASE);
        let hash = self
            .storage
            .code_hash(&address)
            .map_err(|err| ReadError::StorageError(err.to_string()))?;
        self.code_hash_cache.insert(address, hash);
        Ok(hash)
    }

    fn storage_cold(
        &mut self,
        address: Address,
        index: U256,
        location_hash: MemoryLocationHash,
    ) -> Result<U256, ReadError> {
        self.note_storage_origin(location_hash)?;
        if let Some(value) = self.slot_cache.get(&(address, index)) {
            return Ok(*value);
        }
        let _b = super::buckets::Guard::start(super::buckets::READ_BASE);
        let value = self
            .storage
            .storage(&address, &index)
            .map_err(|err| ReadError::StorageError(err.to_string()))?;
        self.slot_cache.insert((address, index), value);
        Ok(value)
    }
}

#[inline(always)]
fn confirm_read(
    chain: &LiveChain,
    rt: &Runtime,
    tx_idx: TxIdx,
    accepted: u32,
    location: u64,
    serial: bool,
) -> Result<(), ReadError> {
    if serial {
        return Ok(());
    }
    confirm_read_chain(chain, rt, tx_idx, accepted, location)
}

fn confirm_read_chain(
    chain: &LiveChain,
    rt: &Runtime,
    tx_idx: TxIdx,
    accepted: u32,
    location: u64,
) -> Result<(), ReadError> {
    let now = chain
        .nearest_blocker(location, tx_idx)
        .map(|tx| tx as u32)
        .unwrap_or(u32::MAX);
    if now != accepted {
        if now != u32::MAX {
            let pred = now as TxIdx;
            if chain.is_armed(location) && !writer_final(chain, rt, location, pred) {
                super::timeline::set_block(super::timeline::ARMED, location);
                return Err(ReadError::Blocking(pred));
            }
        }
        return Err(ReadError::InconsistentRead);
    }
    if accepted != u32::MAX {
        let pred = accepted as TxIdx;
        if chain.is_armed(location) && !writer_final(chain, rt, location, pred) {
            super::timeline::set_block(super::timeline::ARMED, location);
            return Err(ReadError::Blocking(pred));
        }
    }
    Ok(())
}

/// The nearest chain writer has a final published value, or is final without
/// writing this location (a hole the caller may skip). Commit is not required.
fn writer_final(chain: &LiveChain, rt: &Runtime, location: u64, pred: TxIdx) -> bool {
    let (state, inc) = chain.writer_state(location, pred);
    if chain.writer_published(location, pred) {
        return rt.is_final_inc(pred, inc);
    }
    state != 0 && rt.is_final_any(pred)
}

fn mv_origin(serial: bool, tx_idx: TxIdx, incarnation: usize, value: &MemoryValue) -> SfReadOrigin {
    if serial {
        SfReadOrigin::MvId {
            tx_idx,
            incarnation,
        }
    } else {
        SfReadOrigin::Mv(SfOrigin {
            tx_idx,
            incarnation,
            value: value.clone(),
        })
    }
}

fn install_fold(
    read_origins: &mut SfReadOrigins,
    replay_folded: bool,
    base_tx: Option<(TxIdx, usize)>,
    base_account: Option<&AccountBasic>,
    folds: &smallvec::SmallVec<[super::mv::DeltaFold; 4]>,
    consumed: &AccountBasic,
) -> Result<(), ReadError> {
    let fold = SfReadOrigin::Folded(super::mv::FoldedRead {
        base_tx: base_tx.map(|(tx, _)| tx),
        base_incarnation: base_tx.map(|(_, inc)| inc).unwrap_or(0),
        base: MemoryValue::Basic(base_account.cloned().unwrap_or_default()),
        deltas: folds.clone(),
        consumed: consumed.clone(),
    });
    if replay_folded {
        if !origin_eq(read_origins.first().unwrap(), &fold) {
            return Err(ReadError::InconsistentRead);
        }
    } else {
        read_origins.clear();
        read_origins.push(fold);
    }
    Ok(())
}

fn origin_eq(a: &SfReadOrigin, b: &SfReadOrigin) -> bool {
    match (a, b) {
        (SfReadOrigin::Storage, SfReadOrigin::Storage) => true,
        (SfReadOrigin::Mv(x), SfReadOrigin::Mv(y)) => {
            x.tx_idx == y.tx_idx
                && x.incarnation == y.incarnation
                && memory_value_eq(&x.value, &y.value)
        }
        (
            SfReadOrigin::MvId {
                tx_idx: a,
                incarnation: ai,
            },
            SfReadOrigin::MvId {
                tx_idx: b,
                incarnation: bi,
            },
        ) => a == b && ai == bi,
        (SfReadOrigin::Folded(a), SfReadOrigin::Folded(b)) => {
            a.base_tx == b.base_tx
                && a.base_incarnation == b.base_incarnation
                && memory_value_eq(&a.base, &b.base)
                && a.consumed == b.consumed
                && a.deltas.len() == b.deltas.len()
                && a.deltas
                    .iter()
                    .zip(b.deltas.iter())
                    .all(|(left, right)| left.tx_idx == right.tx_idx && left.amount == right.amount)
        }
        _ => false,
    }
}

impl<S: crate::Storage> Database for VmDb<'_, S> {
    type Error = ReadError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let location_hash = self.hash_basic(&address);
        let serial = self.serial;
        if self.is_lazy {
            if location_hash == self.from_hash {
                return Ok(Some(AccountInfo {
                    nonce: self.tx.nonce,
                    balance: U256::MAX,
                    code: None,
                    code_hash: KECCAK_EMPTY,
                    account_id: None,
                }));
            } else if Some(location_hash) == self.to_hash {
                return Ok(None);
            }
        }

        if !self.track {
            return self.basic_untracked(address, location_hash);
        }
        self.mv.note_reader(location_hash);
        if self.cold_location(location_hash) {
            super::buckets::hit(super::buckets::READ_COLD);
            let cold = self.basic_cold(address, location_hash)?;
            if self.cold_location(location_hash) {
                return Ok(cold);
            }
            // A lower write landed during the base read. Drop the storage
            // origin and take the chain path with the published value.
            self.read_set.remove(&location_hash);
        }

        self.coordinate(location_hash)?;

        let read_origins = {
            let _b = super::buckets::Guard::start(super::buckets::READ_ORIGIN);
            self.read_set.entry(location_hash).or_default()
        };
        let has_prev_origins = !read_origins.is_empty();
        let replay_folded =
            has_prev_origins && matches!(read_origins.first(), Some(SfReadOrigin::Folded(_)));
        let mut new_origins = SmallVec::new();
        let mut final_account = None;
        let mut balance_addition = U256::ZERO;
        let mut positive_addition = true;
        let mut nonce_addition = 0u64;
        let mut lazy_ops: Vec<(TxIdx, bool, U256)> = Vec::new();
        let mut base_tx: Option<(TxIdx, usize)> = None;

        let written_transactions = {
            let _b = super::buckets::Guard::start(super::buckets::READ_MV);
            if self.tx_idx > 0 {
                self.mv.data.get(&location_hash)
            } else {
                None
            }
        };
        if let Some(written_transactions) = written_transactions {
            let mut iter = written_transactions.range(..self.tx_idx);
            loop {
                match iter.next_back() {
                    Some((blocking_idx, MemoryEntry::Estimate)) => {
                        if let Some((basic, code_hash)) = self
                            .read_accounts
                            .get(&location_hash)
                            .map(|(basic, code_hash)| (basic.clone(), *code_hash))
                        {
                            // Retained pre-abort account. The origin is left unchanged,
                            // so a commit still requires the live value to match it.
                            return Ok(Some(AccountInfo {
                                balance: basic.balance,
                                nonce: basic.nonce,
                                code_hash: code_hash.unwrap_or(KECCAK_EMPTY),
                                code: None,
                                account_id: None,
                            }));
                        }
                        super::timeline::set_block(super::timeline::ESTIMATE, location_hash);
                        return Err(ReadError::Blocking(*blocking_idx));
                    }
                    Some((closest_idx, MemoryEntry::Data(tx_incarnation, value))) => {
                        if !replay_folded {
                            if has_prev_origins && read_origins.len() == new_origins.len() {
                                return Err(ReadError::InconsistentRead);
                            }
                            let origin = mv_origin(serial, *closest_idx, *tx_incarnation, value);
                            if has_prev_origins
                                && !origin_eq(&read_origins[new_origins.len()], &origin)
                            {
                                return Err(ReadError::InconsistentRead);
                            }
                            new_origins.push(origin);
                        }
                        match value {
                            MemoryValue::Basic(basic) => {
                                base_tx = Some((*closest_idx, *tx_incarnation));
                                final_account = Some(basic.clone());
                                break;
                            }
                            MemoryValue::LazyRecipient(addition) => {
                                lazy_ops.push((*closest_idx, false, *addition));
                                if positive_addition {
                                    balance_addition = balance_addition.saturating_add(*addition);
                                } else {
                                    positive_addition = *addition >= balance_addition;
                                    balance_addition = balance_addition.abs_diff(*addition);
                                }
                            }
                            MemoryValue::LazySender(subtraction) => {
                                lazy_ops.push((*closest_idx, true, *subtraction));
                                if positive_addition {
                                    positive_addition = balance_addition >= *subtraction;
                                    balance_addition = balance_addition.abs_diff(*subtraction);
                                } else {
                                    balance_addition =
                                        balance_addition.saturating_add(*subtraction);
                                }
                                nonce_addition += 1;
                            }
                            _ => return Err(ReadError::InvalidMemoryValueType),
                        }
                    }
                    None => break,
                }
            }
        }

        let mut folds: smallvec::SmallVec<[super::mv::DeltaFold; 4]> = smallvec::SmallVec::new();
        if !serial {
            let start = base_tx.map(|(tx, _)| tx.saturating_add(1)).unwrap_or(0);
            self.chain
                .for_each_delta(location_hash, start, self.tx_idx, |tx, amount| {
                    let used = lazy_ops
                        .iter()
                        .find(|(seen, sender, _)| *seen == tx && !*sender)
                        .map(|(_, _, executed)| *executed)
                        .unwrap_or(amount);
                    folds.push(super::mv::DeltaFold {
                        tx_idx: tx,
                        amount: used,
                    });
                });
        }
        let folding = folds
            .iter()
            .any(|delta| lazy_ops.iter().all(|(seen, _, _)| *seen != delta.tx_idx));
        if folding {
            if has_prev_origins && !replay_folded {
                return Err(ReadError::InconsistentRead);
            }
            let mut ops = lazy_ops.clone();
            for delta in &folds {
                if lazy_ops.iter().all(|(seen, _, _)| *seen != delta.tx_idx) {
                    ops.push((delta.tx_idx, false, delta.amount));
                }
            }
            ops.sort_by(|left, right| right.0.cmp(&left.0));
            let net_ops: Vec<(bool, U256)> = ops
                .iter()
                .map(|(_, sender, amount)| (*sender, *amount))
                .collect();
            let (positive, addition, nonce) = net_lazy(&net_ops);
            positive_addition = positive;
            balance_addition = addition;
            nonce_addition = nonce;
        }

        if final_account.is_none() {
            if folding || replay_folded {
                // The folded origin replaces the storage marker.
            } else if !has_prev_origins {
                new_origins.push(SfReadOrigin::Storage);
            } else if read_origins.len() != new_origins.len() + 1
                || !matches!(read_origins.last(), Some(SfReadOrigin::Storage))
            {
                return Err(ReadError::InconsistentRead);
            }
            final_account = match self.storage.basic(&address) {
                Ok(Some(basic)) => Some(basic),
                Ok(None) => (balance_addition > U256::ZERO).then(AccountBasic::default),
                Err(err) => return Err(ReadError::StorageError(err.to_string())),
            };
        }

        confirm_read(
            self.chain,
            self.rt,
            self.tx_idx,
            self.accepted_pred.get(),
            location_hash,
            serial,
        )?;
        let base_account = final_account.clone();
        if !folding && !has_prev_origins {
            *read_origins = new_origins;
        }

        if let Some(mut account) = final_account {
            account.nonce += nonce_addition;
            if self.has_nonce && location_hash == self.from_hash && self.tx.nonce != account.nonce {
                return Err(self.sender_block());
            }
            if positive_addition {
                account.balance = account.balance.saturating_add(balance_addition);
            } else {
                account.balance = account.balance.saturating_sub(balance_addition);
            }
            if folding {
                install_fold(
                    read_origins,
                    replay_folded,
                    base_tx,
                    base_account.as_ref(),
                    &folds,
                    &account,
                )?;
            }
            let code_hash = if Some(location_hash) == self.to_hash {
                self.to_code_hash
            } else {
                self.get_code_hash(address)?
            };
            let code = self.cached_code(code_hash)?;
            self.read_accounts
                .insert(location_hash, (account.clone(), code_hash));
            return Ok(Some(AccountInfo {
                balance: account.balance,
                nonce: account.nonce,
                code_hash: code_hash.unwrap_or(KECCAK_EMPTY),
                code,
                account_id: None,
            }));
        }
        if folding {
            // No account was visible. The origin still names every predicted
            // credit, so a later non-zero write fails the value compare.
            let empty = AccountBasic::default();
            install_fold(
                read_origins,
                replay_folded,
                base_tx,
                base_account.as_ref(),
                &folds,
                &empty,
            )?;
        }
        Ok(None)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        match self.cached_code(Some(code_hash))? {
            Some(code) => Ok(code),
            None => Ok(Bytecode::default()),
        }
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let location_hash = hash_deterministic(MemoryLocation::Storage(address, index));
        if !self.track {
            return self.storage_untracked(address, index, location_hash);
        }
        self.mv.note_reader(location_hash);
        if self.cold_location(location_hash) {
            super::buckets::hit(super::buckets::READ_COLD);
            let cold = self.storage_cold(address, index, location_hash)?;
            if self.cold_location(location_hash) {
                return Ok(cold);
            }
            self.read_set.remove(&location_hash);
        }
        let serial = self.serial;
        self.coordinate(location_hash)?;
        let read_origins = {
            let _b = super::buckets::Guard::start(super::buckets::READ_ORIGIN);
            self.read_set.entry(location_hash).or_default()
        };
        let written_transactions = {
            let _b = super::buckets::Guard::start(super::buckets::READ_MV);
            if self.tx_idx > 0 {
                self.mv.data.get(&location_hash)
            } else {
                None
            }
        };
        if let Some(written_transactions) = written_transactions
            && let Some((closest_idx, entry)) =
                written_transactions.range(..self.tx_idx).next_back()
        {
            match entry {
                MemoryEntry::Data(tx_incarnation, MemoryValue::Storage(value)) => {
                    confirm_read(
                        self.chain,
                        self.rt,
                        self.tx_idx,
                        self.accepted_pred.get(),
                        location_hash,
                        serial,
                    )?;
                    Self::push_origin(
                        read_origins,
                        mv_origin(
                            serial,
                            *closest_idx,
                            *tx_incarnation,
                            &MemoryValue::Storage(*value),
                        ),
                    )?;
                    return Ok(*value);
                }
                MemoryEntry::Estimate => {
                    if let Some(value) = retained_storage(read_origins) {
                        return Ok(value);
                    }
                    super::timeline::set_block(super::timeline::ESTIMATE, location_hash);
                    return Err(ReadError::Blocking(*closest_idx));
                }
                _ => return Err(ReadError::InvalidMemoryValueType),
            }
        }
        confirm_read(
            self.chain,
            self.rt,
            self.tx_idx,
            self.accepted_pred.get(),
            location_hash,
            serial,
        )?;
        Self::push_origin(read_origins, SfReadOrigin::Storage)?;
        self.storage
            .storage(&address, &index)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.storage
            .block_hash(&number)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }
}

pub(crate) struct SfVm<'a, S: crate::Storage, C: PevmChain> {
    chain: &'a C,
    is_eip_161_enabled: bool,
    block_env: &'a BlockEnv,
    txs: &'a [C::EvmTx],
    mv: &'a SfMv,
    live: &'a LiveChain,
    rt: &'a Runtime,
    trace: &'a Trace,
    prev_sender: &'a [Option<TxIdx>],
    beneficiary_location_hash: MemoryLocationHash,
    evm: C::Evm<VmDb<'a, S>>,
}

impl<'a, S: crate::Storage, C: PevmChain> SfVm<'a, S, C> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        chain: &'a C,
        spec_id: C::EvmSpecId,
        block_env: &'a BlockEnv,
        txs: &'a [C::EvmTx],
        storage: &'a S,
        mv: &'a SfMv,
        live: &'a LiveChain,
        rt: &'a Runtime,
        trace: &'a Trace,
        prev_sender: &'a [Option<TxIdx>],
        fast: bool,
    ) -> Self {
        let db = VmDb {
            storage,
            mv,
            chain: live,
            rt,
            trace,
            tx_idx: 0,
            tx: chain.tx_env(unsafe { txs.get_unchecked(0) }),
            from_hash: 0,
            to_hash: None,
            to_code_hash: None,
            is_lazy: false,
            has_nonce: true,
            read_set: SfReadSet::with_capacity_and_hasher(2, BuildIdentityHasher::default()),
            read_accounts: HashMap::with_capacity_and_hasher(2, BuildIdentityHasher::default()),
            prev_sender,
            accepted_pred: Cell::new(u32::MAX),
            serial: fast,
            track: !fast || trace.profile() || trace.enabled(),
            tl_on: super::timeline::vm_enabled(),
            basic_cache: HashMap::with_hasher(rustc_hash::FxBuildHasher),
            code_hash_cache: HashMap::with_hasher(rustc_hash::FxBuildHasher),
            code_cache: HashMap::with_hasher(rustc_hash::FxBuildHasher),
            slot_cache: HashMap::with_hasher(rustc_hash::FxBuildHasher),
            code_seen: 0,
        };
        Self {
            chain,
            is_eip_161_enabled: chain.is_eip_161_enabled(spec_id),
            block_env,
            txs,
            mv,
            live,
            rt,
            trace,
            prev_sender,
            beneficiary_location_hash: hash_deterministic(MemoryLocation::Basic(
                block_env.beneficiary,
            )),
            // Stock builder. An opcode wrapper, if one is added later, is
            // installed on this EVM only and keeps `static_gas()`.
            evm: chain.build_evm(spec_id, block_env.clone(), db),
        }
    }

    pub(crate) fn execute(
        &mut self,
        tx_idx: TxIdx,
        incarnation: usize,
        result_slot: &mut Option<PevmTxExecutionResult>,
    ) -> Step {
        // The clock is the old ExecPhase regression when it runs on every
        // transaction. Wall-clock runs leave both flags off.
        super::timeline::set_block(super::timeline::OTHER, 0);
        let mut exec_span = super::timeline::ExecSpan::begin(tx_idx);
        let clock = self.trace.timing().then(Instant::now);
        let _c_pre = super::timeline::CycGuard::enter(exec_span.hot(), super::timeline::CYC_PRE);
        let _pre = super::buckets::Guard::start(super::buckets::PRE);
        self.trace.note_exec(incarnation);
        if !self.evm.ctx().db().serial {
            let _c_mark =
                super::timeline::CycGuard::enter(exec_span.hot(), super::timeline::CYC_MARK);
            let _b = super::buckets::Guard::start(super::buckets::MARK);
            self.live.mark_running(tx_idx, incarnation);
        }
        let tx_version = TxVersion {
            tx_idx,
            tx_incarnation: incarnation,
        };
        let full_tx = unsafe { self.txs.get_unchecked(tx_idx) };
        let tx = self.chain.tx_env(full_tx);
        let from_hash = hash_deterministic(MemoryLocation::Basic(tx.caller));
        let to_hash = tx
            .kind
            .to()
            .map(|to| hash_deterministic(MemoryLocation::Basic(*to)));
        let has_nonce = self.chain.has_nonce(&mut self.evm, full_tx);
        {
            let ctx = self.evm.ctx();
            if let Err(err) = ctx
                .db_mut()
                .set_tx(tx_idx, tx, from_hash, to_hash, has_nonce)
            {
                return step_from_read(err);
            }
            ctx.set_tx(full_tx.clone());
            ctx.journal_mut().clear();
        }

        drop(_pre);
        drop(_c_pre);
        let class_bucket = if self.evm.ctx().db().is_lazy {
            super::buckets::CLASS_PLAIN
        } else if self.live.is_hot_contract(tx_idx) {
            super::buckets::CLASS_HOT
        } else {
            super::buckets::CLASS_OTHER
        };
        let class_t0 = super::buckets::stamp();
        let exec_result = {
            let _b = super::buckets::Guard::start(super::buckets::INTERP);
            match NoBeneficiaryHandler::<C, _>::default().run(&mut self.evm) {
                Ok(result) => result,
                Err(EVMError::Database(read_error)) => return step_from_read(read_error),
                Err(err) => {
                    if matches!(
                        err,
                        EVMError::Transaction(
                            InvalidTransaction::LackOfFundForMaxFee { .. }
                                | InvalidTransaction::NonceTooHigh { .. }
                        )
                    ) {
                        if let Some(pred) = self.prev_sender.get(tx_idx).copied().flatten()
                            && !self.rt.is_committed(pred)
                            && !self.rt.is_final_any(pred)
                        {
                            super::timeline::set_block(super::timeline::NONCE, from_hash);
                            return Step::Block(pred);
                        }
                        return Step::Yield;
                    }
                    return Step::Fatal(err);
                }
            }
        };
        super::buckets::add_since(class_bucket, class_t0);
        if incarnation > 0 {
            super::buckets::add_since(super::buckets::CLASS_REEXEC, class_t0);
        }

        let _c_write =
            super::timeline::CycGuard::enter(exec_span.hot(), super::timeline::CYC_WRITE);
        let _writes = super::buckets::Guard::start(super::buckets::WRITESET);
        let mut write_set = WriteSet::with_capacity(6);
        let ctx = self.evm.ctx();
        let state = ctx.journal_mut().finalize();
        for (address, account) in &state {
            if account.is_selfdestructed() {
                write_set.push((
                    hash_deterministic(MemoryLocation::CodeHash(*address)),
                    MemoryValue::SelfDestructed,
                ));
                continue;
            }
            if account.is_touched() {
                let account_location_hash = hash_deterministic(MemoryLocation::Basic(*address));
                let read_account = ctx.db().read_accounts.get(&account_location_hash);
                let has_code = !account.info.is_empty_code_hash();
                let is_new_code =
                    has_code && read_account.is_none_or(|(_, code_hash)| code_hash.is_none());
                if is_new_code
                    || read_account.is_none()
                    || read_account.is_some_and(|(basic, _)| {
                        basic.nonce != account.info.nonce || basic.balance != account.info.balance
                    })
                {
                    if ctx.db().is_lazy {
                        if account_location_hash == from_hash {
                            write_set.push((
                                account_location_hash,
                                MemoryValue::LazySender(U256::MAX - account.info.balance),
                            ));
                        } else if Some(account_location_hash) == to_hash {
                            write_set.push((
                                account_location_hash,
                                MemoryValue::LazyRecipient(tx.value),
                            ));
                        }
                    } else if !self.is_eip_161_enabled || !account.is_empty() {
                        write_set.push((
                            account_location_hash,
                            MemoryValue::Basic(AccountBasic {
                                balance: account.info.balance,
                                nonce: account.info.nonce,
                            }),
                        ));
                    }
                }
                if is_new_code {
                    write_set.push((
                        hash_deterministic(MemoryLocation::CodeHash(*address)),
                        MemoryValue::CodeHash(account.info.code_hash),
                    ));
                    self.mv
                        .new_bytecodes
                        .entry(account.info.code_hash)
                        .or_insert_with(|| account.info.code.clone().unwrap());
                }
            }
            for (slot, value) in account.changed_storage_slots() {
                write_set.push((
                    hash_deterministic(MemoryLocation::Storage(*address, *slot)),
                    MemoryValue::Storage(value.present_value),
                ));
            }
        }

        let mut gas_price = if let Some(priority_fee) = tx.gas_priority_fee {
            std::cmp::min(
                tx.gas_price,
                priority_fee.saturating_add(self.block_env.basefee as u128),
            )
        } else {
            tx.gas_price
        };
        if self.is_eip_161_enabled {
            gas_price = gas_price.saturating_sub(self.block_env.basefee as u128);
        }
        let rewards = self.chain.get_rewards(
            self.beneficiary_location_hash,
            U256::from(exec_result.tx_gas_used()),
            U256::from(gas_price),
            self.block_env.basefee,
            full_tx,
        );
        for (recipient, amount) in rewards {
            if let Some((_, value)) = write_set
                .iter_mut()
                .find(|(location, _)| location == &recipient)
            {
                match value {
                    MemoryValue::Basic(basic) => {
                        basic.balance = basic.balance.saturating_add(amount)
                    }
                    MemoryValue::LazySender(subtraction) => {
                        *subtraction = subtraction.saturating_sub(amount)
                    }
                    MemoryValue::LazyRecipient(addition) => {
                        *addition = addition.saturating_add(amount)
                    }
                    _ => {
                        return Step::Fatal(EVMError::Database(ReadError::InvalidMemoryValueType));
                    }
                }
            } else {
                write_set.push((recipient, MemoryValue::LazyRecipient(amount)));
            }
        }

        let (is_lazy, read_set) = {
            let db = ctx.db_mut();
            (db.is_lazy, std::mem::take(&mut db.read_set))
        };
        if is_lazy {
            self.mv
                .add_lazy_addresses([tx.caller, *tx.kind.to().unwrap()]);
        }

        // Chain publish follows the multi-version record so a reader that
        // observes `Published` also observes the value. `Running` was set at
        // entry, which is what an armed reader waits on.
        let published: Vec<(u64, bool, bool)> = if self.evm.ctx().db().serial {
            Vec::new()
        } else {
            let read_keys: Vec<u64> = read_set.keys().copied().collect();
            write_set
                .iter()
                .filter_map(|(location, value)| {
                    let lazy_credit = matches!(value, MemoryValue::LazyRecipient(_));
                    let rmw = read_keys.contains(location) && !is_lazy_value(value);
                    // A key with no chain and no reader stays in multi-version
                    // memory until commit. Publishing it would only take the
                    // directory lock. An armed chain, a reader, or a
                    // read-then-write still publishes so the next hop can wait.
                    let needed =
                        rmw || self.live.might_hold(*location) || self.mv.reader_seen(*location);
                    needed.then_some((*location, rmw, lazy_credit))
                })
                .collect()
        };

        let elapsed_ns = clock.map(|t| t.elapsed().as_nanos() as u64);
        if self.trace.profile() {
            let reads: Vec<u64> = read_set.keys().copied().collect();
            let mut writes = Vec::new();
            let mut lazy_writes = Vec::new();
            for (location, value) in &write_set {
                if is_lazy_value(value) {
                    lazy_writes.push(*location);
                } else {
                    writes.push(*location);
                }
            }
            self.trace.note_attempt(
                tx_idx,
                incarnation,
                elapsed_ns.unwrap_or(0),
                reads,
                writes,
                lazy_writes,
            );
        }
        if let Some(ns) = elapsed_ns.filter(|_| self.trace.enabled()) {
            self.trace.note_tx_ns(tx_idx, ns);
        }
        drop(_writes);
        drop(_c_write);
        {
            let _c_rec =
                super::timeline::CycGuard::enter(exec_span.hot(), super::timeline::CYC_RECORD);
            let _b = super::buckets::Guard::start(super::buckets::RECORD);
            self.mv.record(&tx_version, read_set, write_set);
        }
        {
            let _c_pub =
                super::timeline::CycGuard::enter(exec_span.hot(), super::timeline::CYC_PUBLISH);
            let _b = super::buckets::Guard::start(super::buckets::PUBLISH);
            for (location, rmw, lazy_credit) in published {
                // Read-then-write waits for the previous RMW writer. A lazy
                // credit stays a delta and is not an admission edge.
                self.live
                    .publish_write(tx_idx, incarnation, location, rmw, lazy_credit, |t| {
                        self.rt.still_open(t)
                    });
            }
        }

        let receipt = receipt_from_revm(exec_result);
        let state = state_transitions_from_revm(self.is_eip_161_enabled, state);
        if let Some(slot) = result_slot {
            slot.receipt = receipt;
            slot.state.clear();
            slot.state.extend(state);
        } else {
            *result_slot = Some(PevmTxExecutionResult {
                receipt,
                state: state.collect(),
            });
        }
        exec_span.done = true;
        Step::Done
    }
}

fn step_from_read(err: ReadError) -> Step {
    match VmExecutionError::from(err) {
        VmExecutionError::Retry => Step::Retry,
        VmExecutionError::FallbackToSequential => Step::Fallback,
        VmExecutionError::Blocking(tx) => Step::Block(tx),
        VmExecutionError::ExecutionError(err) => Step::Fatal(err),
    }
}

struct NoBeneficiaryHandler<C, DB> {
    _phantom: core::marker::PhantomData<(C, DB)>,
}

impl<C, DB> Default for NoBeneficiaryHandler<C, DB> {
    fn default() -> Self {
        Self {
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<C: PevmChain, DB: Database> Handler for NoBeneficiaryHandler<C, DB> {
    type Evm = C::Evm<DB>;
    type Error = EVMError<DB::Error, InvalidTransaction>;
    type HaltReason = C::EvmHaltReason;

    fn reward_beneficiary(
        &self,
        _: &mut Self::Evm,
        _: &mut FrameResult,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::writer_final;
    use crate::specfence::live_chain::{ClassKeyKind, LiveChain};
    use crate::specfence::rt::Runtime;

    #[test]
    fn armed_reader_accepts_a_final_write_before_commit() {
        let n = 2;
        let location = 10u64;
        let mut live = LiveChain::new(n, ClassKeyKind::ToSelector);
        live.install_classes(Vec::new(), vec![u16::MAX; n], vec![location, location]);
        live.preseed_recipients();
        live.publish_write(0, 0, location, false, false, |_| true);
        let rt = Runtime::new(n, 2);
        rt.seed();
        assert_eq!(rt.pop(0).unwrap().0, 0);
        rt.finish_ok(0, 0);
        assert!(!writer_final(&live, &rt, location, 0));
        assert!(rt.mark_validated(0, 0));
        assert!(writer_final(&live, &rt, location, 0));
        assert_eq!(rt.committed(), 0);
    }
}
