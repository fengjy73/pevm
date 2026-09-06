use alloy_primitives::{Address, B256, TxKind, U256};
use alloy_rpc_types_eth::Receipt;
use hashbrown::HashMap;
use revm::{
    Database,
    context::{
        BlockEnv, ContextSetters, ContextTr, DBErrorMarker, JournalTr, TxEnv,
        result::{EVMError, ExecutionResult, InvalidTransaction},
    },
    handler::EvmTr,
    primitives::KECCAK_EMPTY,
    state::{AccountInfo, Bytecode, EvmState},
};
use smallvec::SmallVec;

use crate::{
    AccountBasic, BuildIdentityHasher, BuildSuffixHasher, EvmAccount, FinishExecFlags, MemoryEntry,
    MemoryLocation, MemoryLocationHash, MemoryValue, ReadOrigin, ReadOrigins, ReadSet, Storage,
    TxIdx, TxVersion, WriteSet, chain::PevmChain, hash_deterministic, mv_memory::MvMemory,
    specfence::{
        AccessMode, CheckpointKind, FfValue, ResolveAction, SpecFenceCtx, StorageWriteReplay,
        TAU_VERY_HIGH, early_val_probability, note_pending_effect_boundary,
        arm_call_outcome_cache, resume_was_applied, steps_this_run, try_arm_safe_absolute_jump, with_plant_tls,
    },
};

/// The execution error from the underlying EVM executor.
// Will there be DB errors outside of read?
pub type ExecutionError = EVMError<ReadError>;

/// Represents the state transitions of the EVM accounts after execution.
/// If the value is [None], it indicates that the account is marked for removal.
/// If the value is [`Some(new_state)`], it indicates that the account has become [`new_state`].
type EvmStateTransitions = HashMap<Address, Option<EvmAccount>, BuildSuffixHasher>;

/// Execution result of a transaction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PevmTxExecutionResult {
    /// Receipt of execution
    // TODO: Consider promoting to [ReceiptEnvelope] if there is high demand
    pub receipt: Receipt,
    /// State that got updated
    pub state: EvmStateTransitions,
}

/// Convert Revm's execution result into a standard receipt.
/// Note that the cumulative gas used in the receipt is preset to the gas used in this transaction.
/// It should be post-processed with the remaining transactions in the block.
pub(crate) fn receipt_from_revm<H>(result: ExecutionResult<H>) -> Receipt {
    Receipt {
        status: result.is_success().into(),
        cumulative_gas_used: result.tx_gas_used(),
        logs: result.into_logs(),
    }
}

/// Convert Revm's state transitions into PEVM's state transitions.
pub(crate) fn state_transitions_from_revm(
    is_eip_161_enabled: bool,
    state: EvmState,
) -> impl Iterator<Item = (Address, Option<EvmAccount>)> {
    state
        .into_iter()
        .filter(|(_, account)| account.is_touched())
        .map(move |(address, account)| {
            if account.is_selfdestructed() || account.is_empty() && is_eip_161_enabled {
                (address, None)
            } else {
                (address, Some(EvmAccount::from(account)))
            }
        })
}

pub(crate) enum VmExecutionError {
    Retry,
    FallbackToSequential,
    Blocking(TxIdx),
    ExecutionError(ExecutionError),
}

/// Errors when reading a memory location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// Cannot read memory location from storage.
    // TODO: More concrete type
    #[error("Failed reading memory from storage: {0}")]
    StorageError(String),
    /// This memory location has been written by a lower transaction.
    #[error("Read of memory location is blocked by tx #{0}")]
    Blocking(TxIdx),
    /// There has been an inconsistent read like reading the same
    /// location from storage in the first call but from [`VmMemory`] in
    /// the next.
    #[error("Inconsistent read")]
    InconsistentRead,
    /// Found an invalid nonce, like the first transaction of a sender
    /// not having a (+1) nonce from storage.
    #[error("Tx #{0} has invalid nonce")]
    InvalidNonce(TxIdx),
    /// Read a self-destructed account that is very hard to handle, as
    /// there is no performant way to mark all storage slots as cleared.
    #[error("Tried to read self-destructed account")]
    SelfDestructedAccount,
    /// The stored memory value type doesn't match its location type.
    // TODO: Handle this at the type level?
    #[error("Invalid type of stored memory value")]
    InvalidMemoryValueType,
}

impl DBErrorMarker for ReadError {}

impl From<ReadError> for VmExecutionError {
    fn from(err: ReadError) -> Self {
        match err {
            ReadError::InconsistentRead => Self::Retry,
            ReadError::SelfDestructedAccount => Self::FallbackToSequential,
            ReadError::Blocking(tx_idx) => Self::Blocking(tx_idx),
            _ => Self::ExecutionError(EVMError::Database(err)),
        }
    }
}

// A database interface that intercepts reads while executing a specific
// transaction with Revm. It provides values from the multi-version data
// structure & storage, and tracks the read set of the current execution.
pub(crate) struct VmDb<'a, S: Storage> {
    storage: &'a S,
    mv_memory: &'a MvMemory,
    specfence: SpecFenceCtx<'a>,
    tx_idx: TxIdx,
    tx: &'a TxEnv,
    from_hash: MemoryLocationHash,
    to_hash: Option<MemoryLocationHash>,
    to_code_hash: Option<B256>,
    // Indicates if we lazy update this transaction.
    // Only applied to raw transfers' senders & recipients at the moment.
    is_lazy: bool,
    // Whether to enforce the sender-nonce ordering check for this transaction.
    // False for transaction types with no nonce (e.g. OP deposits).
    has_nonce: bool,
    read_set: ReadSet,
    // TODO: Clearer type for [AccountBasic] plus code hash
    read_accounts: HashMap<MemoryLocationHash, (AccountBasic, Option<B256>), BuildIdentityHasher>,
}

