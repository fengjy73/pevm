use std::time::Instant;
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
        AccountHints, AdaptiveEngagement, AdaptiveParams, BayesMap, ConcurrencyMode, DEFAULT_TAU,
        HotSet, FineGrainCollector, FineGrainSnapshot, HeatMap, InterBlockPrior, LiveLearner,
        LeanAbortRepair, MetricsInner, PartialRetryTable, RemCounters, ResearchAbortRepair, RwPriorMap, SpecDag,
        SpecFenceCtx, SpecFenceMetrics, WaveParkTable, seed_wait_regions, update_bayes,
        update_heat, update_rw_prior,
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
    rw_prior: RwPriorMap,
    /// R1: process-persistent HotSet (per-block members + multi-writer prior).
    hotset: HotSet,
    /// P1: inter-block morph / top-ℓ prior (warm-start only).
    inter_prior: InterBlockPrior,
    /// P1: tunable π constants (process-level).
    adaptive_params: AdaptiveParams,
    last_metrics: SpecFenceMetrics,
    last_initial_wait_accounts: std::collections::HashSet<alloy_primitives::Address>,
    /// M4: abort rate from the previous SpecFence block (`occ_aborts / n_tx`).
    last_abort_rate: f64,
    /// Lab-only fine-grain RW/abort tracer (off by default).
    finegrain_enabled: bool,
    finegrain: FineGrainCollector,
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
            rw_prior: RwPriorMap::new(),
            hotset: HotSet::new(),
            inter_prior: InterBlockPrior::new(),
            adaptive_params: AdaptiveParams::from_l3(),
            last_metrics: SpecFenceMetrics::default(),
            last_initial_wait_accounts: std::collections::HashSet::new(),
            last_abort_rate: 0.0,
            finegrain_enabled: false,
            finegrain: FineGrainCollector::new(),
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

    /// G6/AEC: replace process-level AdaptiveParams (learning rates / priors).
    pub(crate) fn set_adaptive_params(&mut self, params: AdaptiveParams) {
        self.adaptive_params = params;
    }

    /// Current AdaptiveParams (L3 defaults unless overridden).
    pub(crate) const fn adaptive_params(&self) -> &AdaptiveParams {
        &self.adaptive_params
    }

    /// Current concurrency-control mode.
    pub const fn concurrency_mode(&self) -> ConcurrencyMode {
        self.concurrency_mode
    }

    /// Metrics from the last parallel execution (OCC/PCC/`SpecFence`).
    pub const fn last_specfence_metrics(&self) -> &SpecFenceMetrics {
        &self.last_metrics
    }

    /// Enable/disable lab fine-grain RW + abort tracing for subsequent parallel blocks.
    pub fn set_finegrain_trace(&mut self, enabled: bool) {
        self.finegrain_enabled = enabled;
        if enabled {
            self.finegrain.clear();
        } else {
            self.finegrain.set_deep(false);
            self.finegrain.set_journal(false);
        }
    }

    /// Enable/disable deep effect-RAW instrumentation (implies finegrain_trace).
    /// Research flag only — production default remains off.
    pub fn set_finegrain_deep(&mut self, enabled: bool) {
        if enabled {
            self.finegrain_enabled = true;
            self.finegrain.clear();
            self.finegrain.set_deep(true);
        } else {
            self.finegrain.set_deep(false);
            self.finegrain.set_journal(false);
        }
    }

    /// Enable/disable interpreter/journal effect stream (implies deep).
    /// Research flag only — forces opt-in inspect_run for SLOAD/SSTORE logging;
    /// production default remains off (Handler::run, zero overhead).
    pub fn set_finegrain_journal(&mut self, enabled: bool) {
        if enabled {
            self.finegrain_enabled = true;
            self.finegrain.clear();
            self.finegrain.set_deep(true);
            self.finegrain.set_journal(true);
        } else {
            self.finegrain.set_journal(false);
        }
    }

    /// Take the fine-grain snapshot captured at the end of the last traced parallel block.
    pub fn take_finegrain_snapshot(&self) -> Option<FineGrainSnapshot> {
        self.finegrain.take_snapshot()
    }

    /// Accounts seeded in Wait at the start of the last parallel block.
    pub const fn last_initial_wait_accounts(
        &self,
    ) -> &std::collections::HashSet<alloy_primitives::Address> {
        &self.last_initial_wait_accounts
    }

    /// Clear inter-block heat and Bayesian posteriors (test / replay).
    /// Does **not** clear InterBlockPrior (morph EMA / flip α) — use
    /// [`reset_inter_prior`] for a full cold start.
    pub fn reset_heat(&mut self) {
        self.heat.reset();
        self.bayes.reset();
        self.rw_prior.reset();
        self.hotset.reset();
        self.last_initial_wait_accounts.clear();
        self.last_abort_rate = 0.0;
    }

    /// Clear dual-horizon inter-block morph / top-ℓ prior (lab cold start).
    pub fn reset_inter_prior(&mut self) {
        self.inter_prior.reset();
    }

    /// G7: InterBlockPrior flip-α events observed since last reset.
    pub fn inter_prior_flip_count(&self) -> usize {
        self.inter_prior.flip_count()
    }

    /// M3: number of locations with process-local write prior (diagnostics).
    pub fn rw_prior_hot_writes(&self) -> usize {
        self.rw_prior.hot_write_count()
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
        // V5-P0: LeanOCC default; HotSet feature-only; inter-prior never arms SoftWait.
        let learner = LiveLearner::new();
        if self.concurrency_mode == ConcurrencyMode::SpecFence {
            self.hotset.begin_block();
            let prior_morph = self.inter_prior.morph_ema();
            learner.begin_block_with_params(prior_morph, self.adaptive_params);
            // Warm-start HotSet/Bayes from inter-block top-ℓ — NEVER arm SoftWait from prior.
            for top in self.inter_prior.top_locations() {
                self.hotset.track_from_prior(top.location);
                // Mild Bayes seed so conflict_probability is location-aware without Wait arm.
                if top.abort_rate >= 0.15 || top.fanout_ema >= 16.0 {
                    let _ = self.bayes.observe_conflict_location(top.location);
                }
            }
        }
        let start_lean = self.concurrency_mode == ConcurrencyMode::SpecFence
            && AdaptiveEngagement::should_start_lean();
        // P0: only PCC seeds account Wait. SpecFence never seeds account Wait.
        if self.concurrency_mode == ConcurrencyMode::Pcc {
            seed_wait_regions(
                &mv_memory.regions,
                &hints,
                &self.bayes,
                self.concurrency_mode,
                block_env.beneficiary,
                DEFAULT_TAU,
                &mut initial_wait,
            );
        }

        self.execution_results.grow_to(block_size);

        let dag = SpecDag::new();
        let rem = RemCounters::default();
        let partial_retry = PartialRetryTable::new(block_size);
        let wave = WaveParkTable::new();
        let wave_ref = if self.concurrency_mode == ConcurrencyMode::SpecFence {
            Some(&wave)
        } else {
            None
        };
        let engagement = if self.concurrency_mode == ConcurrencyMode::SpecFence {
            AdaptiveEngagement::new(block_size, start_lean)
        } else {
            AdaptiveEngagement::disabled(block_size)
        };
        if self.finegrain_enabled {
            self.finegrain.clear();
            // Producer-readiness sampling for journal/deep research.
            self.finegrain.attach_runtime(&mv_memory, &scheduler);
        }
        let finegrain_ref = self.finegrain_enabled.then_some(&self.finegrain);
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
            wave: &wave,
            rw_prior: &self.rw_prior,
            engagement: &engagement,
            hotset: &self.hotset,
            learner: &learner,
            params: &self.adaptive_params,
            finegrain: finegrain_ref,
        };

        // TODO: Better thread handling
        thread::scope(|scope| {
            for _ in 0..concurrency_level.into() {
                scope.spawn(|| {
                    let mut vm = Vm::new(
                        chain, spec_id, &block_env, &txs, storage, &mv_memory, specfence,
                    );
                    let profile = crate::specfence::profile_timing_enabled();
                    let mut sched_t0 = profile.then(Instant::now);
                    let mut task = scheduler.next_task_with_wave(wave_ref);
                    if let Some(t0) = sched_t0 {
                        metrics_inner
                            .add_profile_scheduler_ns(t0.elapsed().as_nanos() as u64);
                    }
                    while task.is_some() {
                        task = match task.unwrap() {
                            Task::Execution(tx_version) => {
                                let fence_ref = if self.concurrency_mode == ConcurrencyMode::SpecFence {
                                    Some(&dag)
                                } else {
                                    None
                                };
                                self.try_execute(&mut vm, &scheduler, tx_version, wave_ref, fence_ref)
                            }
                            Task::Validation(tx_version) => {
                                if profile {
                                    let v0 = Instant::now();
                                    let next = try_validate(
                                        &mv_memory, &scheduler, &tx_version, specfence,
                                    );
                                    metrics_inner
                                        .add_profile_validate_ns(v0.elapsed().as_nanos() as u64);
                                    next
                                } else {
                                    try_validate(&mv_memory, &scheduler, &tx_version, specfence)
                                }
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
                            sched_t0 = profile.then(Instant::now);
                            task = scheduler.next_task_with_wave(wave_ref);
                            if let Some(t0) = sched_t0 {
                                metrics_inner
                                    .add_profile_scheduler_ns(t0.elapsed().as_nanos() as u64);
                            }
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
                update_rw_prior(&self.rw_prior);
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
        metrics_inner.set_wave_metrics(
            wave.wait_park_count(),
            wave.wait_park_ns(),
            wave.ready_steal_on_wait(),
        );
        metrics_inner.set_park_subtype_metrics(
            wave.park_count_softwait(),
            wave.park_ns_softwait(),
            wave.park_count_early_abort(),
            wave.park_ns_early_abort(),
            wave.park_count_blocking_other(),
            wave.park_ns_blocking_other(),
        );
        // AEC: best-effort steal/idle / park duration proxies → learner (∉ TCB).
        if self.concurrency_mode == ConcurrencyMode::SpecFence {
            let steals = wave.ready_steal_on_wait();
            let park_ns = wave.wait_park_ns();
            if steals > 0 || park_ns > 0 {
                learner.note_steal_or_park_proxy(steals > 0, park_ns);
                // Count additional steals coarsely (one event already recorded).
                for _ in 1..steals {
                    learner.note_steal_or_park_proxy(true, 0);
                }
            }
        }
        metrics_inner.set_park_resume_metrics(
            wave.park_resume_at_k(),
            wave.park_resume_full_retry(),
        );
        if self.concurrency_mode == ConcurrencyMode::SpecFence {
            self.hotset.end_block();
            // P1: pack InterBlockPrior from live morph hat + top-ℓ (flip → higher α).
            let morph_hat = learner.morph_hat();
            let top = learner.pack_top_locations();
            let _alpha = self.inter_prior.end_block(morph_hat, top);
            metrics_inner.set_soft_wait_arms(dag.soft_arm_count());
        }
        metrics_inner.set_engagement_metrics(
            engagement.lean_mode_txs(),
            engagement.full_mode_txs(),
            engagement.engagement_switches(),
            self.hotset.hot_local_reads(),
            self.hotset.len(),
        );
        self.last_metrics = metrics_inner.snapshot(
            wave_id,
            mean_wait,
            mean_p_at_wait,
            mean_p_at_spec,
            wave.wave_width_mean(),
        );
        if self.concurrency_mode == ConcurrencyMode::SpecFence {
            self.last_abort_rate =
                self.last_metrics.occ_aborts as f64 / (block_size as f64).max(1.0);
        }
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

        if self.finegrain_enabled {
            self.finegrain
                .capture(&mv_memory, &scheduler, block_env.beneficiary);
            self.finegrain.detach_runtime();
        }
        self.dropper.drop((mv_memory, scheduler));

        Ok(fully_evaluated_results)
    }

    fn try_execute<'a, S: Storage, C: PevmChain>(
        &self,
        vm: &mut Vm<'a, S, C>,
        scheduler: &Scheduler,
        tx_version: TxVersion,
        wave: Option<&WaveParkTable>,
        fence: Option<&crate::specfence::FenceGraph>,
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
            // P4: SoftWait wake may have restored a (t,k) resume intent — arm RewindTo/FF
            // or FullRetry while the parked incarnation journal is still intact.
            if let Some(wave) = wave {
                vm.try_apply_park_resume(tx_version.tx_idx, wave);
            }
            return match vm.execute(&tx_version, result_slot) {
                Ok(flags) => {
                    // PublishWrite ≈ incarnation finished: wake location waiters + ready.
                    let task = scheduler
                        .finish_execution_with_wave_fence(tx_version, flags, wave, fence);
                    // G4: SoftWait wake → wait_useful learner credit.
                    if let Some(fence) = fence {
                        vm.credit_softwait_wakes(fence);
                    }
                    task
                }
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
                    // M2/P4: WaitHard registered park+(location,k) in Vm (SpecFence).
                    // add_dependency marks Aborting; worker must steal ready work.
                    let pending = vm.take_pending_park();
                    let park_loc = pending.map(|p| p.location).unwrap_or(0);
                    let park_k = pending.map(|p| p.armed_at_k).unwrap_or(0);
                    let park_kind = pending
                        .map(|p| p.kind)
                        .unwrap_or(crate::specfence::ParkKind::BlockingOther);
                    // Dependency first (Block-STM). SoftWait/EarlyAbort still need wave
                    // park for (t,k) resume; BlockingOther ESTIMATE may convert to
                    // steal-without-long-park when the writer is Ready.
                    if !scheduler.add_dependency(tx_version.tx_idx, blocking_tx_idx)
                        && self.abort_reason.get().is_none()
                    {
                        // Writer already done — retry without parking.
                        continue;
                    }
                    if let Some(wave) = wave {
                        if park_kind == crate::specfence::ParkKind::BlockingOther {
                            wave.arm_steal_convert_without_park();
                            if let Some(stolen) = scheduler
                                .next_task_steal_after_park_prefer(wave, Some(blocking_tx_idx))
                            {
                                return Some(stolen);
                            }
                        }
                        wave.park_with_kind(
                            tx_version.tx_idx,
                            blocking_tx_idx,
                            park_loc,
                            park_k,
                            park_kind,
                        );
                        if let Some(stolen) = scheduler
                            .next_task_steal_after_park_prefer(wave, Some(blocking_tx_idx))
                        {
                            return Some(stolen);
                        }
                    }
                    // Worker-free Wait: return None → next_task_with_wave steals.
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
    // OCC-like first pass: one read-set walk. Defer read_locations until fail
    // (avoids a second last_locations lock on the common success path).
    let invalid = if specfence.mode.uses_regions() {
        mv_memory.collect_invalid_reads(tx_version.tx_idx)
    } else {
        Vec::new()
    };
    let mut read_set_valid = if specfence.mode.uses_regions() {
        invalid.is_empty()
    } else {
        mv_memory.validate_read_locations(tx_version.tx_idx)
    };
    let lean_tx = specfence.mode == ConcurrencyMode::SpecFence
        && specfence.engagement.tx_was_lean(tx_version.tx_idx);
    // Carried into SuffixRepair to avoid re-plan / re-lock write set.
    let mut cached_read_locations: Option<Vec<crate::MemoryLocationHash>> = None;
    let mut cached_write_locations: Option<Vec<crate::MemoryLocationHash>> = None;
    let mut cached_plan: Option<Option<crate::specfence::PartialRetryPlan>> = None;
    if specfence.mode == ConcurrencyMode::SpecFence && !invalid.is_empty() {
        specfence
            .metrics
            .record_region_validate_fail(invalid.len());
        // RebindOnly-first (native resolve): patch origins when invalid reads now
        // have Data/Storage and there is no *true* failed-suffix write (first_k ≥
        // k_fail). Uncertified writes before k_fail must not block RebindOnly.
        // One opportunity tick per validate fail (not O(|reads|) rem atomics).
        specfence.rem.note_checkpoint_opportunity();
        specfence.metrics.record_checkpoint_opportunity();
        let write_locations = mv_memory.write_locations(tx_version.tx_idx);
        let read_locations = mv_memory.read_locations(tx_version.tx_idx);
        let mut k_fail = invalid
            .iter()
            .filter_map(|l| specfence.partial_retry.first_k(tx_version.tx_idx, *l))
            .min();
        let mut plan = None;
        if k_fail.is_none() {
            plan = specfence.partial_retry.plan_partial_retry(
                tx_version.tx_idx,
                &read_locations,
                &invalid,
                &write_locations,
            );
            if let Some(ref p) = plan {
                k_fail = Some(p.k_fail);
            }
        }
        // RebindOnly when no true failed-suffix write. Unknown k_fail + any
        // write: conservative true_suffix (avoid unsafe in-place patch).
        // Value-stable still widens Estimate→Data / incarnation same-output.
        let true_suffix = match k_fail {
            Some(k) => specfence.partial_retry.has_true_suffix_writes(
                tx_version.tx_idx,
                k,
                &write_locations,
            ),
            None => !write_locations.is_empty(),
        };
        // Value-stable RebindOnly: same-output republish (Estimate→Data / incarnation
        // bump) is safe without reexec. Snap match first; else prior-origin MV value
        // vs current Data (covers lean paths that skipped snaps). try_rebind refuses
        // Estimate / multi-origin.
        let estimate_cleared = !invalid.is_empty()
            && invalid.iter().all(|&loc| {
                mv_memory
                    .current_data_value(tx_version.tx_idx, loc)
                    .is_some()
            });
        let value_stable = estimate_cleared
            && invalid.iter().all(|&loc| {
                let cur = match mv_memory.current_data_value(tx_version.tx_idx, loc) {
                    Some(c) => c,
                    None => return false,
                };
                if specfence
                    .partial_retry
                    .value_stable_match(tx_version.tx_idx, loc, &cur)
                {
                    return true;
                }
                // Fallback: prior origin's published Basic/Storage == current.
                mv_memory.prior_read_value_stable(tx_version.tx_idx, loc)
            });
        // Prefer RebindOnly when !true_suffix, or when Estimate cleared + value-stable.
        // Value-stable path allows multi-origin (lazy) → single current Data.
        let rebound = if value_stable {
            mv_memory.try_rebind_invalid_reads_value_stable(tx_version.tx_idx, &invalid)
        } else if !true_suffix {
            mv_memory.try_rebind_invalid_reads(tx_version.tx_idx, &invalid)
        } else {
            false
        };
        if rebound {
            specfence.metrics.record_partial_retry();
            specfence.metrics.record_rebind_only();
            specfence.partial_retry.clear_force_bind(tx_version.tx_idx);
            specfence.partial_retry.clear_repair(tx_version.tx_idx);
            specfence
                .partial_retry
                .clear_suffix_repair_depth(tx_version.tx_idx);
            specfence.learner.note_reexec_cost(0.1);
            read_set_valid = true;
            // Fall through to success path below (no abort / no SuffixRepair).
        } else {
            // Ensure one plan for SuffixRepair (k_fail may have come from first_k only).
            if plan.is_none() {
                plan = specfence.partial_retry.plan_partial_retry(
                    tx_version.tx_idx,
                    &read_locations,
                    &invalid,
                    &write_locations,
                );
            }
            cached_read_locations = Some(read_locations);
            cached_write_locations = Some(write_locations);
            cached_plan = Some(plan);
        }
    }

    let aborted = !read_set_valid && scheduler.try_validation_abort(tx_version);
    if aborted {
        // SpecFence-native Lean resolve: SuffixRepair-first (RewindTo+FF when
        // checkpoint exists). Research inspect (`!lean_tx`) keeps separate plant API.
        if lean_tx {
            // Dig: abort while prior ForceBind / SoftWait-wake still armed.
            let prior_force_bind = specfence
                .partial_retry
                .force_bind_locations(tx_version.tx_idx);
            let was_force_bind = !prior_force_bind.is_empty();
            if was_force_bind {
                specfence.metrics.record_force_bind_reabort();
            }
            if specfence
                .partial_retry
                .take_post_softwait_wake(tx_version.tx_idx)
            {
                specfence.metrics.record_soft_wait_wake_reabort();
            }
            let write_locations = cached_write_locations
                .unwrap_or_else(|| mv_memory.write_locations(tx_version.tx_idx));
            let read_locations = cached_read_locations
                .unwrap_or_else(|| mv_memory.read_locations(tx_version.tx_idx));
            // Break force_bind_reabort ≈ resume loop:
            //  (1) escalate after 1 reabort → clear force_bind + OCC FullRestart
            //  (3) cap SuffixRepair depth (≥2) → same escalate
            // RebindOnly already preferred above when it can apply.
            let repair_depth = specfence
                .partial_retry
                .suffix_repair_depth(tx_version.tx_idx);
            let escalate = was_force_bind || repair_depth >= 2;
            let repair = if escalate {
                // Drop sticky force_bind for this incarnation; no RewindTo.
                specfence.partial_retry.escalate_full_restart(tx_version.tx_idx)
            } else {
                // Reuse plan when we already built it for k_fail; else plan once.
                let plan = cached_plan.unwrap_or_else(|| {
                    specfence.partial_retry.plan_partial_retry(
                        tx_version.tx_idx,
                        &read_locations,
                        &invalid,
                        &write_locations,
                    )
                });
                let repair = specfence
                    .partial_retry
                    .apply_suffix_repair_planned(tx_version.tx_idx, plan);
                if repair.did_force_bind() {
                    specfence.partial_retry.note_suffix_repair(tx_version.tx_idx);
                }
                repair
            };
            if repair.did_force_bind() {
                specfence.metrics.record_partial_retry();
            }
            // Sticky AFTER SuffixRepair only when not escalating (reabort escalates).
            if was_force_bind && !escalate {
                for location in &invalid {
                    specfence.learner.note_sticky_resolve(*location);
                }
                specfence
                    .partial_retry
                    .extend_force_bind(tx_version.tx_idx, &invalid);
                specfence
                    .partial_retry
                    .mark_needs_live_capture(tx_version.tx_idx);
            }
            specfence.learner.note_reexec_cost(repair.reexec_cost());
            // SuffixRepair → invalidate failed suffix only; else selective/full.
            let fence_locs: Vec<_> = match &repair {
                LeanAbortRepair::SuffixRepair { suffix_writes, .. } => {
                    specfence.metrics.record_rewind_to_cp();
                    let estimated = mv_memory
                        .invalidate_partial_suffix(tx_version.tx_idx, suffix_writes);
                    if !estimated.is_empty() {
                        specfence
                            .metrics
                            .record_selective_invalidate(estimated.len());
                    }
                    if estimated.is_empty() {
                        if suffix_writes.is_empty() {
                            write_locations.clone()
                        } else {
                            suffix_writes.clone()
                        }
                    } else {
                        estimated
                    }
                }
                LeanAbortRepair::ForceBind { suffix_writes, .. } => {
                    // Certified prefix without mid-tx cp: ESTIMATE failed suffix only.
                    // Never invalidate_selective here — aborted stamp + ESTIMATE would
                    // poison ForceBound prefix Data and drive BlockingOther parks.
                    let estimated = mv_memory
                        .invalidate_partial_suffix(tx_version.tx_idx, suffix_writes);
                    if !estimated.is_empty() {
                        specfence
                            .metrics
                            .record_selective_invalidate(estimated.len());
                    }
                    if estimated.is_empty() {
                        if suffix_writes.is_empty() {
                            write_locations.clone()
                        } else {
                            suffix_writes.clone()
                        }
                    } else {
                        estimated
                    }
                }
                LeanAbortRepair::FullRestart { .. } => {
                    // Escalate (fb_reabort / depth cap): OCC-style full invalidate —
                    // do not protect prior force_bind prefix (sticky fb dropped).
                    // Non-escalate FullRestart still protects armed force_bind Data.
                    let protect = if escalate {
                        Vec::new()
                    } else {
                        let mut protect = prior_force_bind.clone();
                        for loc in specfence
                            .partial_retry
                            .force_bind_locations(tx_version.tx_idx)
                        {
                            if !protect.contains(&loc) {
                                protect.push(loc);
                            }
                        }
                        protect
                    };
                    let fence_locs = if !protect.is_empty() {
                        let suffix: Vec<_> = write_locations
                            .iter()
                            .copied()
                            .filter(|l| !protect.contains(l))
                            .collect();
                        let estimated = mv_memory
                            .invalidate_partial_suffix(tx_version.tx_idx, &suffix);
                        if !estimated.is_empty() {
                            specfence
                                .metrics
                                .record_selective_invalidate(estimated.len());
                        }
                        if estimated.is_empty() {
                            if suffix.is_empty() {
                                write_locations.clone()
                            } else {
                                suffix
                            }
                        } else {
                            estimated
                        }
                    } else {
                        let (estimated, fallback) = mv_memory.invalidate_selective(
                            tx_version.tx_idx,
                            Some(tx_version.tx_incarnation),
                        );
                        if fallback {
                            specfence.metrics.record_selective_fallback_full();
                        } else if !estimated.is_empty() {
                            specfence
                                .metrics
                                .record_selective_invalidate(estimated.len());
                        }
                        write_locations.clone()
                    };
                    specfence.metrics.record_full_restart();
                    fence_locs
                }
            };
            specfence.metrics.record_occ_abort();
            specfence.rw_prior.observe_write_set(&write_locations, None);
            for location in &invalid {
                specfence.bayes.observe_conflict_location_always(*location);
                specfence.metrics.record_bayes_conflict();
                specfence.rw_prior.observe_co_access(*location);
                specfence.hotset.note_abort(*location);
                specfence.promote_from_bayes(&mv_memory.regions, *location, None);
            }
            let cascade_hint = invalid.len().max(1);
            for location in &invalid {
                specfence.learner.note_abort(*location, cascade_hint);
            }
            let rewind_to =
                mv_memory.min_higher_reader_of(tx_version.tx_idx, &fence_locs);
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
            // V5-P0: engagement.note_abort is metrics-only (no HotSet storm insert).
            let _ = specfence.engagement.note_abort();
            return scheduler.finish_validation_fenced(tx_version, true, rewind_to, Some(specfence.wave));
        }
        // Snapshot write locations before invalidate (same set).
        let write_locations = if specfence.mode == ConcurrencyMode::SpecFence {
            mv_memory.write_locations(tx_version.tx_idx)
        } else {
            Vec::new()
        };
        if specfence.mode == ConcurrencyMode::SpecFence {
            // Research-inspect abort path (`SPECFENCE_ENABLE_INSPECT`): RewindTo+FF
            // when certified prefix + checkpoint; else same FullRestart as Lean.
            if specfence.partial_retry.has_force_bind(tx_version.tx_idx) {
                specfence.metrics.record_force_bind_reabort();
                for location in &invalid {
                    specfence.learner.note_sticky_resolve(*location);
                }
                specfence
                    .partial_retry
                    .extend_force_bind(tx_version.tx_idx, &invalid);
                specfence
                    .partial_retry
                    .mark_needs_live_capture(tx_version.tx_idx);
            }
            if specfence
                .partial_retry
                .take_post_softwait_wake(tx_version.tx_idx)
            {
                specfence.metrics.record_soft_wait_wake_reabort();
            }
            specfence.metrics.record_occ_abort();
            // V5-P0: engagement.note_abort is metrics-only (no HotSet storm insert).
            let _ = specfence.engagement.note_abort();
            let _ = specfence
                .partial_retry
                .disable_jump_after_failed_resume(tx_version.tx_idx);
            // M3: learn WŜ from aborted incarnation + first-pass miss metrics.
            specfence
                .rw_prior
                .observe_write_set(&write_locations, None);
            let mut first_pass = 0usize;
            let cascade_hint = invalid.len().max(1);
            for location in &invalid {
                specfence.bayes.observe_conflict_location_always(*location);
                specfence.metrics.record_bayes_conflict();
                specfence.rw_prior.observe_co_access(*location);
                specfence.hotset.note_abort(*location);
                specfence.learner.note_abort(*location, cascade_hint);
                if specfence.rw_prior.predicts_write(*location)
                    || mv_memory.residual_writer_before(*location, tx_version.tx_idx).is_some()
                {
                    first_pass += 1;
                    specfence.metrics.record_prior_bind_miss();
                }
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
            if first_pass > 0 {
                specfence
                    .metrics
                    .record_first_pass_validate_fail(first_pass);
            }

            // Research plant API (opt-in inspect only) — separate from Lean
            // `apply_suffix_repair`. Absolute jump stays inspect-gated.
            let read_locations = cached_read_locations
                .unwrap_or_else(|| mv_memory.read_locations(tx_version.tx_idx));
            let fence_locs = match specfence.partial_retry.research_apply_abort_repair(
                tx_version.tx_idx,
                &read_locations,
                &invalid,
                &write_locations,
            ) {
                ResearchAbortRepair::RewindTo {
                    certified: _,
                    suffix_writes,
                    reexec_cost,
                } => {
                    specfence.metrics.record_partial_retry();
                    specfence.metrics.record_rewind_to_cp();
                    specfence.learner.note_reexec_cost(reexec_cost);
                    let estimated = mv_memory
                        .invalidate_partial_suffix(tx_version.tx_idx, &suffix_writes);
                    if !estimated.is_empty() {
                        specfence
                            .metrics
                            .record_selective_invalidate(estimated.len());
                    }
                    if estimated.is_empty() {
                        suffix_writes
                    } else {
                        estimated
                    }
                }
                ResearchAbortRepair::FullRestart { .. } => {
                    research_full_restart_invalidate(
                        &specfence,
                        mv_memory,
                        tx_version,
                        &write_locations,
                    )
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
            return scheduler.finish_validation_fenced(tx_version, true, rewind_to, Some(specfence.wave));
        }
        // OCC / PCC: full write-set ESTIMATE (unchanged).
        let occ_write_locs = mv_memory.write_locations(tx_version.tx_idx);
        mv_memory.convert_writes_to_estimates(tx_version.tx_idx);
        specfence.metrics.record_occ_abort();
        // OCC/PCC abort always restarts interpreter from tx head on next incarnation.
        specfence.metrics.record_full_restart();
        if let Some(fg) = specfence.finegrain {
            let cascade = scheduler
                .block_size()
                .saturating_sub(tx_version.tx_idx.saturating_add(1));
            fg.record_abort(
                tx_version.tx_idx,
                tx_version.tx_incarnation,
                occ_write_locs.len(),
                cascade,
            );
        }
        if specfence.mode.uses_regions() {
            for location in &invalid {
                if mv_memory.regions.promote_location(*location) {
                    specfence.metrics.record_promotion(None);
                }
            }
        }
    } else if !aborted && specfence.mode == ConcurrencyMode::SpecFence && read_set_valid {
        // Dig: SoftWait wake → validate-ok.
        if specfence
            .partial_retry
            .take_post_softwait_wake(tx_version.tx_idx)
        {
            specfence.metrics.record_soft_wait_wake_ok();
        }
        // Successful validation clears PartialRetry / RewindTo state for this tx.
        specfence
            .partial_retry
            .clear_force_bind(tx_version.tx_idx);
        specfence.partial_retry.clear_repair(tx_version.tx_idx);
        specfence
            .partial_retry
            .clear_suffix_repair_depth(tx_version.tx_idx);
        specfence
            .partial_retry
            .clear_jump_disabled(tx_version.tx_idx);
        // M3: fold completed WŜ into process prior (inter-block Bind-before-touch).
        // Block-local residual remains abort/ESTIMATE-driven (Bohm-lite); publishing
        // successful WS into residual caused WaitHard storms / M2 hangs on ERC-20.
        let writes = mv_memory.write_locations(tx_version.tx_idx);
        specfence.rw_prior.observe_write_set(&writes, None);
        // Successful SpecRead validation: O(1) opportunity + revoke sticky Waits only.
        // Skip O(|reads|) bayes success storm — SoftWait scarce under Bind-no-park;
        // abort-path bayes/hotset still learn conflicts.
        specfence.rem.note_checkpoint_opportunity();
        let read_locations = mv_memory.read_locations(tx_version.tx_idx);
        for location in &read_locations {
            if *location
                == hash_deterministic(MemoryLocation::Basic(specfence.beneficiary))
            {
                continue;
            }
            if mv_memory.regions.location_mode(*location) == crate::specfence::RegionMode::Wait {
                let _ = specfence.try_revoke(&mv_memory.regions, *location, None);
            }
        }
    }
    scheduler.finish_validation(tx_version, aborted)
}


/// Research-inspect FullRestart arm (shared by duplicate match arms).
/// Clears force-bind/repair, selective-invalidates, records FullRestart metrics.
fn research_full_restart_invalidate(
    specfence: &SpecFenceCtx<'_>,
    mv_memory: &MvMemory,
    tx_version: &TxVersion,
    write_locations: &[crate::MemoryLocationHash],
) -> Vec<crate::MemoryLocationHash> {
    specfence.metrics.record_tx_full_retry();
    specfence.metrics.record_full_restart();
    specfence.metrics.record_partial_retry_fallback_full();
    specfence.learner.note_reexec_cost(2.0);
    specfence.partial_retry.clear_force_bind(tx_version.tx_idx);
    specfence.partial_retry.clear_repair(tx_version.tx_idx);
    let (estimated, fallback) =
        mv_memory.invalidate_selective(tx_version.tx_idx, Some(tx_version.tx_incarnation));
    if fallback {
        specfence.metrics.record_selective_fallback_full();
    } else {
        specfence
            .metrics
            .record_selective_invalidate(estimated.len().max(1));
    }
    if estimated.is_empty() {
        write_locations.to_vec()
    } else {
        estimated
    }
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
