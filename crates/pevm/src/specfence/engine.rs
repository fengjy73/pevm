//! `SpecFence` block driver.
//!
//! Workers share Chase-Lev deques seeded by transaction index. A transaction
//! commits only as the next prefix entry, and only after its read set matches
//! the latest write in both identity and value. The block returns when
//! `committed_upto == n` and that check has been repeated for every read set.

use std::cell::UnsafeCell;
use std::num::NonZeroUsize;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use alloy_primitives::{Address, TxKind, U256};
use hashbrown::HashMap;
use revm::context::{BlockEnv, Transaction as _, TxEnv};
use rustc_hash::FxBuildHasher;

use crate::{
    EvmAccount, MemoryEntry, MemoryLocation, MemoryValue, Storage, TxIdx,
    chain::PevmChain,
    hash_deterministic,
    vm::{ExecutionError, PevmTxExecutionResult},
};
use crate::{PevmError, PevmResult};

use super::live_chain::{ClassGroup, ClassKeyKind, LiveChain};
use super::mv::SfMv;
use super::rt::Runtime;
use super::trace::{SfTrace, Trace};
use super::vm::{SfVm, Step};

/// Which class key the in-block predictor uses.
pub use super::live_chain::ClassKeyKind as SfClassKey;

/// Knobs for one `SpecFence` block. Chain budget and wait threshold are not
/// taken from here; those move inside safety bounds while the block runs.
#[derive(Debug, Clone)]
pub struct SfOptions {
    /// Worker count.
    pub concurrency: NonZeroUsize,
    /// Class key. Measured alternatives are `(to, selector)` and `(code_hash, selector)`.
    pub class_key: SfClassKey,
    /// Cross-block writer lists. Installed as radar only; fresh runs pass none.
    pub radar: Vec<(u64, Vec<TxIdx>)>,
}

impl SfOptions {
    /// `(to, selector)` classes and no cross-block radar.
    pub const fn fresh(concurrency: NonZeroUsize, class_key: SfClassKey) -> Self {
        Self {
            concurrency,
            class_key,
            radar: Vec::new(),
        }
    }
}

static LAST_TRACE: Mutex<Option<SfTrace>> = Mutex::new(None);

/// Trace of the most recent [`run_sf_block`] in this process.
pub fn last_trace() -> Option<SfTrace> {
    LAST_TRACE.lock().unwrap().clone()
}

struct Results(Vec<UnsafeCell<Option<PevmTxExecutionResult>>>);
unsafe impl Sync for Results {}

impl Results {
    fn new(n: usize) -> Self {
        Self((0..n).map(|_| UnsafeCell::new(None)).collect())
    }

    fn slot(&self, tx: usize) -> *mut Option<PevmTxExecutionResult> {
        unsafe { self.0.get_unchecked(tx).get() }
    }

    fn take(&self, tx: usize) -> PevmTxExecutionResult {
        unsafe { (*self.0.get_unchecked(tx).get()).take().unwrap() }
    }
}

enum AbortKind {
    Fallback,
    Fatal(ExecutionError),
}