impl<'a, S: Storage> VmDb<'a, S> {
    // Reset per-transaction fields for allocation reuse.
    // Must be called before each transaction execution.
    fn set_tx(
        &mut self,
        tx_idx: TxIdx,
        tx: &'a TxEnv,
        from_hash: MemoryLocationHash,
        to_hash: Option<MemoryLocationHash>,
        has_nonce: bool,
        incarnation: crate::TxIncarnation,
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
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            self.specfence
                .partial_retry
                .reset_incarnation(tx_idx, incarnation);
            // M1b: restore SpecFence journal to checkpoint via FF continuation.
            let n = self.specfence.partial_retry.replay_ff_if_armed(tx_idx);
            let entries = self
                .specfence
                .partial_retry
                .ff_entries(tx_idx)
                .max(n);
            if entries > 0 {
                self.specfence.metrics.record_journal_ff_entries(entries);
            }
        }
        if let TxKind::Call(to) = tx.kind {
            self.to_code_hash = self.get_code_hash(to)?;

            // We only lazy update raw transfers that already have the sender
            // or recipient in [MvMemory] since sequentially evaluating memory
            // locations with only one entry is much costlier than fully
            // evaluating it concurrently.
            // TODO: Only lazy update in block syncing mode, not for block
            // building.
            self.is_lazy = self.to_code_hash.is_none()
                && (self.mv_memory.data.contains_key(&from_hash)
                    || self.mv_memory.data.contains_key(&to_hash.unwrap()));
        }
        Ok(())
    }


    /// M1b: try serving a certified-prefix read from the FF value cache.
    /// Returns Some when origin is unchanged — caller skips MV lazy walk.
    fn try_ff_storage(
        &self,
        location_hash: MemoryLocationHash,
    ) -> Option<(U256, ReadOrigin)> {
        if !self
            .specfence
            .partial_retry
            .is_rewind_resume(self.tx_idx)
        {
            return None;
        }
        let FfValue::Storage { value, origin, .. } =
            self.specfence.partial_retry.ff_value(self.tx_idx, location_hash)?
        else {
            return None;
        };
        let current = self
            .mv_memory
            .last_data_before(location_hash, self.tx_idx)
            .map(|(tx_idx, tx_incarnation)| (tx_idx, tx_incarnation));
        if current != origin {
            return None;
        }
        let read_origin = match origin {
            Some((tx_idx, tx_incarnation)) => ReadOrigin::MvMemory(TxVersion {
                tx_idx,
                tx_incarnation,
            }),
            None => ReadOrigin::Storage,
        };
        Some((value, read_origin))
    }

    fn try_ff_basic(
        &self,
        location_hash: MemoryLocationHash,
    ) -> Option<(AccountBasic, Option<B256>, ReadOrigin)> {
        if !self
            .specfence
            .partial_retry
            .is_rewind_resume(self.tx_idx)
        {
            return None;
        }
        let FfValue::Basic {
            basic,
            code_hash,
            origin,
            ..
        } = self.specfence.partial_retry.ff_value(self.tx_idx, location_hash)?
        else {
            return None;
        };
        // Only single-origin basics are cached; require matching top writer.
        let current = self
            .mv_memory
            .last_data_before(location_hash, self.tx_idx)
            .map(|(tx_idx, tx_incarnation)| (tx_idx, tx_incarnation));
        if current != origin {
            return None;
        }
        let read_origin = match origin {
            Some((tx_idx, tx_incarnation)) => ReadOrigin::MvMemory(TxVersion {
                tx_idx,
                tx_incarnation,
            }),
            None => ReadOrigin::Storage,
        };
        Some((basic, code_hash, read_origin))
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

    fn promote_on_conflict(&self, address: Address, location: MemoryLocationHash) {
        if !self.specfence.mode.uses_regions() || address == self.specfence.beneficiary {
            return;
        }
        // Intra-block Wait; at most one Bayes conflict obs per location per block
        // (validation aborts still call observe_conflict_location_always).
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            if self.specfence.bayes.observe_conflict_location(location) {
                self.specfence.metrics.record_bayes_conflict();
                if address != self.specfence.beneficiary {
                    self.specfence.bayes.observe_conflict_account_n(address, 3);
                }
            }
            self.specfence
                .promote_from_bayes(&self.mv_memory.regions, location, Some(address));
            return;
        }
        if self.mv_memory.regions.promote_location(location) {
            self.specfence.metrics.record_promotion(Some(address));
        }
        self.mv_memory.regions.promote_account(address);
        self.specfence.metrics.mark_hot(address);
    }

    /// SpecFence π at location granularity: WaitHard / Bind / SpecRead.
    /// PCC keeps sticky Wait. Beneficiary never waits.
    /// P2: force Bind/WaitHard on certified-prefix locations after PartialRetry.
    fn maybe_wait(
        &self,
        address: Address,
        location_hash: MemoryLocationHash,
    ) -> Result<(), ReadError> {
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && self.specfence.hotset.contains(location_hash)
        {
            let _ = self.specfence.rem.note_effect();
            self.specfence.partial_retry.note_access(
                self.tx_idx,
                location_hash,
                AccessMode::Read,
            );
        }

        if self.specfence.mode != crate::ConcurrencyMode::SpecFence {
            if !self
                .specfence
                .should_wait_location(&self.mv_memory.regions, location_hash, &address)
            {
                return Ok(());
            }
            if let Some(prev) = self
                .mv_memory
                .last_writer_before(location_hash, self.tx_idx)
                && !self.specfence.scheduler.is_done(prev)
            {
                self.specfence.metrics.record_wait(address);
                return Err(ReadError::Blocking(prev));
            }
            if let Some(prev) =
                self.specfence
                    .wait_blocker(&self.mv_memory.regions, &address, self.tx_idx)
            {
                self.specfence.metrics.record_wait(address);
                return Err(ReadError::Blocking(prev));
            }
            return Ok(());
        }

        // --- SpecFence path ---
        if address == self.specfence.beneficiary {
            self.specfence.metrics.record_spec_read();
            return Ok(());
        }
        // R1 LeanOCC: ℓ ∉ HotSet → OCC-style SpecRead only (no Bayes WaitHard).
        if !self.specfence.hotset.contains(location_hash) {
            self.specfence.metrics.record_spec_read();
            return Ok(());
        }
        // R2 HotLocal: Bind / WaitHard+park / SpecRead for ℓ ∈ HotSet.
        self.specfence.hotset.record_hot_local_read();

        let residual_predicts = self
            .mv_memory
            .residual_writer_before(location_hash, self.tx_idx)
            .is_some();
        // M3: process-local WŜ prior (inter-block) + residual (intra-block / reincarnation).
        let process_prior = self.specfence.rw_prior.predicts_write(location_hash);
        let prior_ws_predicts = residual_predicts || process_prior;
        // Concrete writer: MV published/ESTIMATE entry, else residual WŜ(t) of a lower tx.
        // Do NOT fall back to AccountHints.prev here — that can WaitHard on a tx that
        // never writes ℓ (ERC-20 address vs slot) and inflate park chains under M3.
        let writer = self
            .mv_memory
            .last_writer_before(location_hash, self.tx_idx)
            .or_else(|| self.mv_memory.residual_writer_before(location_hash, self.tx_idx));
        let writer_done = writer.is_some_and(|w| self.specfence.scheduler.is_done(w));
        let bind_version = self
            .mv_memory
            .last_data_before(location_hash, self.tx_idx)
            .map(|(tx_idx, tx_incarnation)| TxVersion {
                tx_idx,
                tx_incarnation,
            });
        if prior_ws_predicts && writer.is_some() {
            self.specfence.rw_prior.observe_co_access(location_hash);
        }

        // P2: certified-prefix from prior PartialRetry → force Bind/WaitHard.
        let force_prefix = self
            .specfence
            .partial_retry
            .must_force_bind(self.tx_idx, location_hash);

        // Revoke sticky Wait when posterior < τ_revoke (side effects only).
        // v6: sticky no longer forces WaitHard — cost-aware π decides.
        let _ = self.specfence.try_revoke(
            &self.mv_memory.regions,
            location_hash,
            Some(&address),
        );

        let posterior = self
            .specfence
            .bayes
            .conflict_probability(location_hash, Some(&address));

        let action = if force_prefix {
            if let Some(v) = bind_version.clone() {
                ResolveAction::Bind(v)
            } else {
                ResolveAction::WaitHard
            }
        } else {
            let mut a = self.specfence.choose_resolve(
                location_hash,
                &address,
                writer,
                writer_done,
                bind_version.clone(),
                residual_predicts,
                prior_ws_predicts,
            );
            // Safety valve only: escalate SpecRead when P is very high.
            // (Old EarlyVal@0.35 WaitHard bias removed — cost model owns π.)
            // M3: also escalate when process/residual WŜ predicts a writer.
            // M3: if WŜ prior + published Data → Bind before SpecRead.
            // Keep WaitHard escalate at very-high P only (pre-M3 safety valve).
            // Residual unfinished writers already flow through cost π / writer_known.
            if matches!(a, ResolveAction::SpecRead) {
                if let Some(v) = bind_version.clone() {
                    if prior_ws_predicts || posterior >= TAU_VERY_HIGH {
                        a = ResolveAction::Bind(v);
                    }
                } else if posterior >= TAU_VERY_HIGH && writer.is_some() {
                    a = ResolveAction::WaitHard;
                }
            }
            a
        };

        match action {
            ResolveAction::WaitHard => {
                self.specfence.metrics.record_wait_hard();
                if let Some(prev) = writer
                    && !self.specfence.scheduler.is_done(prev)
                {
                    self.specfence.metrics.record_wait(address);
                    self.specfence.dag.note_soft_wait(location_hash, self.tx_idx);
                    self.specfence
                        .wave
                        .set_pending_park_location(location_hash);
                    return Err(ReadError::Blocking(prev));
                }
                // Cold-start: skip WaitHard when posterior is cold.
                if posterior < crate::specfence::TAU_REVOKE {
                    let _ = self.specfence.try_revoke(
                        &self.mv_memory.regions,
                        location_hash,
                        Some(&address),
                    );
                    return Ok(());
                }
                if !self.specfence.bayes.has_location(location_hash)
                    && let Some(prev) = self.specfence.hints.prev(&address, self.tx_idx)
                    && !self.specfence.scheduler.is_done(prev)
                {
                    self.specfence.metrics.record_wait(address);
                    self.specfence
                        .wave
                        .set_pending_park_location(location_hash);
                    return Err(ReadError::Blocking(prev));
                }
                Ok(())
            }
            ResolveAction::Bind(v) => {
                self.specfence.metrics.record_bind_hit();
                self.specfence.bayes.observe_bind_hit(location_hash);
                // M3: Bind-before-touch credit when WŜ prior (re-check process map —
                // may have been learned mid-block) or residual / force-prefix.
                let prior_now = prior_ws_predicts
                    || force_prefix
                    || self.specfence.rw_prior.predicts_write(location_hash)
                    || residual_predicts;
                if prior_now {
                    self.specfence.metrics.record_prior_bind_hit();
                }
                if !self.specfence.scheduler.is_done(v.tx_idx) {
                    self.specfence.metrics.record_wait_hard();
                    self.specfence.metrics.record_wait(address);
                    self.specfence
                        .wave
                        .set_pending_park_location(location_hash);
                    return Err(ReadError::Blocking(v.tx_idx));
                }
                self.specfence.dag.note_hard_edge();
                // Bind success → certify for PartialRetry prefix + effect cp.
                // M1c: defer checkpoint to Inspector step_end so PC/stack are post-opcode.
                self.specfence
                    .partial_retry
                    .note_certified(self.tx_idx, location_hash);
                self.specfence.rem.note_checkpoint_opportunity();
                // M1d: defer to Inspector step_end for live PC/stack snap when
                // inspect_run is driving; else lite effect-ordinal snap.
                note_pending_effect_boundary(self.tx_idx, self.specfence.partial_retry);
                Ok(())
            }
            ResolveAction::SpecRead => {
                self.specfence.metrics.record_spec_read();
                // M3: SpecRead despite WŜ prior — bayes bind-useful miss signal (validate
                // may still succeed; hard prior_bind_miss counted on validate fail).
                if prior_ws_predicts {
                    self.specfence.bayes.observe_bind_miss(location_hash);
                }
                // M1f/M1g: EffectBoundary so RewindTo can leave CallEntry with a live
                // jump_snap. Absolute jump gated by jump_is_safe (Storage reads OK;
                // no journal-blob restore; nested CALL via CallOutcome cache).
                note_pending_effect_boundary(self.tx_idx, self.specfence.partial_retry);
                Ok(())
            }
        }
    }

    /// P2 EarlyVal after a SpecRead origin is recorded: certify or abort early.
    fn maybe_early_val(
        &mut self,
        address: Address,
        location_hash: MemoryLocationHash,
    ) -> Result<(), ReadError> {
        if self.specfence.mode != crate::ConcurrencyMode::SpecFence {
            return Ok(());
        }
        // R0/R1: EarlyVal only on HotSet under research inspect (default path: off).
        if self.specfence.engagement.is_lean() || !self.specfence.hotset.contains(location_hash) {
            return Ok(());
        }
        if address == self.specfence.beneficiary {
            return Ok(());
        }
        let posterior = self
            .specfence
            .bayes
            .conflict_probability(location_hash, Some(&address));
        // Only EarlyVal when cheap/pressure: high P_conflict or hot SpecRead.
        if early_val_probability(posterior) < 0.35
            && self.mv_memory.regions.location_mode(location_hash)
                != crate::specfence::RegionMode::Wait
        {
            return Ok(());
        }
        let origins = match self.read_set.get(&location_hash) {
            Some(o) => o.clone(),
            None => return Ok(()),
        };
        if self
            .mv_memory
            .origins_still_valid(self.tx_idx, location_hash, &origins)
        {
            self.specfence
                .partial_retry
                .note_certified(self.tx_idx, location_hash);
            self.specfence.rem.note_checkpoint_opportunity();
            self.specfence.metrics.record_checkpoint_opportunity();
            // M1d: live Inspector snap via step_end when plant TLS active.
            note_pending_effect_boundary(self.tx_idx, self.specfence.partial_retry);
            Ok(())
        } else {
            // EarlyVal fail → enter PartialRetry path (re-exec with force-bind).
            let mut certified = self
                .specfence
                .partial_retry
                .force_bind_locations(self.tx_idx);
            for loc in self.read_set.keys() {
                if *loc != location_hash
                    && !certified.contains(loc)
                    && self.mv_memory.origins_still_valid(
                        self.tx_idx,
                        *loc,
                        self.read_set.get(loc).unwrap(),
                    )
                {
                    certified.push(*loc);
                }
            }
            // M1/M1b: demote head PartialRetry → RewindTo + journal FF when a cp exists.
            let k_fail = self
                .specfence
                .partial_retry
                .first_k(self.tx_idx, location_hash)
                .unwrap_or_else(|| self.specfence.partial_retry.current_k(self.tx_idx));
            let cp = self
                .specfence
                .partial_retry
                .last_checkpoint_before(self.tx_idx, k_fail)
                .unwrap_or(crate::specfence::CheckpointId {
                    tx_idx: self.tx_idx,
                    incarnation: 0,
                    k: 0,
                });
            self.specfence.partial_retry.arm_rewind_to(
                self.tx_idx,
                cp,
                k_fail,
                certified.clone(),
                Vec::new(),
                Vec::new(),
            );
            self.specfence
                .partial_retry
                .set_force_bind(self.tx_idx, certified);
            self.specfence.metrics.record_partial_retry();
            self.specfence.metrics.record_rewind_to_cp();
            self.specfence.metrics.record_region_validate_fail(1);
            Err(ReadError::InconsistentRead)
        }
    }

    // Push a new read origin. Return an error when there's already
    // an origin but doesn't match the new one to force re-execution.
    fn push_origin(read_origins: &mut ReadOrigins, origin: ReadOrigin) -> Result<(), ReadError> {
        if let Some(prev_origin) = read_origins.last() {
            if prev_origin != &origin {
                return Err(ReadError::InconsistentRead);
            }
        } else {
            read_origins.push(origin);
        }
        Ok(())
    }

    fn get_code_hash(&mut self, address: Address) -> Result<Option<B256>, ReadError> {
        let location_hash = hash_deterministic(MemoryLocation::CodeHash(address));
        let read_origins = self.read_set.entry(location_hash).or_default();

        // Try to read the latest code hash in [MvMemory]
        // TODO: Memoize read locations (expected to be small) here in [Vm] to avoid
        // contention in [MvMemory]
        if let Some(written_transactions) = self.mv_memory.data.get(&location_hash)
            && let Some((tx_idx, MemoryEntry::Data(tx_incarnation, value))) =
                written_transactions.range(..self.tx_idx).next_back()
        {
            if self
                .mv_memory
                .is_aborted_incarnation(*tx_idx, *tx_incarnation)
            {
                return Err(ReadError::Blocking(*tx_idx));
            }
            match value {
                MemoryValue::SelfDestructed => {
                    return Err(ReadError::SelfDestructedAccount);
                }
                MemoryValue::CodeHash(code_hash) => {
                    Self::push_origin(
                        read_origins,
                        ReadOrigin::MvMemory(TxVersion {
                            tx_idx: *tx_idx,
                            tx_incarnation: *tx_incarnation,
                        }),
                    )?;
                    return Ok(Some(*code_hash));
                }
                _ => {}
            }
        };

        // Fallback to storage
        Self::push_origin(read_origins, ReadOrigin::Storage)?;
        self.storage
            .code_hash(&address)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }
}

