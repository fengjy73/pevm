use std::{
    cell::UnsafeCell,
    fmt::Debug,
    num::NonZeroUsize,
    sync::{OnceLock, mpsc},
    thread,
};

use alloy_primitives::{TxNonce, U256};
use alloy_rpc_types_eth::{Block, BlockTransactions};
use hashbrown::HashMap;
use revm::{
    DatabaseCommit, ExecuteEvm,
    context::{
        BlockEnv, ContextTr, Transaction,
        result::{InvalidTransaction, ResultAndState},
    },
    database::CacheDB,
    handler::EvmTr,
};

use crate::{
    EvmAccount, MemoryEntry, MemoryLocation, MemoryValue, Storage, Task, TxIdx, TxVersion,
    chain::PevmChain,
    compat::get_block_env,
    hash_deterministic,
    mv_memory::MvMemory,
    scheduler::Scheduler,
    specfence::{
        AccountHints, BayesMap, ConcurrencyMode, DEFAULT_TAU, HeatMap, MetricsInner,
        PartialRetryTable, RemCounters, SpecDag, SpecFenceCtx, SpecFenceMetrics,
        seed_wait_regions, update_bayes, update_heat,
    },
    storage::StorageWrapper,
    vm::{
        ExecutionError, PevmTxExecutionResult, Vm, VmExecutionError, receipt_from_revm,
        state_transitions_from_revm,
    },
};

/// Errors when executing a block with pevm.
// TODO: implement traits explicitly due to trait bounds on `C` instead of types of `PevmChain`
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PevmError<C: PevmChain> {
    /// Cannot derive the chain spec from the block header.
    #[error("Cannot derive the chain spec from the block header")]
    BlockSpecError(#[source] C::BlockSpecError),
    /// Transactions lack information for execution.
    #[error("Transactions lack information for execution")]
    MissingTransactionData,
    /// Invalid input transaction.
    #[error("Invalid input transaction")]
    InvalidTransaction(#[source] C::TransactionParsingError),
    /// Nonce too low or too high
    #[error("Nonce mismatch for tx #{tx_idx}. Expected {executed_nonce}, got {tx_nonce}")]
    NonceMismatch {
        /// Transaction index
        tx_idx: TxIdx,
        /// Nonce from tx (from the very input)
        tx_nonce: TxNonce,
        /// Nonce from state and execution
        executed_nonce: TxNonce,
    },
    /// Storage error.
    // TODO: More concrete types than just an arbitrary string.
    #[error("Storage error: {0}")]
    StorageError(String),
    /// EVM execution error.
    #[error("Execution error")]
    ExecutionError(
        #[source]
        #[from]
        ExecutionError,
    ),
    /// Impractical errors that should be unreachable.
    /// The library has bugs if this is yielded.
    #[error(
        "PEVM encountered a bug. Please open an issue in https://github.com/risechain/pevm/issues/new"
    )]
    UnreachableError,
}

/// Execution result of a block
pub type PevmResult<C> = Result<Vec<PevmTxExecutionResult>, PevmError<C>>;

#[derive(Debug)]
enum AbortReason {
    FallbackToSequential,
    ExecutionError(ExecutionError),
}

// TODO: Better implementation
#[derive(Debug)]
struct AsyncDropper<T> {
    sender: mpsc::Sender<T>,
    _handle: thread::JoinHandle<()>,
}

impl<T: Send + 'static> Default for AsyncDropper<T> {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            _handle: std::thread::spawn(move || receiver.into_iter().for_each(drop)),
        }
    }
}

impl<T> AsyncDropper<T> {
    fn drop(&self, t: T) {
        let _ = self.sender.send(t);
    }
}

// Reusable per-block execution result buffer. Each slot is an `UnsafeCell` so workers
// can write into it while sharing a thread-safe reference to the buffer at runtime
// without synchronisation overheads (like putting each slot behind a mutex).
//
// All unsafe operations are centralised here as they share the same invariant:
// The scheduler assigns exclusive `Executing` status to exactly one worker thread
// per slot at a time, and result collection runs only after all worker threads have
// joined. Other worker tasks like validation don't touch these results at all.
#[derive(Debug, Default)]
struct ExecutionResults(Vec<UnsafeCell<Option<PevmTxExecutionResult>>>);