/// Execute one block with `SpecFence`.
///
/// Upstream [`crate::Pevm::execute_revm_parallel`] is not used and is not modified.
pub fn run_sf_block<S, C>(
    chain: &C,
    storage: &S,
    spec_id: C::EvmSpecId,
    block_env: BlockEnv,
    txs: Vec<C::EvmTx>,
    options: SfOptions,
) -> PevmResult<C>
where
    C: PevmChain + Send + Sync,
    S: Storage + Send + Sync + std::fmt::Debug,
{
    if txs.is_empty() {
        *LAST_TRACE.lock().unwrap() = None;
        return Ok(Vec::new());
    }

    let n = txs.len();
    let workers = usize::from(options.concurrency).max(1).min(n.max(1));
    let template = chain.build_mv_memory(&block_env, &txs);
    let estimates: Vec<_> = template
        .data
        .iter()
        .map(|entry| {
            (
                *entry.key(),
                entry.value().keys().copied().collect::<Vec<_>>(),
            )
        })
        .collect();
    let lazy: Vec<_> = template.consume_lazy_addresses().into_iter().collect();
    drop(template);

    let mv = SfMv::new(n, estimates, lazy);
    let mut live = LiveChain::new(n, options.class_key);
    live.skip_location(hash_deterministic(MemoryLocation::Basic(
        block_env.beneficiary,
    )));
    let (groups, class_of) = build_classes(chain, storage, &txs, options.class_key);
    live.install_classes(groups, class_of);
    for (location, writers) in &options.radar {
        live.install_radar(*location, writers);
    }
    let live = live;
    let rt = Runtime::new(n, workers);
    rt.seed();
    let prev_sender = previous_senders(chain, &txs);
    // Radar chain heads are seeded by pushing the lowest radar member later;
    // with an empty radar this is a no-op. Index seeding already ran.
    let trace = Trace::new(n);
    let results = Results::new(n);
    let abort: OnceLock<AbortKind> = OnceLock::new();
    let commit_mu = Mutex::new(());
    let started = Instant::now();
    let deadline = Duration::from_secs(120);

    thread::scope(|scope| {
        for worker in 0..workers {
            let mv = &mv;
            let live = &live;
            let rt = &rt;
            let trace = &trace;
            let results = &results;
            let abort = &abort;
            let commit_mu = &commit_mu;
            let txs = &txs;
            let block_env = &block_env;
            let prev_sender = prev_sender.as_slice();
            scope.spawn(move || {
                let mut vm = SfVm::new(
                    chain,
                    spec_id,
                    block_env,
                    txs,
                    storage,
                    mv,
                    live,
                    rt,
                    trace,
                    prev_sender,
                );
                let mut idle = 0u32;
                while rt.committed() < n && !rt.aborted() {
                    if started.elapsed() > deadline {
                        rt.request_abort();
                        break;
                    }
                    try_commit(worker, n, rt, mv, live, trace, commit_mu);
                    if let Some((tx, inc)) = rt.pop(worker) {
                        idle = 0;
                        live.note_idle(false);
                        if let Some(pred) = live.admission_predecessor(tx)
                            && !rt.is_executing(pred)
                            && !rt.is_committed(pred)
                            && !rt.is_executed(pred)
                        {
                            // Not admitted: the predecessor has not started.
                            // Tier A still starts the tx once the predecessor is executing;
                            // that case falls through because `is_executing` is true.
                            rt.park(worker, tx, pred, false);
                            continue;
                        }
                        rt.executing_add(1);
                        let incarnation = inc;
                        let mut guard = 0;
                        let step = loop {
                            guard += 1;
                            if guard > 8 {
                                break Step::Yield;
                            }
                            match vm.execute(tx, incarnation, unsafe { &mut *results.slot(tx) }) {
                                Step::Retry => continue,
                                other => break other,
                            }
                        };
                        rt.executing_add(-1);
                        match step {
                            Step::Done => rt.finish_ok(worker, tx),
                            Step::Yield => rt.defer(tx),
                            Step::Block(pred) => {
                                if pred >= n || rt.is_committed(pred) || rt.is_executed(pred) {
                                    rt.defer(tx);
                                } else {
                                    rt.park(worker, tx, pred, false);
                                }
                            }
                            Step::Retry => unreachable!("retry is consumed above"),
                            Step::Fallback => {
                                rt.request_abort();
                                let _ = abort.set(AbortKind::Fallback);
                            }
                            Step::Fatal(err) => {
                                rt.request_abort();
                                let _ = abort.set(AbortKind::Fatal(err));
                            }
                        }
                        try_commit(worker, n, rt, mv, live, trace, commit_mu);
                    } else {
                        idle += 1;
                        live.note_idle(true);
                        try_commit(worker, n, rt, mv, live, trace, commit_mu);
                        if rt.committed() == n {
                            break;
                        }
                        if !rt.rescue(worker) {
                            if idle > 8 {
                                rt.wait_brief();
                                idle = 0;
                            } else {
                                thread::yield_now();
                            }
                        }
                    }
                }
            });
        }
    });

    if let Some(kind) = abort.get() {
        return match kind {
            AbortKind::Fallback => {
                crate::execute_revm_sequential(chain, storage, spec_id, block_env, txs)
            }
            AbortKind::Fatal(err) => Err(PevmError::ExecutionError(err.clone())),
        };
    }
    if started.elapsed() > deadline || rt.committed() != n {
        eprintln!(
            "specfence exit before prefix: committed={} n={n}",
            rt.committed()
        );
        return Err(PevmError::UnreachableError);
    }

    for tx in 0..n {
        if let Some(loc) = mv.failing_location(tx) {
            eprintln!("specfence final read set mismatch tx={tx} loc={loc}");
            return Err(PevmError::UnreachableError);
        }
        mv.collect_edges(tx, &trace);
    }

    let mut fully = Vec::with_capacity(n);
    let mut cumulative_gas_used: u64 = 0;
    for tx_idx in 0..n {
        let mut execution_result = results.take(tx_idx);
        cumulative_gas_used =
            cumulative_gas_used.saturating_add(execution_result.receipt.cumulative_gas_used);
        execution_result.receipt.cumulative_gas_used = cumulative_gas_used;
        fully.push(execution_result);
    }

    evaluate_lazy(chain, storage, spec_id, &txs, &mv, &mut fully)?;

    let snap = trace.snapshot(
        live.max_chain_len(),
        live.armed_locations(),
        options.class_key.as_str(),
    );
    *LAST_TRACE.lock().unwrap() = Some(snap);
    Ok(fully)
}