impl<S: Storage> Database for VmDb<'_, S> {
    type Error = ReadError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let location_hash = self.hash_basic(&address);
        self.maybe_wait(address, location_hash)?;

        // We return a mock for non-contract addresses (for lazy updates) to avoid
        // unnecessarily evaluating its balance here.
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

        // M1b: single-origin basic FF (no lazy chain) — skip MV walk when stable.
        if !self.is_lazy
            && let Some((account, code_hash, origin)) = self.try_ff_basic(location_hash)
        {
            let read_origins = self.read_set.entry(location_hash).or_default();
            Self::push_origin(read_origins, origin)?;
            self.specfence.metrics.record_journal_ff_hit();
            self.read_accounts
                .insert(location_hash, (account.clone(), code_hash));
            let code = if let Some(code_hash) = &code_hash {
                if let Some(code) = self.mv_memory.new_bytecodes.get(code_hash) {
                    Some(code.clone())
                } else {
                    match self.storage.code_by_hash(code_hash) {
                        Ok(code) => code.map(Bytecode::from),
                        Err(err) => return Err(ReadError::StorageError(err.to_string())),
                    }
                }
            } else {
                None
            };
            return Ok(Some(AccountInfo {
                balance: account.balance,
                nonce: account.nonce,
                code_hash: code_hash.unwrap_or(KECCAK_EMPTY),
                code,
                account_id: None,
            }));
        }

        let read_origins = self.read_set.entry(location_hash).or_default();
        let has_prev_origins = !read_origins.is_empty();
        // We accumulate new origins to either:
        // - match with the previous origins to check consistency
        // - register origins on the first read
        let mut new_origins = SmallVec::new();

        let mut final_account = None;
        let mut balance_addition = U256::ZERO;
        // The sign of [balance_addition] since it can be negative for lazy senders.
        let mut positive_addition = true;
        let mut nonce_addition = 0;

        // Try reading from multi-version data
        if self.tx_idx > 0
            && let Some(written_transactions) = self.mv_memory.data.get(&location_hash)
        {
            let mut iter = written_transactions.range(..self.tx_idx);

            // Fully evaluate lazy updates
            loop {
                match iter.next_back() {
                    Some((blocking_idx, MemoryEntry::Estimate)) => {
                        self.promote_on_conflict(address, location_hash);
                        return Err(ReadError::Blocking(*blocking_idx));
                    }
                    Some((closest_idx, MemoryEntry::Data(tx_incarnation, value))) => {
                        if self
                            .mv_memory
                            .is_aborted_incarnation(*closest_idx, *tx_incarnation)
                        {
                            self.promote_on_conflict(address, location_hash);
                            return Err(ReadError::Blocking(*closest_idx));
                        }
                        self.specfence.metrics.record_db_heavy_op();
                        // About to push a new origin
                        // Inconsistent: new origin will be longer than the previous!
                        if has_prev_origins && read_origins.len() == new_origins.len() {
                            return Err(ReadError::InconsistentRead);
                        }
                        let origin = ReadOrigin::MvMemory(TxVersion {
                            tx_idx: *closest_idx,
                            tx_incarnation: *tx_incarnation,
                        });
                        // Inconsistent: new origin is different from the previous!
                        if has_prev_origins
                            && unsafe { read_origins.get_unchecked(new_origins.len()) } != &origin
                        {
                            return Err(ReadError::InconsistentRead);
                        }
                        new_origins.push(origin);
                        match value {
                            MemoryValue::Basic(basic) => {
                                // TODO: Return [SelfDestructedAccount] if [basic] is
                                // [SelfDestructed]?
                                // For now we are betting on [code_hash] triggering the
                                // sequential fallback when we read a self-destructed contract.
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
                    None => {
                        break;
                    }
                }
            }
        }

        // Fall back to storage
        if final_account.is_none() {
            self.specfence.metrics.record_db_heavy_op();
            // Populate [Storage] on the first read
            if !has_prev_origins {
                new_origins.push(ReadOrigin::Storage);
            }
            // Inconsistent: previous origin is longer or didn't read
            // from storage for the last origin.
            else if read_origins.len() != new_origins.len() + 1
                || read_origins.last() != Some(&ReadOrigin::Storage)
            {
                return Err(ReadError::InconsistentRead);
            }
            final_account = match self.storage.basic(&address) {
                Ok(Some(basic)) => Some(basic),
                Ok(None) => (balance_addition > U256::ZERO).then(AccountBasic::default),
                Err(err) => return Err(ReadError::StorageError(err.to_string())),
            };
        }

        // Populate read origins on the first read.
        // Otherwise [read_origins] matches [new_origins] already.
        if !has_prev_origins {
            *read_origins = new_origins;
        }

        if let Some(mut account) = final_account {
            // Check sender nonce
            account.nonce += nonce_addition;
            if self.has_nonce && location_hash == self.from_hash && self.tx.nonce != account.nonce {
                return if self.tx_idx > 0 {
                    // TODO: Better retry strategy -- immediately, to the
                    // closest sender tx, to the missing sender tx, etc.
                    self.promote_on_conflict(address, location_hash);
                    Err(ReadError::Blocking(self.tx_idx - 1))
                } else {
                    Err(ReadError::InvalidNonce(self.tx_idx))
                };
            }

            // Fully evaluate the account and register it to read cache
            // to later check if they have changed (been written to).
            if positive_addition {
                account.balance = account.balance.saturating_add(balance_addition);
            } else {
                account.balance = account.balance.saturating_sub(balance_addition);
            };

            let code_hash = if Some(location_hash) == self.to_hash {
                self.to_code_hash
            } else {
                self.get_code_hash(address)?
            };
            let code = if let Some(code_hash) = &code_hash {
                if let Some(code) = self.mv_memory.new_bytecodes.get(code_hash) {
                    Some(code.clone())
                } else {
                    match self.storage.code_by_hash(code_hash) {
                        Ok(code) => code.map(Bytecode::from),
                        Err(err) => return Err(ReadError::StorageError(err.to_string())),
                    }
                }
            } else {
                None
            };
            self.read_accounts
                .insert(location_hash, (account.clone(), code_hash));

            // M1b: cache single-origin basics for FF (lazy chains have multi-origins).
            if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
                let origins = self.read_set.get(&location_hash);
                if origins.is_some_and(|o| o.len() == 1) {
                    let origin = match origins.and_then(|o| o.first()) {
                        Some(ReadOrigin::MvMemory(v)) => Some((v.tx_idx, v.tx_incarnation)),
                        Some(ReadOrigin::Storage) | None => None,
                    };
                    self.specfence.partial_retry.note_value(
                        self.tx_idx,
                        location_hash,
                        FfValue::Basic {
                            address,
                            basic: account.clone(),
                            code_hash,
                            origin,
                        },
                    );
                }
            }

            self.maybe_early_val(address, location_hash)?;
            return Ok(Some(AccountInfo {
                balance: account.balance,
                nonce: account.nonce,
                code_hash: code_hash.unwrap_or(KECCAK_EMPTY),
                code,
                account_id: None,
            }));
        }

        self.maybe_early_val(address, location_hash)?;
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
        self.maybe_wait(address, location_hash)?;

        // M1b: certified-prefix FF cache — skip MV/storage heavy path when origin stable.
        if let Some((value, origin)) = self.try_ff_storage(location_hash) {
            let read_origins = self.read_set.entry(location_hash).or_default();
            Self::push_origin(read_origins, origin)?;
            self.specfence.metrics.record_journal_ff_hit();
            if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
                self.specfence.partial_retry.note_value(
                    self.tx_idx,
                    location_hash,
                    FfValue::Storage {
                        address,
                        slot: index,
                        value,
                        origin: match self.read_set.get(&location_hash).and_then(|o| o.last()) {
                            Some(ReadOrigin::MvMemory(v)) => Some((v.tx_idx, v.tx_incarnation)),
                            _ => None,
                        },
                    },
                );
            }
            return Ok(value);
        }

        let read_origins = self.read_set.entry(location_hash).or_default();

        // Try reading from multi-version data
        if self.tx_idx > 0
            && let Some(written_transactions) = self.mv_memory.data.get(&location_hash)
            && let Some((closest_idx, entry)) =
                written_transactions.range(..self.tx_idx).next_back()
        {
            match entry {
                MemoryEntry::Data(tx_incarnation, MemoryValue::Storage(value)) => {
                    if self
                        .mv_memory
                        .is_aborted_incarnation(*closest_idx, *tx_incarnation)
                    {
                        self.promote_on_conflict(address, location_hash);
                        return Err(ReadError::Blocking(*closest_idx));
                    }
                    self.specfence.metrics.record_db_heavy_op();
                    Self::push_origin(
                        read_origins,
                        ReadOrigin::MvMemory(TxVersion {
                            tx_idx: *closest_idx,
                            tx_incarnation: *tx_incarnation,
                        }),
                    )?;
                    if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
                        self.specfence.partial_retry.note_value(
                            self.tx_idx,
                            location_hash,
                            FfValue::Storage {
                                address,
                                slot: index,
                                value: *value,
                                origin: Some((*closest_idx, *tx_incarnation)),
                            },
                        );
                    }
                    self.maybe_early_val(address, location_hash)?;
                    return Ok(*value);
                }
                MemoryEntry::Estimate => {
                    self.promote_on_conflict(address, location_hash);
                    return Err(ReadError::Blocking(*closest_idx));
                }
                _ => return Err(ReadError::InvalidMemoryValueType),
            }
        }

        // Fall back to storage
        self.specfence.metrics.record_db_heavy_op();
        Self::push_origin(read_origins, ReadOrigin::Storage)?;
        let value = self
            .storage
            .storage(&address, &index)
            .map_err(|err| ReadError::StorageError(err.to_string()))?;
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            self.specfence.partial_retry.note_value(
                self.tx_idx,
                location_hash,
                FfValue::Storage {
                    address,
                    slot: index,
                    value,
                    origin: None,
                },
            );
        }
        self.maybe_early_val(address, location_hash)?;
        Ok(value)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.storage
            .block_hash(&number)
            .map_err(|err| ReadError::StorageError(err.to_string()))
    }
}