unsafe impl Sync for ExecutionResults {}

impl ExecutionResults {
    fn grow_to(&mut self, block_size: usize) {
        if block_size > self.0.len() {
            self.0.resize_with(block_size, || UnsafeCell::new(None));
        }
    }

    #[allow(clippy::mut_from_ref)]
    fn slot_mut(&self, tx_idx: TxIdx) -> &mut Option<PevmTxExecutionResult> {
        unsafe { &mut *self.0.get_unchecked(tx_idx).get() }
    }

    fn take_slot(&self, tx_idx: TxIdx) -> PevmTxExecutionResult {
        unsafe {
            (*self.0.get_unchecked(tx_idx).get())
                .take()
                .unwrap_unchecked()
        }
    }
}

// TODO: Port more recyclable resources into here.
#[derive(Debug)]
/// The main pevm struct that executes blocks.
pub struct Pevm {
    execution_results: ExecutionResults,
    abort_reason: OnceLock<AbortReason>,
    dropper: AsyncDropper<(MvMemory, Scheduler)>,
    concurrency_mode: ConcurrencyMode,
    heat: HeatMap,
    bayes: BayesMap,
    last_metrics: SpecFenceMetrics,
    last_initial_wait_accounts: std::collections::HashSet<alloy_primitives::Address>,
}

impl Default for Pevm {
    fn default() -> Self {
        Self {
            execution_results: ExecutionResults::default(),
            abort_reason: OnceLock::new(),
            dropper: AsyncDropper::default(),
            concurrency_mode: ConcurrencyMode::Occ,
            heat: HeatMap::new(),
            bayes: BayesMap::new(),
            last_metrics: SpecFenceMetrics::default(),
            last_initial_wait_accounts: std::collections::HashSet::new(),
        }
    }
}

impl Pevm {
    /// Create an executor with a concurrency-control mode. Default is OCC.
    pub fn with_concurrency_mode(mode: ConcurrencyMode) -> Self {
        Self {
            concurrency_mode: mode,
            ..Self::default()
        }
    }

    /// Set the concurrency-control mode for subsequent blocks.
    pub const fn set_concurrency_mode(&mut self, mode: ConcurrencyMode) {
        self.concurrency_mode = mode;
    }

    /// Current concurrency-control mode.
    pub const fn concurrency_mode(&self) -> ConcurrencyMode {
        self.concurrency_mode
    }

    /// Metrics from the last parallel execution (OCC/PCC/`SpecFence`).
    pub const fn last_specfence_metrics(&self) -> &SpecFenceMetrics {
        &self.last_metrics
    }

    /// Accounts seeded in Wait at the start of the last parallel block.
    pub const fn last_initial_wait_accounts(
        &self,
    ) -> &std::collections::HashSet<alloy_primitives::Address> {
        &self.last_initial_wait_accounts
    }

    /// Clear inter-block heat and Bayesian posteriors (test / replay).
    pub fn reset_heat(&mut self) {
        self.heat.reset();
        self.bayes.reset();
        self.last_initial_wait_accounts.clear();
    }

    /// Conflict probability for an account-level region (tests / diagnostics).
    pub fn bayes_account_conflict_prob(&self, address: &alloy_primitives::Address) -> f64 {
        self.bayes.account_wait_probability(address)
    }

    /// Conflict probability for a location hash (tests / diagnostics).
    pub fn bayes_location_conflict_prob(&self, location: u64) -> f64 {
        self.bayes.prior_wait_probability(location)
    }

