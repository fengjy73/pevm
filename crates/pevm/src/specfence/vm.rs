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
    retained_storage,
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
}

impl<'a, S: crate::Storage> VmDb<'a, S> {
    /// Armed locations settle only on the committed write. A `Published`
    /// incarnation can still fail validation and store a different value.
    /// Unarmed locations keep the publish mark; nothing has joined the chain
    /// that a reader was required to wait for.
    fn settled(&self, location: u64, pred: TxIdx) -> bool {
        if !self.chain.writer_published(location, pred) {
            return false;
        }
        !self.chain.is_armed(location) || self.rt.committed() > pred
    }

    fn finished_without_write(&self, location: u64, pred: TxIdx) -> bool {
        let (state, _) = self.chain.writer_state(location, pred);
        if state == 0 || state == 3 {
            return false;
        }
        // A finished-but-uncommitted attempt can still abort and write.
        self.rt.committed() > pred
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
        let _b = super::buckets::Guard::start(super::buckets::COORD);
        if self.trace.enabled() && self.chain.is_armed(location) {
            self.trace.note_read_after_arm();
        }
        let result = self.wait_writer(location);
        let accepted = self
            .chain
            .nearest_lower(location, self.tx_idx)
            .map(|tx| tx as u32)
            .unwrap_or(u32::MAX);
        self.accepted_pred.set(accepted);
        if self.trace.diag() {
            let (nearest, state, reason) = match result {
                Ok(()) => {
                    match self.chain.nearest_lower(location, self.tx_idx) {
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
                    }
                }
                Err(_) => {
                    let pred = self.chain.nearest_lower(location, self.tx_idx);
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
        result
    }

    /// Armed locations always wait. Unarmed locations still consult the cost model.
    fn wait_writer(&self, location: u64) -> Result<(), ReadError> {
        if !self.chain.any() {
            return Ok(());
        }
        for _ in 0..8 {
            let Some(pred) = self.chain.nearest_lower(location, self.tx_idx) else {
                return Ok(());
            };
            if self.settled(location, pred) {
                return Ok(());
            }
            if self.finished_without_write(location, pred) {
                self.chain.clear_hole(location, pred);
                continue;
            }
            let executing = self.rt.is_executing(pred);
            let armed = self.chain.is_armed(location);
            if !armed && !self.chain.should_wait(location, pred, executing) {
                return Ok(());
            }
            if !executing {
                return Err(ReadError::Blocking(pred));
            }
            // Overlap only while the predecessor is inside the interpreter.
            // 40 × 50µs is the cap; a longer spin holds the worker off the prefix.
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
            if self.settled(location, pred) {
                return Ok(());
            }
            if self.finished_without_write(location, pred) {
                self.chain.clear_hole(location, pred);
                continue;
            }
            return Err(ReadError::Blocking(pred));
        }
        match self.chain.nearest_lower(location, self.tx_idx) {
            Some(pred) if !self.settled(location, pred) => Err(ReadError::Blocking(pred)),
            _ => Ok(()),
        }
    }

    /// Nonce and balance checks block on the previous same-sender transaction.
    /// Blocking on `tx-1` retries forever once that unrelated transaction has committed.
    fn sender_block(&self) -> ReadError {
        match self.prev_sender.get(self.tx_idx).copied().flatten() {
            Some(pred) if !self.rt.is_committed(pred) && !self.rt.is_executed(pred) => {
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
        let location_hash = hash_deterministic(MemoryLocation::CodeHash(address));
        let serial = self.serial;
        self.coordinate(location_hash)?;
        let read_origins = self.read_set.entry(location_hash).or_default();
        if let Some(written_transactions) = self.mv.data.get(&location_hash)
            && let Some((tx_idx, entry)) = written_transactions.range(..self.tx_idx).next_back()
        {
            match entry {
                MemoryEntry::Estimate => return Err(ReadError::Blocking(*tx_idx)),
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
        .nearest_lower(location, tx_idx)
        .map(|tx| tx as u32)
        .unwrap_or(u32::MAX);
    if now != accepted {
        if now != u32::MAX {
            let pred = now as TxIdx;
            if chain.is_armed(location) && !writer_final(chain, rt, location, pred) {
                return Err(ReadError::Blocking(pred));
            }
        }
        return Err(ReadError::InconsistentRead);
    }
    if accepted != u32::MAX {
        let pred = accepted as TxIdx;
        if chain.is_armed(location) && !writer_final(chain, rt, location, pred) {
            return Err(ReadError::Blocking(pred));
        }
    }
    Ok(())
}

/// The nearest chain writer has either committed its published value or
/// committed without writing this location (a hole the caller may skip).
fn writer_final(chain: &LiveChain, rt: &Runtime, location: u64, pred: TxIdx) -> bool {
    if rt.committed() <= pred {
        return false;
    }
    let (state, _) = chain.writer_state(location, pred);
    chain.writer_published(location, pred) || (state != 0 && state != 3)
}

fn mv_origin(serial: bool, tx_idx: TxIdx, incarnation: usize, value: &MemoryValue) -> SfReadOrigin {
    if serial {
        SfReadOrigin::MvId { tx_idx, incarnation }
    } else {
        SfReadOrigin::Mv(SfOrigin {
            tx_idx,
            incarnation,
            value: value.clone(),
        })
    }
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

        self.coordinate(location_hash)?;

        let read_origins = self.read_set.entry(location_hash).or_default();
        let has_prev_origins = !read_origins.is_empty();
        let mut new_origins = SmallVec::new();
        let mut final_account = None;
        let mut balance_addition = U256::ZERO;
        let mut positive_addition = true;
        let mut nonce_addition = 0u64;

        if self.tx_idx > 0
            && let Some(written_transactions) = self.mv.data.get(&location_hash)
        {
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
                        return Err(ReadError::Blocking(*blocking_idx));
                    }
                    Some((closest_idx, MemoryEntry::Data(tx_incarnation, value))) => {
                        if has_prev_origins && read_origins.len() == new_origins.len() {
                            return Err(ReadError::InconsistentRead);
                        }
                        let origin = mv_origin(serial, *closest_idx, *tx_incarnation, value);
                        if has_prev_origins && !origin_eq(&read_origins[new_origins.len()], &origin)
                        {
                            return Err(ReadError::InconsistentRead);
                        }
                        new_origins.push(origin);
                        match value {
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

        if final_account.is_none() {
            if !has_prev_origins {
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
        if !has_prev_origins {
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
            let code_hash = if Some(location_hash) == self.to_hash {
                self.to_code_hash
            } else {
                self.get_code_hash(address)?
            };
            let code = code_for(self, code_hash)?;
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
        Ok(None)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        match self
            .storage
            .code_by_hash(&code_hash)
            .map_err(|err| ReadError::StorageError(err.to_string()))?
        {
            Some(evm_code) => Ok(Bytecode::from(evm_code)),
            None => Ok(Bytecode::default()),
        }
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let location_hash = hash_deterministic(MemoryLocation::Storage(address, index));
        let serial = self.serial;
        self.coordinate(location_hash)?;
        let read_origins = self.read_set.entry(location_hash).or_default();
        if self.tx_idx > 0
            && let Some(written_transactions) = self.mv.data.get(&location_hash)
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

fn code_for<S: crate::Storage>(
    db: &VmDb<'_, S>,
    code_hash: Option<B256>,
) -> Result<Option<Bytecode>, ReadError> {
    if let Some(code_hash) = &code_hash {
        if let Some(code) = db.mv.new_bytecodes.get(code_hash) {
            return Ok(Some(code.clone()));
        }
        match db.storage.code_by_hash(code_hash) {
            Ok(code) => Ok(code.map(Bytecode::from)),
            Err(err) => Err(ReadError::StorageError(err.to_string())),
        }
    } else {
        Ok(None)
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
            serial: rt.worker_count() == 1,
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
        let clock = self.trace.timing().then(Instant::now);
        let _pre = super::buckets::Guard::start(super::buckets::PRE);
        self.trace.note_exec(incarnation);
        if !self.evm.ctx().db().serial {
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
                        && !self.rt.is_executed(pred)
                    {
                        return Step::Block(pred);
                    }
                    return Step::Yield;
                }
                return Step::Fatal(err);
            }
        }
        };

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
        let published: Vec<(u64, bool)> = if self.evm.ctx().db().serial {
            Vec::new()
        } else {
            let read_keys: Vec<u64> = read_set.keys().copied().collect();
            write_set
                .iter()
                .map(|(location, value)| {
                    let rmw = read_keys.contains(location) && !is_lazy_value(value);
                    (*location, rmw)
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
        {
            let _b = super::buckets::Guard::start(super::buckets::RECORD);
            self.mv.record(&tx_version, read_set, write_set);
        }
        {
            let _b = super::buckets::Guard::start(super::buckets::PUBLISH);
            for (location, rmw) in published {
                // Read-then-write waits for the previous writer. Lazy values are
                // blind: they join the chain for readers, and writers do not wait.
                self.live.publish_write(tx_idx, incarnation, location, rmw, |t| {
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