// Per-worker execution VM. Holds all block-level state and a reusable EVM.
pub(crate) struct Vm<'a, S: Storage, C: PevmChain> {
    // Shared block-level state
    chain: &'a C,
    is_eip_161_enabled: bool,
    block_env: &'a BlockEnv,
    txs: &'a [C::EvmTx],
    mv_memory: &'a MvMemory,
    specfence: SpecFenceCtx<'a>,
    beneficiary_location_hash: MemoryLocationHash,
    // Dedicated EVM for the worker, reset before each transaction exectution.
    evm: C::Evm<VmDb<'a, S>>,
}

impl<'a, S: Storage, C: PevmChain> Vm<'a, S, C> {
    pub(crate) fn new(
        chain: &'a C,
        spec_id: C::EvmSpecId,
        block_env: &'a BlockEnv,
        txs: &'a [C::EvmTx],
        storage: &'a S,
        mv_memory: &'a MvMemory,
        specfence: SpecFenceCtx<'a>,
    ) -> Self {
        // The DB is initialised with mock values; each transaction execution
        // [VmDb::set_tx] the intended transaction before executing.
        let db = VmDb {
            storage,
            mv_memory,
            specfence,
            tx_idx: 0,
            // SAFETY: txs is non-empty (checked by the caller before spawning threads).
            tx: chain.tx_env(unsafe { txs.get_unchecked(0) }),
            from_hash: 0,
            to_hash: None,
            to_code_hash: None,
            is_lazy: false,
            has_nonce: true,
            // Unless it is a raw transfer that is lazy updated, we'll
            // read at least from the sender and recipient accounts.
            read_set: ReadSet::with_capacity_and_hasher(2, BuildIdentityHasher::default()),
            read_accounts: HashMap::with_capacity_and_hasher(2, BuildIdentityHasher::default()),
        };
        Self {
            chain,
            is_eip_161_enabled: chain.is_eip_161_enabled(spec_id),
            block_env,
            txs,
            mv_memory,
            specfence,
            beneficiary_location_hash: hash_deterministic(MemoryLocation::Basic(
                block_env.beneficiary,
            )),
            evm: chain.build_evm(spec_id, block_env.clone(), db),
        }
    }