    /// Execute an Alloy block, which is becoming the "standard" format in Rust.
    /// TODO: Better error handling.
    pub fn execute<S, C>(
        &mut self,
        chain: &C,
        storage: &S,
        // We assume the block is still needed afterwards like in most Reth cases
        // so take in a reference and only copy values when needed. We may want
        // to use a [`std::borrow::Cow`] to build [`BlockEnv`] and [`TxEnv`] without
        // (much) copying when ownership can be given. Another challenge with this is
        // the new Alloy [`Transaction`] interface that is mostly `&self`. We'd need
        // to do some dirty destruction to get the owned fields.
        block: &Block<C::Transaction>,
        concurrency_level: NonZeroUsize,
        force_sequential: bool,
    ) -> PevmResult<C>
    where
        C: PevmChain + Send + Sync,
        S: Storage + Send + Sync + Debug,
    {
        let spec_id = chain
            .get_block_spec(&block.header)
            .map_err(PevmError::BlockSpecError)?;
        let block_env = get_block_env(&block.header, spec_id);
        let tx_envs = match &block.transactions {
            BlockTransactions::Full(txs) => txs
                .iter()
                .map(|tx| chain.get_tx_env(tx))
                .collect::<Result<Vec<_>, _>>()
                .map_err(PevmError::InvalidTransaction)?,
            _ => return Err(PevmError::MissingTransactionData),
        };
        // TODO: Continue to fine tune this condition.
        if force_sequential
            || tx_envs.len() < concurrency_level.into()
            || block.header.gas_used < 4_000_000
        {
            execute_revm_sequential(chain, storage, spec_id, block_env, tx_envs)
        } else {
            self.execute_revm_parallel(
                chain,
                storage,
                spec_id,
                block_env,
                tx_envs,
                concurrency_level,
            )
        }
    }