fn try_commit(
    worker: usize,
    n: usize,
    rt: &Runtime,
    mv: &SfMv,
    live: &LiveChain,
    trace: &Trace,
    commit_mu: &Mutex<()>,
) {
    let Ok(_guard) = commit_mu.try_lock() else {
        return;
    };
    loop {
        let i = rt.committed();
        if i >= n {
            return;
        }
        if !rt.is_executed(i) {
            return;
        }
        let inc = rt.incarnation(i);
        if let Some(loc) = mv.failing_location(i) {
            let armed = live.is_armed(loc);
            let writes = mv.write_locations(i);
            mv.convert_writes_to_estimates(i);
            live.arm_failure(loc);
            backfill(mv, live, loc);
            live.on_abort_residual(i, inc.saturating_add(1), &writes);
            trace.note_full_replay(armed);
            live.note_finished(true);
            rt.requeue_abort(worker, i);
            return;
        }
        if rt.try_mark_committed(i, inc) {
            let writes = mv.write_locations(i);
            live.hole_clear(i, &writes);
            live.note_finished(false);
            rt.notify();
        } else {
            return;
        }
    }
}

fn backfill(mv: &SfMv, live: &LiveChain, location: u64) {
    let Some(written) = mv.data.get(&location) else {
        return;
    };
    for (tx, entry) in written.iter() {
        if let MemoryEntry::Data(inc, _) = entry {
            live.note_backfill_writer(location, *tx, *inc);
        }
    }
}

fn previous_senders<C: PevmChain>(chain: &C, txs: &[C::EvmTx]) -> Vec<Option<TxIdx>> {
    let mut last: HashMap<Address, TxIdx, FxBuildHasher> = HashMap::with_hasher(FxBuildHasher);
    let mut out = Vec::with_capacity(txs.len());
    for (i, tx) in txs.iter().enumerate() {
        let caller = chain.tx_env(tx).caller;
        out.push(last.insert(caller, i));
    }
    out
}

fn build_classes<S: Storage, C: PevmChain>(
    chain: &C,
    storage: &S,
    txs: &[C::EvmTx],
    kind: ClassKeyKind,
) -> (Vec<ClassGroup>, Vec<u16>) {
    let mut groups: HashMap<u64, Vec<TxIdx>, FxBuildHasher> = HashMap::with_hasher(FxBuildHasher);
    let mut keys = Vec::with_capacity(txs.len());
    for tx in txs {
        let env: &TxEnv = chain.tx_env(tx);
        let to = match env.kind {
            TxKind::Call(to) => Some(to),
            TxKind::Create => None,
        };
        let mut selector = [0u8; 4];
        if env.data.len() >= 4 {
            selector.copy_from_slice(&env.data[..4]);
        }
        let code = to
            .and_then(|addr| storage.code_hash(&addr).ok().flatten())
            .unwrap_or_default();
        let key = match kind {
            ClassKeyKind::ToSelector => hash_deterministic((to, selector)),
            ClassKeyKind::CodeHashSelector => hash_deterministic((code, selector)),
        };
        keys.push(key);
        if to.is_some() {
            groups.entry(key).or_default().push(keys.len() - 1);
        }
    }
    let mut class_of = vec![u16::MAX; txs.len()];
    let mut out = Vec::new();
    for members in groups.into_values() {
        if members.len() < 2 || out.len() >= u16::MAX as usize {
            continue;
        }
        let id = out.len() as u16;
        for &tx in &members {
            class_of[tx] = id;
        }
        out.push(ClassGroup { members });
    }
    let _ = keys;
    (out, class_of)
}

