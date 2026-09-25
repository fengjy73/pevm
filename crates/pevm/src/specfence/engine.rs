//! `SpecFence` block driver.
//!
//! Workers share Chase-Lev deques seeded by transaction index. A transaction
//! commits only as the next prefix entry. Wakeups do not wait for that prefix:
//! an incarnation is final once it has finished, every read it consumed is
//! final or from storage, and validation has kept it. The block returns when
//! `committed_upto == n` and the final rescan has repeated that check.

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
use super::trace::{AbortNote, SfTrace, Trace};
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
    let serial = workers == 1;
    let timeline = super::timeline::Timeline::start(n, workers);
    super::timeline::bind(0);
    let t_setup = super::timeline::stamp();
    // One worker never races a pre-seeded estimate. Building the upstream
    // memory just to copy the beneficiary's `0..n` estimates is a second
    // allocation of the same map. The beneficiary still has to be lazy so
    // rewards evaluate at the end of the block.
    let mv = if serial {
        SfMv::new(n, std::iter::empty(), [block_env.beneficiary])
    } else {
        let template = {
            let _b = super::buckets::Guard::start(super::buckets::TEMPLATE);
            chain.build_mv_memory(&block_env, &txs)
        };
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
        SfMv::new(n, estimates, lazy)
    };
    let beneficiary = hash_deterministic(MemoryLocation::Basic(block_env.beneficiary));
    let mut live = if serial {
        LiveChain::untracked(n)
    } else {
        let _b = super::buckets::Guard::start(super::buckets::ALLOC);
        LiveChain::new(n, options.class_key)
    };
    live.skip_location(beneficiary);
    if !serial {
        let (groups, class_of, to_of) = {
            let _b = super::buckets::Guard::start(super::buckets::CLASS);
            build_classes(chain, storage, &txs, options.class_key)
        };
        live.install_classes(groups, class_of, to_of);
        let _b = super::buckets::Guard::start(super::buckets::PRESEED);
        live.preseed_recipients();
    }
    for (location, writers) in &options.radar {
        live.install_radar(*location, writers);
    }
    let live = live;
    let rt = {
        let _b = super::buckets::Guard::start(super::buckets::RUNTIME);
        let rt = Runtime::new(n, workers);
        rt.seed();
        rt
    };
    let prev_sender = previous_senders(chain, &txs);
    // Radar chain heads are seeded by pushing the lowest radar member later;
    // with an empty radar this is a no-op. Index seeding already ran.
    let trace = Trace::new(n);
    let results = Results::new(n);
    let abort: OnceLock<AbortKind> = OnceLock::new();
    let commit_mu = Mutex::new(());
    super::timeline::post_span(super::timeline::SETUP, t_setup);
    let started = Instant::now();
    let deadline = Duration::from_secs(120);
    let tl = super::timeline::vm_enabled();

    let prev_sender_slice = prev_sender.as_slice();
    let mv = &mv;
    let live = &live;
    let rt = &rt;
    let trace = &trace;
    let commit_mu = &commit_mu;
    let drive = |worker: usize| {
        super::timeline::bind(worker);
        let mut vm = SfVm::new(
            chain,
            spec_id,
            &block_env,
            txs.as_slice(),
            storage,
            mv,
            live,
            rt,
            trace,
            prev_sender_slice,
        );
        let mut idle = 0u32;
        let mut spins = 0u32;
        let mut idle_since = 0u64;
        let mut idle_waited = false;
        while rt.committed() < n && !rt.aborted() {
            spins = spins.wrapping_add(1);
            if spins.is_multiple_of(64) {
                let _b = super::buckets::Guard::start(super::buckets::DEADLINE);
                if started.elapsed() > deadline {
                    rt.request_abort();
                    break;
                }
            }
            try_commit(worker, n, rt, mv, live, trace, commit_mu, tl);
            let popped = {
                let _c = super::timeline::CycGuard::enter(tl, super::timeline::CYC_SCHED);
                let _b = super::buckets::Guard::start(super::buckets::SCHED);
                rt.pop(worker)
            };
            if let Some((tx, inc)) = popped {
                idle = 0;
                live.note_idle(false);
                if tl {
                    if idle_since != 0 {
                        super::timeline::idle_span(idle_since, idle_waited);
                        idle_since = 0;
                        idle_waited = false;
                    }
                    super::timeline::close_park(tx);
                }
                if !serial {
                    if let Some(pred) = live.admission_predecessor(tx)
                        && !rt.is_executing(pred)
                        && !rt.is_committed(pred)
                        && !rt.is_executed(pred)
                    {
                        // Not admitted: the predecessor has not started.
                        // Tier A still starts the tx once the predecessor is executing;
                        // that case falls through because `is_executing` is true.
                        if tl {
                            super::timeline::open_park(
                                tx,
                                pred,
                                0,
                                live.class_id(tx),
                                super::timeline::ADMIT,
                            );
                        }
                        rt.park(worker, tx, pred, false, false);
                        continue;
                    }
                    if let Some(head) = live.class_head(tx)
                        && !rt.is_committed(head)
                        && !rt.is_executed(head)
                    {
                        // The class head publishes read-then-write locations
                        // before classmates read them. Incarnation stays: this
                        // attempt has not entered the interpreter.
                        if tl {
                            super::timeline::open_park(
                                tx,
                                head,
                                0,
                                live.class_id(tx),
                                super::timeline::CLASS,
                            );
                        }
                        rt.park(worker, tx, head, false, false);
                        continue;
                    }
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
                    Step::Done => {
                        if !serial {
                            // A write that landed under a higher executed reader
                            // invalidates that reader before this incarnation
                            // can become final.
                            revoke_writes(worker, tx, n, rt, mv, live, trace, commit_mu);
                        }
                        rt.finish_ok(worker, tx);
                        if !serial {
                            close_from(worker, tx, n, rt, mv, live, trace, commit_mu, tl);
                        }
                    }
                    Step::Yield => rt.defer(tx),
                    Step::Block(pred) => {
                        if pred >= n || rt.committed() > pred {
                            rt.defer(tx);
                        } else {
                            // Unarmed waits end when the predecessor finishes.
                            // Armed reads, nonce, and estimates wait until that
                            // incarnation is final, which is not its commit.
                            let (reason, loc) = super::timeline::block_wait();
                            let until_final = reason != super::timeline::UNARMED;
                            if tl {
                                super::timeline::open_park(
                                    tx,
                                    pred,
                                    loc,
                                    live.class_id(tx),
                                    if reason == 0 {
                                        super::timeline::OTHER
                                    } else {
                                        reason
                                    },
                                );
                            }
                            rt.park(worker, tx, pred, false, until_final);
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
                try_commit(worker, n, rt, mv, live, trace, commit_mu, tl);
            } else {
                idle += 1;
                live.note_idle(true);
                if tl && idle_since == 0 {
                    idle_since = super::timeline::stamp();
                }
                try_commit(worker, n, rt, mv, live, trace, commit_mu, tl);
                if rt.committed() == n {
                    break;
                }
                if !rt.rescue(worker) {
                    if idle > 8 {
                        rt.wait_brief();
                        idle_waited = true;
                        idle = 0;
                    } else {
                        thread::yield_now();
                    }
                }
            }
        }
        if tl && idle_since != 0 {
            super::timeline::idle_span(idle_since, idle_waited);
        }
    };
    if serial {
        drive(0);
    } else {
        thread::scope(|scope| {
            for worker in 0..workers {
                scope.spawn(move || drive(worker));
            }
        });
    }

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

    let t_post = super::timeline::stamp();
    {
        let _b = super::buckets::Guard::start(super::buckets::RESCAN);
        for tx in 0..n {
            if let Some(loc) = mv.failing_location(tx) {
                eprintln!("specfence final read set mismatch tx={tx} loc={loc}");
                return Err(PevmError::UnreachableError);
            }
            mv.collect_edges(tx, &trace);
        }
    }
    super::timeline::post_span(super::timeline::RESCAN, t_post);

    let mut fully = Vec::with_capacity(n);
    let mut cumulative_gas_used: u64 = 0;
    for tx_idx in 0..n {
        let mut execution_result = results.take(tx_idx);
        cumulative_gas_used =
            cumulative_gas_used.saturating_add(execution_result.receipt.cumulative_gas_used);
        execution_result.receipt.cumulative_gas_used = cumulative_gas_used;
        fully.push(execution_result);
    }

    let t_lazy = super::timeline::stamp();
    {
        let _b = super::buckets::Guard::start(super::buckets::LAZY);
        evaluate_lazy(chain, storage, spec_id, &txs, &mv, &mut fully)?;
    }
    super::timeline::post_span(super::timeline::LAZY, t_lazy);

    trace.dump_aborts();
    super::buckets::dump();
    timeline.dump(
        options.class_key.as_str(),
        trace.full_replay.load(std::sync::atomic::Ordering::Relaxed),
        trace.reexec.load(std::sync::atomic::Ordering::Relaxed),
        live.max_chain_len(),
        live.armed_locations(),
        beneficiary,
        live,
        mv,
        prev_sender_slice,
        trace,
    );
    let snap = trace.snapshot(
        live.max_chain_len(),
        live.armed_locations(),
        options.class_key.as_str(),
        beneficiary,
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
    tl: bool,
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
        if !rt.is_final_inc(i, inc) {
            let t_val = if tl { super::timeline::stamp() } else { 0 };
            let mismatch = {
                let _b = super::buckets::Guard::start(super::buckets::VALIDATE);
                mv.failing_location(i)
            };
            if tl {
                super::timeline::validate_span(i, t_val);
            }
            if mismatch.is_some() {
                if abort_one(worker, i, rt, mv, live, trace) {
                    cascade_readers(worker, i, n, rt, mv, live, trace);
                }
                return;
            }
            // Every lower transaction is already committed, so it is final.
            // Do not stall the prefix on a flag the early path missed.
            if rt.mark_validated(i, inc) {
                let writes = mv.write_locations(i);
                live.hole_clear(i, &writes);
                rt.note_final(worker, i);
            }
        }
        if rt.try_mark_committed(worker, i, inc) {
            super::timeline::note_commit(i);
            trace.note_committed(i, inc);
            let writes = mv.write_locations(i);
            live.hole_clear(i, &writes);
            live.note_finished(false);
            rt.notify();
        } else {
            return;
        }
    }
}

/// Drop higher executed readers whose origin no longer matches this write.
fn revoke_writes(
    worker: usize,
    writer: TxIdx,
    n: usize,
    rt: &Runtime,
    mv: &SfMv,
    live: &LiveChain,
    trace: &Trace,
    commit_mu: &Mutex<()>,
) {
    let Ok(_guard) = commit_mu.lock() else {
        return;
    };
    cascade_readers(worker, writer, n, rt, mv, live, trace);
}

fn cascade_readers(
    worker: usize,
    root: TxIdx,
    n: usize,
    rt: &Runtime,
    mv: &SfMv,
    live: &LiveChain,
    trace: &Trace,
) {
    // Read-from edges point at a lower writer, so this walk cannot cycle.
    let mut stack = vec![root];
    let mut seen = vec![false; n];
    if root < n {
        seen[root] = true;
    }
    while let Some(tx) = stack.pop() {
        let locations = mv.write_locations(tx);
        for location in locations {
            for reader in mv.readers_of(location) {
                if reader <= tx || reader >= n || seen[reader] {
                    continue;
                }
                seen[reader] = true;
                if rt.is_committed(reader) || !rt.is_executed(reader) {
                    continue;
                }
                if mv.failing_location(reader).is_none() {
                    continue;
                }
                if abort_one(worker, reader, rt, mv, live, trace) {
                    stack.push(reader);
                }
            }
        }
    }
}

/// Mark `start` and every reader whose origins just became final.
///
/// The queue is the read-from edges plus readers of chain locations this
/// transaction was a member of, so a final hole retries the next reader.
fn close_from(
    worker: usize,
    start: TxIdx,
    n: usize,
    rt: &Runtime,
    mv: &SfMv,
    live: &LiveChain,
    trace: &Trace,
    commit_mu: &Mutex<()>,
    tl: bool,
) {
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start);
    // Closed and aborted transactions are not retried in this wave. A reader
    // that is still blocked by a gap member stays unseen so a hole cleared
    // later in the same wave can try it again.
    let mut settled = vec![false; n];
    while let Some(tx) = queue.pop_front() {
        if tx >= n || settled[tx] {
            continue;
        }
        if !rt.is_executed(tx) || rt.is_final_any(tx) {
            settled[tx] = true;
            continue;
        }
        let inc = rt.incarnation(tx);
        let t_val = if tl { super::timeline::stamp() } else { 0 };
        let mismatch = {
            let _b = super::buckets::Guard::start(super::buckets::VALIDATE);
            mv.failing_location(tx)
        };
        if tl {
            super::timeline::validate_span(tx, t_val);
        }
        if mismatch.is_some() {
            let Ok(_guard) = commit_mu.lock() else {
                continue;
            };
            if abort_one(worker, tx, rt, mv, live, trace) {
                cascade_readers(worker, tx, n, rt, mv, live, trace);
            }
            settled[tx] = true;
            continue;
        }
        if !mv.origins_final(tx, |writer, writer_inc| rt.is_final_inc(writer, writer_inc)) {
            continue;
        }
        if chain_blocks(tx, mv, live, rt) {
            continue;
        }
        if !rt.mark_validated(tx, inc) {
            continue;
        }
        settled[tx] = true;
        let member_locs = live.member_locations(tx);
        let writes = mv.write_locations(tx);
        live.hole_clear(tx, &writes);
        rt.note_final(worker, tx);
        for waiter in mv.waiters_of(tx) {
            if waiter > tx {
                queue.push_back(waiter);
            }
        }
        for location in member_locs {
            for reader in mv.readers_of(location) {
                if reader > tx {
                    queue.push_back(reader);
                }
            }
        }
    }
}

fn chain_blocks(tx: TxIdx, mv: &SfMv, live: &LiveChain, rt: &Runtime) -> bool {
    for (location, origin) in mv.read_floors(tx) {
        // One past the writer this read consumed. Storage has no origin, so
        // every still-open lower member can still publish the value.
        let start = origin.map(|writer| writer.saturating_add(1)).unwrap_or(0);
        if live.any_unresolved_between(location, start, tx, |member, inc, published| {
            if published {
                rt.is_final_inc(member, inc)
            } else {
                rt.is_final_any(member)
            }
        }) {
            return true;
        }
    }
    false
}

/// Validation failed. Caller holds `commit_mu`. Returns whether the abort landed.
fn abort_one(
    worker: usize,
    tx: TxIdx,
    rt: &Runtime,
    mv: &SfMv,
    live: &LiveChain,
    trace: &Trace,
) -> bool {
    if !rt.is_executed(tx) {
        return false;
    }
    let inc = rt.incarnation(tx);
    let Some(mismatch) = mv.first_mismatch(tx) else {
        return false;
    };
    let loc = mismatch.location;
    let armed = live.is_armed(loc);
    if trace.diag() {
        let live_tx = mismatch.live_tx;
        let origin_tx = mismatch.origin_tx;
        let state = live_tx
            .map(|writer| live.writer_state(loc, writer).0 as u8)
            .unwrap_or(0);
        trace.note_abort(AbortNote {
            reader: tx as u32,
            location: loc,
            origin_tx: origin_tx.map(|writer| writer as u32).unwrap_or(u32::MAX),
            live_tx: live_tx.map(|writer| writer as u32).unwrap_or(u32::MAX),
            live_in_chain_now: live_tx.is_some_and(|writer| live.member_bit(loc, writer)),
            live_state_now: state,
            armed_now: armed,
            read: None,
        });
    }
    let writes = mv.write_locations(tx);
    mv.convert_writes_to_estimates(tx);
    live.arm_failure(loc);
    backfill(mv, live, loc);
    live.note_class_conflict(live.class_id(tx));
    if let Some(writer) = mismatch.live_tx {
        live.note_class_conflict(live.class_id(writer));
        live.note_conflict_writer(loc, writer, |t| rt.still_open(t));
    }
    live.on_abort_residual(tx, inc.saturating_add(1), &writes);
    trace.note_full_replay(armed);
    live.note_finished(true);
    rt.requeue_abort(worker, tx);
    true
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
) -> (Vec<ClassGroup>, Vec<u16>, Vec<u64>) {
    let mut groups: HashMap<u64, (bool, Vec<TxIdx>), FxBuildHasher> =
        HashMap::with_hasher(FxBuildHasher);
    let mut to_of = Vec::with_capacity(txs.len());
    for (i, tx) in txs.iter().enumerate() {
        let env: &TxEnv = chain.tx_env(tx);
        let to = match env.kind {
            TxKind::Call(to) => Some(to),
            TxKind::Create => None,
        };
        let mut selector = [0u8; 4];
        let contract = env.data.len() >= 4;
        if contract {
            selector.copy_from_slice(&env.data[..4]);
        }
        let to_hash = to
            .map(|addr| hash_deterministic(MemoryLocation::Basic(addr)))
            .unwrap_or(0);
        to_of.push(to_hash);
        let key = match kind {
            ClassKeyKind::ToSelector => hash_deterministic((to, selector)),
            ClassKeyKind::CodeHashSelector => {
                let code = to
                    .and_then(|addr| storage.code_hash(&addr).ok().flatten())
                    .unwrap_or_default();
                hash_deterministic((code, selector))
            }
        };
        if to.is_some() {
            let entry = groups.entry(key).or_insert_with(|| (contract, Vec::new()));
            entry.1.push(i);
        }
    }
    let mut class_of = vec![u16::MAX; txs.len()];
    let mut out = Vec::new();
    for (contract, members) in groups.into_values() {
        if members.len() < 2 || out.len() >= u16::MAX as usize {
            continue;
        }
        let id = out.len() as u16;
        for &tx in &members {
            class_of[tx] = id;
        }
        out.push(ClassGroup { members, contract });
    }
    (out, class_of, to_of)
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use alloy_primitives::U256;

    use crate::{MemoryValue, TxVersion};

    use super::super::live_chain::{ClassKeyKind, LiveChain};
    use super::super::mv::{SfMv, SfOrigin, SfReadOrigin, SfReadSet};
    use super::super::rt::Runtime;
    use super::super::trace::Trace;
    use super::{close_from, revoke_writes};

    fn storage(value: u64) -> MemoryValue {
        MemoryValue::Storage(U256::from(value))
    }

    fn record_write(mv: &SfMv, tx: usize, location: u64, value: u64) {
        mv.record(
            &TxVersion {
                tx_idx: tx,
                tx_incarnation: 0,
            },
            SfReadSet::default(),
            vec![(location, storage(value))],
        );
    }

    fn record_edge(
        mv: &SfMv,
        reader: usize,
        location: u64,
        writer: usize,
        value: u64,
        write: Option<(u64, u64)>,
    ) {
        let mut reads = SfReadSet::default();
        reads.insert(
            location,
            smallvec::smallvec![SfReadOrigin::Mv(SfOrigin {
                tx_idx: writer,
                incarnation: 0,
                value: storage(value),
            })],
        );
        let writes = write
            .map(|(loc, val)| vec![(loc, storage(val))])
            .unwrap_or_default();
        mv.record(
            &TxVersion {
                tx_idx: reader,
                tx_incarnation: 0,
            },
            reads,
            writes,
        );
    }

    fn finish_and_close(
        rt: &Runtime,
        mv: &SfMv,
        live: &LiveChain,
        trace: &Trace,
        commit_mu: &Mutex<()>,
        tx: usize,
        n: usize,
    ) {
        rt.finish_ok(0, tx);
        close_from(0, tx, n, rt, mv, live, trace, commit_mu, false);
    }

    #[test]
    fn finality_cascades_along_reads_before_commit() {
        let n = 3;
        let mv = SfMv::new(n, std::iter::empty(), std::iter::empty());
        let live = LiveChain::untracked(n);
        let rt = Runtime::new(n, 1);
        let trace = Trace::new(n);
        let commit_mu = Mutex::new(());
        rt.seed();

        assert_eq!(rt.pop(0).unwrap().0, 0);
        record_write(&mv, 0, 11, 4);
        finish_and_close(&rt, &mv, &live, &trace, &commit_mu, 0, n);
        assert!(rt.is_final_inc(0, 0));
        assert_eq!(rt.committed(), 0);

        assert_eq!(rt.pop(0).unwrap().0, 1);
        record_edge(&mv, 1, 11, 0, 4, Some((12, 5)));
        finish_and_close(&rt, &mv, &live, &trace, &commit_mu, 1, n);
        assert!(rt.is_final_inc(1, 0));

        assert_eq!(rt.pop(0).unwrap().0, 2);
        record_edge(&mv, 2, 12, 1, 5, None);
        finish_and_close(&rt, &mv, &live, &trace, &commit_mu, 2, n);
        assert!(rt.is_final_inc(2, 0));
        assert_eq!(rt.committed(), 0);
    }

    #[test]
    fn late_lower_writer_revokes_a_final_reader() {
        let n = 3;
        let location = 77u64;
        let mv = SfMv::new(n, std::iter::empty(), std::iter::empty());
        let live = LiveChain::new(n, ClassKeyKind::ToSelector);
        let rt = Runtime::new(n, 2);
        let trace = Trace::new(n);
        let commit_mu = Mutex::new(());
        rt.seed();

        assert_eq!(rt.pop(0).unwrap().0, 0);
        record_write(&mv, 0, location, 1);
        live.publish_write(0, 0, location, false, |_| true);
        rt.finish_ok(0, 0);
        close_from(0, 0, n, &rt, &mv, &live, &trace, &commit_mu, false);
        assert!(rt.is_final_inc(0, 0));

        assert_eq!(rt.pop(0).unwrap().0, 2);
        record_edge(&mv, 2, location, 0, 1, None);
        rt.finish_ok(0, 2);
        close_from(0, 2, n, &rt, &mv, &live, &trace, &commit_mu, false);
        assert!(rt.is_final_inc(2, 0));
        assert_eq!(live.nearest_lower(location, 2), Some(0));

        assert_eq!(rt.pop(1).unwrap().0, 1);
        record_write(&mv, 1, location, 9);
        live.publish_write(1, 0, location, true, |_| true);
        assert_eq!(live.nearest_lower(location, 2), Some(1));
        revoke_writes(1, 1, n, &rt, &mv, &live, &trace, &commit_mu);
        assert!(!rt.is_final_any(2), "stale reader stays final");
        assert!(mv.failing_location(2).is_some());
        assert!(rt.is_final_inc(0, 0));
        assert_eq!(rt.committed(), 0);
    }

    #[test]
    fn abort_and_estimate_drop_finality() {
        let n = 2;
        let location = 3u64;
        let mv = SfMv::new(n, std::iter::empty(), std::iter::empty());
        let live = LiveChain::untracked(n);
        let rt = Runtime::new(n, 1);
        let trace = Trace::new(n);
        let commit_mu = Mutex::new(());
        rt.seed();

        assert_eq!(rt.pop(0).unwrap().0, 0);
        record_write(&mv, 0, location, 3);
        finish_and_close(&rt, &mv, &live, &trace, &commit_mu, 0, n);
        assert_eq!(rt.pop(0).unwrap().0, 1);
        record_edge(&mv, 1, location, 0, 3, None);
        finish_and_close(&rt, &mv, &live, &trace, &commit_mu, 1, n);
        assert!(rt.is_final_inc(1, 0));

        rt.requeue_abort(0, 0);
        mv.convert_writes_to_estimates(0);
        revoke_writes(0, 0, n, &rt, &mv, &live, &trace, &commit_mu);
        assert!(!rt.is_final_inc(0, 0));
        assert!(!rt.is_final_any(0));
        assert!(
            !rt.is_final_any(1),
            "reader of the aborted incarnation stays final"
        );
        assert!(mv.failing_location(1).is_some());
        assert!(!mv.origins_final(1, |writer, inc| rt.is_final_inc(writer, inc)));
        assert_eq!(rt.incarnation(0), 1);
    }

    #[test]
    fn final_hole_drops_out_of_the_chain() {
        let n = 3;
        let location = 10u64;
        let mut live = LiveChain::new(n, ClassKeyKind::ToSelector);
        live.install_classes(
            Vec::new(),
            vec![u16::MAX; n],
            vec![location, location, location],
        );
        live.preseed_recipients();
        assert_eq!(live.nearest_lower(location, 2), Some(1));
        live.publish_write(0, 0, location, false, |_| true);

        let rt = Runtime::new(n, 2);
        rt.seed();
        assert_eq!(rt.pop(1).unwrap().0, 1);
        rt.finish_ok(1, 1);
        assert!(rt.mark_validated(1, 0));
        assert!(rt.is_final_any(1));
        assert_eq!(rt.committed(), 0);
        assert_ne!(live.writer_state(location, 1).0, 3);
        live.clear_hole(location, 1);
        assert!(!live.member_bit(location, 1));
        assert_eq!(live.nearest_lower(location, 2), Some(0));
    }

    #[test]
    fn gap_member_blocks_finality_and_a_lower_member_does_not() {
        let n = 3;
        let location = 10u64;
        let mv = SfMv::new(n, std::iter::empty(), std::iter::empty());
        let mut live = LiveChain::new(n, ClassKeyKind::ToSelector);
        live.install_classes(
            Vec::new(),
            vec![u16::MAX; n],
            vec![location, location, location],
        );
        live.preseed_recipients();
        live.publish_write(1, 0, location, false, |_| true);
        let rt = Runtime::new(n, 1);
        let trace = Trace::new(n);
        let commit_mu = Mutex::new(());
        rt.seed();

        // tx0 stays predicted and not final. It is below tx2's origin.
        assert_eq!(rt.pop(0).unwrap().0, 0);
        rt.defer(0);
        assert_eq!(rt.pop(0).unwrap().0, 1);
        record_write(&mv, 1, location, 4);
        rt.finish_ok(0, 1);
        close_from(0, 1, n, &rt, &mv, &live, &trace, &commit_mu, false);
        assert!(rt.is_final_inc(1, 0));

        assert_eq!(rt.pop(0).unwrap().0, 2);
        record_edge(&mv, 2, location, 1, 4, None);
        rt.finish_ok(0, 2);
        close_from(0, 2, n, &rt, &mv, &live, &trace, &commit_mu, false);
        assert!(
            rt.is_final_inc(2, 0),
            "a member below the read origin does not block"
        );

        // A predicted member between the origin and the reader does block.
        let n = 3;
        let mv = SfMv::new(n, std::iter::empty(), std::iter::empty());
        let mut live = LiveChain::new(n, ClassKeyKind::ToSelector);
        live.install_classes(
            Vec::new(),
            vec![u16::MAX; n],
            vec![location, location, location],
        );
        live.preseed_recipients();
        live.publish_write(0, 0, location, false, |_| true);
        let rt = Runtime::new(n, 1);
        let trace = Trace::new(n);
        let commit_mu = Mutex::new(());
        rt.seed();
        assert_eq!(rt.pop(0).unwrap().0, 0);
        record_write(&mv, 0, location, 4);
        rt.finish_ok(0, 0);
        close_from(0, 0, n, &rt, &mv, &live, &trace, &commit_mu, false);
        // tx1 stays in the interpreter, so it is still a predicted gap member.
        assert_eq!(rt.pop(0).unwrap().0, 1);
        assert_eq!(rt.pop(0).unwrap().0, 2);
        record_edge(&mv, 2, location, 0, 4, None);
        rt.finish_ok(0, 2);
        close_from(0, 2, n, &rt, &mv, &live, &trace, &commit_mu, false);
        assert!(
            !rt.is_final_any(2),
            "predicted writer between origin and reader blocks finality"
        );
        rt.finish_ok(0, 1);
        close_from(0, 1, n, &rt, &mv, &live, &trace, &commit_mu, false);
        assert!(
            rt.is_final_inc(2, 0),
            "clearing the gap lets the reader close"
        );
        assert_eq!(rt.committed(), 0);
    }
}