    /// Execute an REVM block.
    // Ideally everyone would go through the [Alloy] interface. This one is currently
    // useful for testing, and for users that are heavily tied to Revm like Reth.
    pub fn execute_revm_parallel<S, C>(
        &mut self,
        chain: &C,
        storage: &S,
        spec_id: C::EvmSpecId,
        block_env: BlockEnv,
        txs: Vec<C::EvmTx>,
        concurrency_level: NonZeroUsize,
    ) -> PevmResult<C>
    where
        C: PevmChain + Send + Sync,
        S: Storage + Send + Sync + Debug,
    {
        if txs.is_empty() {
            return Ok(Vec::new());
        }

        let block_size = txs.len();
        let scheduler = Scheduler::new(block_size);

        let mv_memory = chain.build_mv_memory(&block_env, &txs);
        let hints = AccountHints::build(chain, &txs);
        let metrics_inner = MetricsInner::default();
        let mut initial_wait = std::collections::HashSet::new();
        seed_wait_regions(
            &mv_memory.regions,
            &hints,
            &self.bayes,
            self.concurrency_mode,
            block_env.beneficiary,
            DEFAULT_TAU,
            &mut initial_wait,
        );

        self.execution_results.grow_to(block_size);

        let dag = SpecDag::new();
        let rem = RemCounters::default();
        let partial_retry = PartialRetryTable::new(block_size);
        let specfence = SpecFenceCtx {
            mode: self.concurrency_mode,
            hints: &hints,
            metrics: &metrics_inner,
            scheduler: &scheduler,
            beneficiary: block_env.beneficiary,
            bayes: &self.bayes,
            tau: DEFAULT_TAU,
            dag: &dag,
            rem: &rem,
            partial_retry: &partial_retry,
        };

        // TODO: Better thread handling
        thread::scope(|scope| {
            for _ in 0..concurrency_level.into() {
                scope.spawn(|| {
                    let mut vm = Vm::new(
                        chain, spec_id, &block_env, &txs, storage, &mv_memory, specfence,
                    );
                    let mut task = scheduler.next_task();
                    while task.is_some() {
                        task = match task.unwrap() {
                            Task::Execution(tx_version) => {
                                self.try_execute(&mut vm, &scheduler, tx_version)
                            }
                            Task::Validation(tx_version) => {
                                try_validate(&mv_memory, &scheduler, &tx_version, specfence)
                            }
                        };

                        // TODO: Have different functions or an enum for the caller to choose
                        // the handling behaviour when a transaction's EVM execution fails.
                        // Parallel block builders would like to exclude such transaction,
                        // verifiers may want to exit early to save CPU cycles, while testers
                        // may want to collect all execution results. We are exiting early as
                        // the default behaviour for now.
                        if self.abort_reason.get().is_some() {
                            break;
                        }

                        if task.is_none() {
                            task = scheduler.next_task();
                        }
                    }
                });
            }
        });

        if self.concurrency_mode == ConcurrencyMode::Pcc {
            update_heat(&self.heat, &hints, &metrics_inner, block_env.beneficiary);
        }
        let (mean_wait, mean_p_at_wait, mean_p_at_spec) =
            if self.concurrency_mode == ConcurrencyMode::SpecFence {
                update_bayes(&self.bayes);
                // mean_wait_posterior keeps historical Wait-decision mean;
                // cost-aware means are taken after (same accumulators for wait).
                let mean_wait = self.bayes.take_mean_wait_posterior();
                let mean_spec = self.bayes.take_mean_spec_posterior();
                (mean_wait, mean_wait, mean_spec)
            } else {
                (0.0, 0.0, 0.0)
            };
        let wave_id = self.bayes.wave_id();
        metrics_inner.set_checkpoint_opportunities(rem.checkpoint_opportunities());
        self.last_metrics =
            metrics_inner.snapshot(wave_id, mean_wait, mean_p_at_wait, mean_p_at_spec);
        self.last_initial_wait_accounts = initial_wait;

        if let Some(abort_reason) = self.abort_reason.take() {
            match abort_reason {
                AbortReason::FallbackToSequential => {
                    self.dropper.drop((mv_memory, scheduler));
                    return execute_revm_sequential(chain, storage, spec_id, block_env, txs);
                }
                AbortReason::ExecutionError(err) => {
                    self.dropper.drop((mv_memory, scheduler));
                    return Err(PevmError::ExecutionError(err));
                }
            }
        }

        let mut fully_evaluated_results = Vec::with_capacity(block_size);
        let mut cumulative_gas_used: u64 = 0;
        for tx_idx in 0..block_size {
            let mut execution_result = self.execution_results.take_slot(tx_idx);
            cumulative_gas_used =
                cumulative_gas_used.saturating_add(execution_result.receipt.cumulative_gas_used);
            execution_result.receipt.cumulative_gas_used = cumulative_gas_used;
            fully_evaluated_results.push(execution_result);
        }

        // We fully evaluate (the balance and nonce of) the beneficiary account
        // and raw transfer recipients that may have been atomically updated.
        for address in mv_memory.consume_lazy_addresses() {
            let location_hash = hash_deterministic(MemoryLocation::Basic(address));
            if let Some(write_history) = mv_memory.data.get(&location_hash) {
                let mut balance = U256::ZERO;
                let mut nonce = 0;
                // Read from storage if the first multi-version entry is not an absolute value.
                if !matches!(
                    write_history.first_key_value(),
                    Some((_, MemoryEntry::Data(_, MemoryValue::Basic(_))))
                ) && let Ok(Some(account)) = storage.basic(&address)
                {
                    balance = account.balance;
                    nonce = account.nonce;
                }
                // Accounts that take implicit writes like the beneficiary account can be contract!
                let code_hash = match storage.code_hash(&address) {
                    Ok(code_hash) => code_hash,
                    Err(err) => return Err(PevmError::StorageError(err.to_string())),
                };
                let code = if let Some(code_hash) = &code_hash {
                    match storage.code_by_hash(code_hash) {
                        Ok(code) => code,
                        Err(err) => return Err(PevmError::StorageError(err.to_string())),
                    }
                } else {
                    None
                };

                for (tx_idx, memory_entry) in write_history.iter() {
                    let tx = chain.tx_env(unsafe { txs.get_unchecked(*tx_idx) });
                    match memory_entry {
                        MemoryEntry::Data(_, MemoryValue::Basic(info)) => {
                            // We fall back to sequential execution when reading a self-destructed account,
                            // so an empty account here would be a bug
                            debug_assert!(!(info.balance.is_zero() && info.nonce == 0));
                            balance = info.balance;
                            nonce = info.nonce;
                        }
                        MemoryEntry::Data(_, MemoryValue::LazyRecipient(addition)) => {
                            balance = balance.saturating_add(*addition);
                        }
                        MemoryEntry::Data(_, MemoryValue::LazySender(subtraction)) => {
                            // We must re-do extra sender balance checks as we mock
                            // the max value in [Vm] during execution. Ideally we
                            // can turn off these redundant checks in revm.
                            // Ideally we would share these calculations with revm
                            // (using their utility functions).
                            let mut max_fee = U256::from(tx.gas_limit)
                                .saturating_mul(U256::from(tx.gas_price))
                                .saturating_add(tx.value);
                            max_fee = max_fee.saturating_add(
                                U256::from(tx.total_blob_gas())
                                    .saturating_mul(U256::from(tx.max_fee_per_blob_gas)),
                            );
                            if balance < max_fee {
                                Err(ExecutionError::Transaction(
                                    InvalidTransaction::LackOfFundForMaxFee {
                                        balance: Box::new(balance),
                                        fee: Box::new(max_fee),
                                    },
                                ))?
                            }
                            balance = balance.saturating_sub(*subtraction);
                            nonce += 1;
                        }
                        // TODO: Better error handling
                        _ => unreachable!(),
                    }
                    // Assert that evaluated nonce is correct when address is caller.
                    if tx.caller == address {
                        let executed_nonce = if nonce == 0 {
                            return Err(PevmError::UnreachableError);
                        } else {
                            nonce - 1
                        };
                        if tx.nonce != executed_nonce {
                            // TODO: Consider falling back to sequential instead
                            return Err(PevmError::NonceMismatch {
                                tx_idx: *tx_idx,
                                tx_nonce: tx.nonce,
                                executed_nonce,
                            });
                        }
                    }
                    // SAFETY: The multi-version data structure should not leak an index over block size.
                    let tx_result = unsafe { fully_evaluated_results.get_unchecked_mut(*tx_idx) };
                    let account = tx_result.state.entry(address).or_default();
                    // TODO: Deduplicate this logic with [PevmTxExecutionResult::from_revm]
                    if chain.is_eip_161_enabled(spec_id)
                        && code_hash.is_none()
                        && nonce == 0
                        && balance == U256::ZERO
                    {
                        *account = None;
                    } else if let Some(account) = account {
                        // Explicit write: only overwrite the account info in case there are storage changes
                        // Code cannot change midblock here as we're falling back to sequential execution
                        // on reading a self-destructed contract.
                        account.balance = balance;
                        account.nonce = nonce;
                    } else {
                        // Implicit write: e.g. gas payments to the beneficiary account,
                        // which doesn't have explicit writes in [tx_result.state]
                        *account = Some(EvmAccount {
                            balance,
                            nonce,
                            code_hash,
                            code: code.clone(),
                            storage: HashMap::default(),
                        });
                    }
                }
            }
        }

        self.dropper.drop((mv_memory, scheduler));

        Ok(fully_evaluated_results)
    }