fn evaluate_lazy<S: Storage, C: PevmChain>(
    chain: &C,
    storage: &S,
    spec_id: C::EvmSpecId,
    txs: &[C::EvmTx],
    mv: &SfMv,
    fully: &mut [PevmTxExecutionResult],
) -> Result<(), PevmError<C>> {
    for address in mv.consume_lazy_addresses() {
        let location_hash = hash_deterministic(MemoryLocation::Basic(address));
        let Some(write_history) = mv.data.get(&location_hash) else {
            continue;
        };
        let mut balance = U256::ZERO;
        let mut nonce = 0u64;
        if !matches!(
            write_history.first_key_value(),
            Some((_, MemoryEntry::Data(_, MemoryValue::Basic(_))))
        ) && let Ok(Some(account)) = storage.basic(&address)
        {
            balance = account.balance;
            nonce = account.nonce;
        }
        let code_hash = storage
            .code_hash(&address)
            .map_err(|err| PevmError::StorageError(err.to_string()))?;
        let code = if let Some(code_hash) = &code_hash {
            storage
                .code_by_hash(code_hash)
                .map_err(|err| PevmError::StorageError(err.to_string()))?
        } else {
            None
        };
        for (tx_idx, memory_entry) in write_history.iter() {
            let tx = chain.tx_env(unsafe { txs.get_unchecked(*tx_idx) });
            match memory_entry {
                MemoryEntry::Data(_, MemoryValue::Basic(info)) => {
                    balance = info.balance;
                    nonce = info.nonce;
                }
                MemoryEntry::Data(_, MemoryValue::LazyRecipient(addition)) => {
                    balance = balance.saturating_add(*addition);
                }
                MemoryEntry::Data(_, MemoryValue::LazySender(subtraction)) => {
                    let mut max_fee = U256::from(tx.gas_limit)
                        .saturating_mul(U256::from(tx.gas_price))
                        .saturating_add(tx.value);
                    max_fee = max_fee.saturating_add(
                        U256::from(tx.total_blob_gas())
                            .saturating_mul(U256::from(tx.max_fee_per_blob_gas)),
                    );
                    if balance < max_fee {
                        return Err(PevmError::ExecutionError(ExecutionError::Transaction(
                            revm::context::result::InvalidTransaction::LackOfFundForMaxFee {
                                balance: Box::new(balance),
                                fee: Box::new(max_fee),
                            },
                        )));
                    }
                    balance = balance.saturating_sub(*subtraction);
                    nonce += 1;
                }
                MemoryEntry::Estimate => continue,
                _ => return Err(PevmError::UnreachableError),
            }
            if tx.caller == address {
                let executed_nonce = if nonce == 0 {
                    return Err(PevmError::UnreachableError);
                } else {
                    nonce - 1
                };
                if tx.nonce != executed_nonce {
                    return Err(PevmError::NonceMismatch {
                        tx_idx: *tx_idx,
                        tx_nonce: tx.nonce,
                        executed_nonce,
                    });
                }
            }
            let tx_result = &mut fully[*tx_idx];
            let account = tx_result.state.entry(address).or_default();
            if chain.is_eip_161_enabled(spec_id)
                && code_hash.is_none()
                && nonce == 0
                && balance == U256::ZERO
            {
                *account = None;
            } else if let Some(account) = account {
                account.balance = balance;
                account.nonce = nonce;
            } else {
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
    Ok(())
}