    /// Hinted Wait admission: previous `from`/`to` writer that is not done yet.
    pub(crate) fn hinted_wait_blocker(&self, tx_idx: TxIdx) -> Option<(TxIdx, Address)> {
        if !self.specfence.mode.uses_regions() {
            return None;
        }
        // R1: proactive Wait only via should_wait_account (HotSet-gated). No block-wide lean skip.
        let tx = self.chain.tx_env(unsafe { self.txs.get_unchecked(tx_idx) });
        if let Some(prev) = self
            .specfence
            .wait_blocker(&self.mv_memory.regions, &tx.caller, tx_idx)
        {
            return Some((prev, tx.caller));
        }
        if let Some(to) = tx.kind.to()
            && let Some(prev) = self
                .specfence
                .wait_blocker(&self.mv_memory.regions, to, tx_idx)
        {
            return Some((prev, *to));
        }
        None
    }

    /// SpecFence M2: location of WaitHard that returned Blocking (if any).
    pub(crate) fn take_pending_park_location(&self) -> Option<crate::MemoryLocationHash> {
        self.specfence.wave.take_pending_park_location()
    }

    pub(crate) fn record_wait_admission(&self, address: Address) {
        self.specfence.metrics.record_wait(address);
    }

    fn promote_region(&self, location: MemoryLocationHash, address: Option<Address>) {
        // WW contention → intra-block Wait; mild once-per-block Bayes conflict.
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            if self.specfence.bayes.observe_conflict_location(location) {
                self.specfence.metrics.record_bayes_conflict();
                if let Some(address) = address {
                    if address != self.specfence.beneficiary {
                        self.specfence.bayes.observe_conflict_account_n(address, 3);
                    }
                }
            }
            self.specfence
                .promote_from_bayes(&self.mv_memory.regions, location, address);
            return;
        }
        if self.mv_memory.regions.promote_location(location) {
            self.specfence.metrics.record_promotion(address);
        }
        if let Some(address) = address {
            if address == self.specfence.beneficiary {
                return;
            }
            self.mv_memory.regions.promote_account(address);
            self.specfence.metrics.mark_hot(address);
        }
    }

    fn promote_if_multi_writer(&self, address: Address, location: Option<MemoryLocationHash>) {
        // SpecFence v1: do not promote Wait from mere from/to writer_count hints.
        // Intra-block Wait comes from observed invalid locations / WW contention.
        // Inter-block Bayes posteriors seed Wait via seed_wait_regions.
        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
            return;
        }
        if address == self.specfence.beneficiary {
            return;
        }
        if self.specfence.hints.writer_count(&address) < 2 {
            return;
        }
        if let Some(location) = location {
            self.promote_region(location, Some(address));
        } else {
            self.mv_memory.regions.promote_account(address);
            self.specfence.metrics.mark_hot(address);
        }
    }

    // Execute a transaction. This can read from memory but cannot modify any state.
    // A successful execution returns:
    //   - A write-set consisting of memory locations and their updated values.
    //   - A read-set consisting of memory locations and their origins.
    //
    // An execution may observe a read dependency on a lower transaction. This happens
    // when the last incarnation of the dependency wrote to a memory location that
    // this transaction reads, but it aborted before the read. In this case, the
    // dependency index is returned via [blocking_tx_idx]. An execution task for this
    // transaction is re-scheduled after the blocking dependency finishes its
    // next incarnation.
    //
    // When a transaction attempts to write a value to a location, the location and
    // value are added to the write set, possibly replacing a pair with a prior value
    // (if it is not the first time the transaction wrote to this location during the
    // execution).
    pub(crate) fn execute(
        &mut self,
        tx_version: &TxVersion,
        result_slot: &mut Option<PevmTxExecutionResult>,
) -> Result<FinishExecFlags, VmExecutionError> {
        // SAFETY: A correct scheduler would guarantee this index to be inbound.
        let full_tx = unsafe { self.txs.get_unchecked(tx_version.tx_idx) };
        let tx = self.chain.tx_env(full_tx);

        let from_hash = hash_deterministic(MemoryLocation::Basic(tx.caller));
        let to_hash = tx
            .kind
            .to()
            .map(|to| hash_deterministic(MemoryLocation::Basic(*to)));

        let has_nonce = self.chain.has_nonce(&mut self.evm, full_tx);

        // Prepare state for execution
        {
            let ctx = self.evm.ctx();

            ctx.db_mut()
                .set_tx(
                    tx_version.tx_idx,
                    tx,
                    from_hash,
                    to_hash,
                    has_nonce,
                    tx_version.tx_incarnation,
                )
                .map_err(VmExecutionError::from)?;

            ctx.set_tx(full_tx.clone());

            // We reset the journal when we finalise it into the result state on a
            // successful execution but not on errors. Always reset here to be sure.
            ctx.journal_mut().clear();
        }

        // Plant v2 M1: RewindTo resume must NOT call record_evm_entry / head path.
        // Fresh starts (incl. FullRestart) still count as evm_entries.
        let lean = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && self.specfence.engagement.begin_tx(tx_version.tx_idx);
        let rewind_resume = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && !lean
            && self
                .specfence
                .partial_retry
                .is_rewind_resume(tx_version.tx_idx);
        if rewind_resume {
            // M1b/M1d: journal FF in set_tx; live PC arm below inside inspect_run.
            self.specfence.metrics.record_resume();
        } else {
            self.specfence.metrics.record_evm_entry();
            // CallEntry/CallExit come from SpecFenceInspector on the inspect path.
            // OCC/PCC keep a synthetic CallEntry for PartialRetry bookkeeping only
            // when SpecFence mode... (SpecFence uses inspector hooks instead).
            // M4 lean: no checkpoints (OCC-fast Handler::run).
            if self.specfence.mode == crate::ConcurrencyMode::SpecFence && !lean {
                // Still record a k=0 CallEntry so plan_repair has a floor cp when
                // the inspector call hook is delayed; inspector may add richer cps.
                let _ = self.specfence.partial_retry.push_checkpoint(
                    tx_version.tx_idx,
                    CheckpointKind::CallEntry,
                );
            }
        }

        // M1d: seed certified-prefix read origins so validation still covers
        // locations whose SLOAD/BALANCE opcodes are PC-skipped.
        if rewind_resume {
            let db = self.evm.ctx().db_mut();
            for (location_hash, ff) in self.specfence.partial_retry.ff_values(tx_version.tx_idx)
            {
                let origin = match &ff {
                    FfValue::Storage { origin, .. } | FfValue::Basic { origin, .. } => *origin,
                };
                let read_origin = match origin {
                    Some((tx_idx, tx_incarnation)) => ReadOrigin::MvMemory(TxVersion {
                        tx_idx,
                        tx_incarnation,
                    }),
                    None => ReadOrigin::Storage,
                };
                db.read_set
                    .entry(location_hash)
                    .or_default()
                    .push(read_origin);
            }
        }

        // R0: Handler::run by default; inspect_run only with SPECFENCE_ENABLE_INSPECT=1.
        let use_inspect = self.specfence.mode == crate::ConcurrencyMode::SpecFence
            && !lean
            && crate::specfence::research_inspect_enabled();
        let run_result = if use_inspect {
            let partial_retry = self.specfence.partial_retry;
            let metrics = self.specfence.metrics;
            let tx_idx = tx_version.tx_idx;
            with_plant_tls(tx_idx, partial_retry, metrics, || {
                if rewind_resume {
                    let jumped = partial_retry.ff_continuation(tx_idx).is_some_and(|cont| {
                        try_arm_safe_absolute_jump(tx_idx, partial_retry, &cont, metrics)
                    });
                    if !jumped {
                        // M1g: still arm CallOutcome cache on fallback resume so
                        // certified nested CALLs short-circuit without absolute jump.
                        if let Some(cont) = partial_retry.ff_continuation(tx_idx) {
                            if !cont.call_outcomes.is_empty() {
                                arm_call_outcome_cache(cont.call_outcomes);
                            }
                        }
                        if let Some(snap) = partial_retry.ff_boundary(tx_idx) {
                            if snap.opcode_steps > 0 {
                                metrics.record_pc_resume(snap.opcode_steps);
                            }
                        } else {
                            let n = partial_retry.ff_entries(tx_idx) as u64;
                            if n > 0 {
                                metrics.record_pc_resume(n);
                            }
                        }
                    }
                }
                let result = self.chain.run_pevm_tx(&mut self.evm, true);
                if rewind_resume {
                    partial_retry.note_jump_applied(tx_idx, resume_was_applied());
                }
                let steps = steps_this_run();
                metrics.record_inspector_steps(steps, rewind_resume);
                result
            })
        } else {
            self.chain.run_pevm_tx(&mut self.evm, false)
        };

        match run_result {

            Ok(exec_result) => {
                // M1f: jumped Success may commit when jump_is_safe. Validation abort
                // still disables further jumps via pevm.rs circuit breaker (anti-livelock).

                // There are at least six locations most of the time: the sender,
                // the recipient, and up to four fee recipients (beneficiary, base fee,
                // L1 fee, operator fee on OP Stack chains).
                let mut write_set = WriteSet::with_capacity(6);

                let ctx = self.evm.ctx();
                let state = ctx.journal_mut().finalize();

                for (address, account) in &state {
                    if account.is_selfdestructed() {
                        // TODO: Also write [SelfDestructed] to the basic location?
                        // For now we are betting on [code_hash] triggering the sequential
                        // fallback when we read a self-destructed contract.
                        write_set.push((
                            hash_deterministic(MemoryLocation::CodeHash(*address)),
                            MemoryValue::SelfDestructed,
                        ));
                        continue;
                    }

                    if account.is_touched() {
                        let account_location_hash =
                            hash_deterministic(MemoryLocation::Basic(*address));
                        let read_account = ctx.db().read_accounts.get(&account_location_hash);

                        let has_code = !account.info.is_empty_code_hash();
                        let is_new_code = has_code
                            && read_account.is_none_or(|(_, code_hash)| code_hash.is_none());

                        // Write new account changes
                        if is_new_code
                            || read_account.is_none()
                            || read_account.is_some_and(|(basic, _)| {
                                basic.nonce != account.info.nonce
                                    || basic.balance != account.info.balance
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
                            }
                            // We don't register empty accounts after [SPURIOUS_DRAGON]
                            // as they are cleared. This can only happen via 2 ways:
                            // 1. Self-destruction which is handled by an if above.
                            // 2. Sending 0 ETH to an empty account, which we treat as a
                            // non-write here. A later read would trace back to storage
                            // and return a [None], i.e., [LoadedAsNotExisting]. Without
                            // this check it would write then read a [Some] default
                            // account, which may yield a wrong gas fee, etc.
                            else if !self.is_eip_161_enabled || !account.is_empty() {
                                write_set.push((
                                    account_location_hash,
                                    MemoryValue::Basic(AccountBasic {
                                        balance: account.info.balance,
                                        nonce: account.info.nonce,
                                    }),
                                ));
                            }
                        }

                        // Write new contract
                        if is_new_code {
                            write_set.push((
                                hash_deterministic(MemoryLocation::CodeHash(*address)),
                                MemoryValue::CodeHash(account.info.code_hash),
                            ));
                            self.mv_memory
                                .new_bytecodes
                                .entry(account.info.code_hash)
                                .or_insert_with(|| account.info.code.clone().unwrap());
                        }
                    }

                    // TODO: We should move this changed check to our read set like for account info?
                    for (slot, value) in account.changed_storage_slots() {
                        let loc = hash_deterministic(MemoryLocation::Storage(*address, *slot));
                        write_set.push((loc, MemoryValue::Storage(value.present_value)));
                        // M1i: capture storage presents for RewindTo residual republish
                        // + absolute-jump journal slot replay (never journal-blob poison).
                        // gas_remaining_after filled from Inspector post-SSTORE captures.
                        if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
                            self.specfence.partial_retry.note_write_replay(
                                tx_version.tx_idx,
                                loc,
                                StorageWriteReplay {
                                    address: *address,
                                    slot: *slot,
                                    original: value.original_value,
                                    present: value.present_value,
                                    gas_remaining_after: 0, // filled via post_sstore_gases
                                },
                            );
                        }
                    }
                }

                // M1d/M1e/M1h: when live PC jump omitted prefix SSTORE/account writes from
                // the revm journal, re-publish certified-prefix writes so record() does
                // not drop them — from MvMemory residual Data and/or SpecFence
                // write_replays (never journal-blob present_values).
                if rewind_resume {
                    let suffix: hashbrown::HashSet<_, BuildIdentityHasher> = self
                        .specfence
                        .partial_retry
                        .ff_suffix_writes(tx_version.tx_idx)
                        .into_iter()
                        .collect();
                    for loc in self.mv_memory.residual_writes(tx_version.tx_idx) {
                        if suffix.contains(&loc) {
                            continue;
                        }
                        if write_set.iter().any(|(l, _)| *l == loc) {
                            continue;
                        }
                        if let Some(value) =
                            self.mv_memory.published_data_value(tx_version.tx_idx, loc)
                        {
                            write_set.push((loc, value));
                        }
                    }
                    // M1h: SpecFence-captured write presents (source of truth when
                    // residual was ESTIMATEd or jump skipped SSTORE in revm journal).
                    if let Some(cont) = self.specfence.partial_retry.ff_continuation(tx_version.tx_idx)
                    {
                        for wr in &cont.write_replays {
                            let loc = hash_deterministic(MemoryLocation::Storage(
                                wr.address,
                                wr.slot,
                            ));
                            if suffix.contains(&loc) {
                                continue;
                            }
                            if write_set.iter().any(|(l, _)| *l == loc) {
                                continue;
                            }
                            write_set.push((loc, MemoryValue::Storage(wr.present)));
                        }
                    }
                }

                // Rewards
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
                            _ => return Err(ReadError::InvalidMemoryValueType.into()),
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
                    self.mv_memory
                        .add_lazy_addresses([tx.caller, *tx.kind.to().unwrap()]);
                }

                let mut flags = if tx_version.tx_idx > 0 && !is_lazy {
                    FinishExecFlags::NeedValidation
                } else {
                    FinishExecFlags::empty()
                };

                if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
                    for (loc, value) in &write_set {
                        self.specfence.partial_retry.note_access(
                            tx_version.tx_idx,
                            *loc,
                            AccessMode::Write,
                        );
                        let kind = match value {
                            MemoryValue::Basic(_)
                            | MemoryValue::LazySender(_)
                            | MemoryValue::LazyRecipient(_)
                            | MemoryValue::SelfDestructed => CheckpointKind::AccountWrite,
                            MemoryValue::Storage(_) => CheckpointKind::StorageWrite,
                            // Code-hash / other account-adjacent writes.
                            _ => CheckpointKind::AccountWrite,
                        };
                        let _ = self
                            .specfence
                            .partial_retry
                            .push_checkpoint(tx_version.tx_idx, kind);
                    }
                    let _ = self.specfence.partial_retry.push_checkpoint(
                        tx_version.tx_idx,
                        CheckpointKind::CallExit,
                    );
                }

                // R3: HotSet H_w ignores LazyRecipient multi-writer noise (popular
                // payees on wide blocks; fine-grain G* drops basic_lazy). Keep
                // LazySender so same-sender RAW storms still escalate. Storage/Basic
                // full writes always count. Still learn full WŜ for M3 prior.
                let hotset_writer_locs: Vec<MemoryLocationHash> = write_set
                    .iter()
                    .filter_map(|(loc, val)| {
                        if *loc == self.beneficiary_location_hash {
                            return None;
                        }
                        match val {
                            MemoryValue::LazyRecipient(_) => None,
                            _ => Some(*loc),
                        }
                    })
                    .collect();
                let (wrote_new_location, contended) =
                    self.mv_memory.record(tx_version, read_set, write_set);
                // M3: learn process WŜ from this incarnation's writes (no residual publish).
                // R1/R3: feed HotSet writer counts (H_w) from non-lazy writes only.
                if self.specfence.mode == crate::ConcurrencyMode::SpecFence {
                    let locs: Vec<_> = self.mv_memory.write_locations(tx_version.tx_idx);
                    self.specfence.rw_prior.observe_write_set(&locs, None);
                    for loc in hotset_writer_locs {
                        self.specfence.hotset.note_writer(loc, tx_version.tx_idx);
                    }
                }
                if wrote_new_location {
                    flags |= FinishExecFlags::WroteNewLocation;
                }
                if self.specfence.mode.uses_regions() {
                    let from_wait = self
                        .specfence
                        .should_wait_account(&self.mv_memory.regions, &tx.caller);
                    let to_wait = tx.kind.to().is_some_and(|to| {
                        self.specfence
                            .should_wait_account(&self.mv_memory.regions, to)
                    });
                    // Independents (no hinted predecessor) still count as Speculate
                    // even when PCC seeds their account Wait with nobody to wait for.
                    let from_has_pred = self
                        .specfence
                        .hints
                        .prev(&tx.caller, tx_version.tx_idx)
                        .is_some();
                    let to_has_pred = tx.kind.to().is_some_and(|to| {
                        self.specfence.hints.prev(to, tx_version.tx_idx).is_some()
                    });
                    if (!from_wait || !from_has_pred) && (!to_wait || !to_has_pred) {
                        self.specfence
                            .metrics
                            .record_speculate(tx.caller, tx.kind.to().copied());
                    }
                    // Wave: a second hinted writer to the same account is a WW overlap.
                    self.promote_if_multi_writer(tx.caller, Some(from_hash));
                    if let Some(to) = tx.kind.to().copied() {
                        self.promote_if_multi_writer(to, to_hash);
                    }
                    for loc in contended {
                        if loc == self.beneficiary_location_hash {
                            continue;
                        }
                        let addr = if loc == from_hash {
                            Some(tx.caller)
                        } else if Some(loc) == to_hash {
                            tx.kind.to().copied()
                        } else {
                            None
                        };
                        if addr == Some(self.specfence.beneficiary) {
                            continue;
                        }
                        self.promote_region(loc, addr);
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
                Ok(flags)
            }
            Err(EVMError::Database(read_error)) => Err(VmExecutionError::from(read_error)),
            Err(err) => {
                // Optimistically retry in case some previous internal transactions send
                // more fund to the sender but hasn't been executed yet.
                // TODO: Let users define this behaviour through a mode enum or something.
                // Since this retry is safe for syncing canonical blocks but can deadlock
                // on new or faulty blocks. We can skip the transaction for new blocks and
                // error out after a number of tries for the latter.
                if tx_version.tx_idx > 0
                    && matches!(
                        err,
                        EVMError::Transaction(
                            InvalidTransaction::LackOfFundForMaxFee { .. }
                                | InvalidTransaction::NonceTooHigh { .. }
                        )
                    )
                {
                    Err(VmExecutionError::Blocking(tx_version.tx_idx - 1))
                } else {
                    Err(VmExecutionError::ExecutionError(err))
                }
            }
        }
    }
}