    fn try_execute<'a, S: Storage, C: PevmChain>(
        &self,
        vm: &mut Vm<'a, S, C>,
        scheduler: &Scheduler,
        tx_version: TxVersion,
    ) -> Option<Task> {
        let result_slot = self.execution_results.slot_mut(tx_version.tx_idx);
        loop {
            // Proactive Wait admission (per-region PCC), before optimistic execute.
            if let Some((blocking_tx_idx, address)) = vm.hinted_wait_blocker(tx_version.tx_idx) {
                if !scheduler.add_dependency(tx_version.tx_idx, blocking_tx_idx)
                    && self.abort_reason.get().is_none()
                {
                    continue;
                }
                vm.record_wait_admission(address);
                return None;
            }
            return match vm.execute(&tx_version, result_slot) {
                Ok(flags) => scheduler.finish_execution(tx_version, flags),
                Err(VmExecutionError::Retry) => {
                    if self.abort_reason.get().is_none() {
                        continue;
                    }
                    None
                }
                Err(VmExecutionError::FallbackToSequential) => {
                    scheduler.abort();
                    self.abort_reason
                        .get_or_init(|| AbortReason::FallbackToSequential);
                    None
                }
                Err(VmExecutionError::Blocking(blocking_tx_idx)) => {
                    if !scheduler.add_dependency(tx_version.tx_idx, blocking_tx_idx)
                        && self.abort_reason.get().is_none()
                    {
                        // Retry the execution immediately if the blocking transaction was
                        // re-executed by the time we can add it as a dependency.
                        continue;
                    }
                    None
                }
                Err(VmExecutionError::ExecutionError(err)) => {
                    scheduler.abort();
                    self.abort_reason
                        .get_or_init(|| AbortReason::ExecutionError(err));
                    None
                }
            };
        }
    }
}

fn try_validate(
    mv_memory: &MvMemory,
    scheduler: &Scheduler,
    tx_version: &TxVersion,
    specfence: SpecFenceCtx<'_>,
) -> Option<Task> {
    let read_locations = if specfence.mode == ConcurrencyMode::SpecFence {
        mv_memory.read_locations(tx_version.tx_idx)
    } else {
        Vec::new()
    };
    let invalid = if specfence.mode.uses_regions() {
        mv_memory.collect_invalid_reads(tx_version.tx_idx)
    } else {
        Vec::new()
    };
    let read_set_valid = if specfence.mode.uses_regions() {
        invalid.is_empty()
    } else {
        mv_memory.validate_read_locations(tx_version.tx_idx)
    };
    if specfence.mode == ConcurrencyMode::SpecFence && !invalid.is_empty() {
        specfence
            .metrics
            .record_region_validate_fail(invalid.len());
        // Per-location validate opportunities for Phase-2 checkpoint prep.
        for _ in &read_locations {
            specfence.rem.note_checkpoint_opportunity();
            specfence.metrics.record_checkpoint_opportunity();
        }
    }
    let aborted = !read_set_valid && scheduler.try_validation_abort(tx_version);
    if aborted {
        // Snapshot write locations before invalidate (same set).
        let write_locations = if specfence.mode == ConcurrencyMode::SpecFence {
            mv_memory.write_locations(tx_version.tx_idx)
        } else {
            Vec::new()
        };
        if specfence.mode == ConcurrencyMode::SpecFence {
            specfence.metrics.record_occ_abort();
            for location in &invalid {
                specfence.bayes.observe_conflict_location_always(*location);
                specfence.metrics.record_bayes_conflict();
                for address in specfence.hints.accounts() {
                    if address == specfence.beneficiary {
                        continue;
                    }
                    if hash_deterministic(MemoryLocation::Basic(address)) == *location {
                        specfence.bayes.observe_conflict_account(address);
                    }
                }
                specfence.promote_from_bayes(&mv_memory.regions, *location, None);
            }

            // P2: try semantic PartialRetry before FullRetry.
            let plan = specfence.partial_retry.plan_partial_retry(
                tx_version.tx_idx,
                &read_locations,
                &invalid,
                &write_locations,
            );
            let fence_locs = if let Some(plan) = plan {
                // Semantic PartialRetry: certified-prefix Bind, but still reexec from
                // tx head → counts as tx_head_reexec (not L1 resume). Next Vm::execute
                // increments evm_entries.
                specfence.metrics.record_partial_retry();
                specfence.metrics.record_tx_head_reexec();
                specfence
                    .partial_retry
                    .set_force_bind(tx_version.tx_idx, plan.certified.clone());
                let estimated = mv_memory
                    .invalidate_partial_suffix(tx_version.tx_idx, &plan.suffix_writes);
                if !estimated.is_empty() {
                    specfence
                        .metrics
                        .record_selective_invalidate(estimated.len());
                }
                // Fence only on failed-suffix writes (prefix readers stay valid).
                if estimated.is_empty() {
                    plan.suffix_writes
                } else {
                    estimated
                }
            } else {
                // Unsafe / no certified prefix → FullRetry / FullRestart from tx head.
                specfence.metrics.record_tx_full_retry();
                specfence.metrics.record_full_restart();
                specfence.metrics.record_partial_retry_fallback_full();
                specfence.partial_retry.clear_force_bind(tx_version.tx_idx);
                let (estimated, fallback) = mv_memory
                    .invalidate_selective(tx_version.tx_idx, Some(tx_version.tx_incarnation));
                if fallback {
                    specfence.metrics.record_selective_fallback_full();
                } else {
                    specfence
                        .metrics
                        .record_selective_invalidate(estimated.len().max(1));
                }
                if estimated.is_empty() {
                    write_locations.clone()
                } else {
                    estimated
                }
            };

            let rewind_to = mv_memory.min_higher_reader_of(tx_version.tx_idx, &fence_locs);
            let block_size = scheduler.block_size();
            let cascade_from = tx_version.tx_idx + 1;
            let (cascade, skipped) = match rewind_to {
                Some(to) => {
                    let to = to.min(block_size);
                    (
                        block_size.saturating_sub(to),
                        to.saturating_sub(cascade_from),
                    )
                }
                None => (0, block_size.saturating_sub(cascade_from)),
            };
            specfence.metrics.record_fence_cascade(cascade, skipped);
            return scheduler.finish_validation_fenced(tx_version, true, rewind_to);
        }
        // OCC / PCC: full write-set ESTIMATE (unchanged).
        mv_memory.convert_writes_to_estimates(tx_version.tx_idx);
        specfence.metrics.record_occ_abort();
        // OCC/PCC abort always restarts interpreter from tx head on next incarnation.
        specfence.metrics.record_full_restart();
        if specfence.mode.uses_regions() {
            for location in &invalid {
                if mv_memory.regions.promote_location(*location) {
                    specfence.metrics.record_promotion(None);
                }
            }
        }
    } else if !aborted && specfence.mode == ConcurrencyMode::SpecFence && read_set_valid {
        // Successful validation clears PartialRetry force-bind for this tx.
        specfence
            .partial_retry
            .clear_force_bind(tx_version.tx_idx);
        // Successful SpecRead validation → success++; try revoke sticky Waits.
        for location in &read_locations {
            if *location
                == hash_deterministic(MemoryLocation::Basic(specfence.beneficiary))
            {
                continue;
            }
            specfence.rem.note_checkpoint_opportunity();
            if mv_memory.regions.location_mode(*location) == crate::specfence::RegionMode::Wait {
                // Revoke when posterior dropped below τ_revoke.
                let _ = specfence.try_revoke(&mv_memory.regions, *location, None);
                continue;
            }
            specfence.bayes.observe_speculate_ok_location(*location);
            specfence.metrics.record_bayes_success();
            let _ = specfence.try_revoke(&mv_memory.regions, *location, None);
        }
    }
    scheduler.finish_validation(tx_version, aborted)
}

/// Execute REVM transactions sequentially.
// Useful for falling back for (small) blocks with many dependencies.
// TODO: Use this for a long chain of sequential transactions even in parallel mode.
pub fn execute_revm_sequential<S: Storage + Debug, C: PevmChain>(
    chain: &C,
    storage: &S,
    spec_id: C::EvmSpecId,
    block_env: BlockEnv,
    txs: Vec<C::EvmTx>,
) -> PevmResult<C> {
    let db = CacheDB::new(StorageWrapper(storage));
    let is_eip_161_enabled = chain.is_eip_161_enabled(spec_id);
    let mut evm = chain.build_evm(spec_id, block_env, db);

    let mut results: Vec<PevmTxExecutionResult> = Vec::with_capacity(txs.len());
    let mut cumulative_gas_used: u64 = 0;
    for tx in txs {
        // TODO: More concrete error type
        let ResultAndState { result, state } = evm
            .transact(tx)
            .map_err(|err| ExecutionError::Custom(err.to_string()))?;

        evm.ctx().db_mut().commit(state.clone());

        let mut execution_result = PevmTxExecutionResult {
            receipt: receipt_from_revm(result),
            state: state_transitions_from_revm(is_eip_161_enabled, state).collect(),
        };

        cumulative_gas_used =
            cumulative_gas_used.saturating_add(execution_result.receipt.cumulative_gas_used);
        execution_result.receipt.cumulative_gas_used = cumulative_gas_used;

        results.push(execution_result);
    }
    Ok(results)
}
