use std::{
    cmp::min,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use smallvec::SmallVec;

use crate::{
    FinishExecFlags, IncarnationStatus, Task, TxIdx, TxStatus, TxVersion,
    specfence::{FenceGraph, WaveParkTable},
};

// The Pevm collaborative scheduler coordinates execution & validation
// tasks among work threads.
//
// To pick a task, threads increment the smaller of the (execution and
// validation) task counters until they find a task that is ready to be
// performed. To redo a task for a transaction, the thread updates the status
// and reduces the corresponding counter to the transaction index if it had a
// larger value.
//
// An incarnation may write to a memory location that was previously
// read by a higher transaction. Thus, when an incarnation finishes, new
// validation tasks are created for higher transactions.
//
// Validation tasks are scheduled optimistically and in parallel. Identifying
// validation failures and aborting incarnations as soon as possible is critical
// for performance, as any incarnation that reads values written by an
// incarnation that aborts also must abort.
// When an incarnation writes only to a subset of memory locations written
// by the previously completed incarnation of the same transaction, we schedule
// validation just for the incarnation. This is sufficient as the whole write
// set of the previous incarnation is marked as ESTIMATE during the abort.
// The abort leads to optimistically creating validation tasks for higher
// transactions. Threads that perform these tasks can already detect validation
// failure due to the ESTIMATE markers on memory locations, instead of waiting
// for a subsequent incarnation to finish.
#[derive(Debug)]
pub(crate) struct Scheduler {
    // The number of transactions in this block.
    block_size: usize,
    // The most up-to-date incarnation number (initially 0) and
    // the status of this incarnation.
    // TODO: Consider packing [TxStatus]s into atomics instead of
    // [Mutex] given how small they are.
    transactions_status: Vec<Mutex<TxStatus>>,
    // Lock-free mirror: true iff status is Executed|Validated (Bind/Wait hot path).
    done_flags: Vec<AtomicBool>,
    // The list of dependent transactions to resume when the
    // key transaction is re-executed.
    transactions_dependents: Vec<Mutex<SmallVec<[TxIdx; 1]>>>,
    // The next transaction to try and execute.
    execution_idx: AtomicUsize,
    // The next transaction to try and validate.
    validation_idx: AtomicUsize,
    // We won't validate until we find the first non-lazy transaction that
    // needs to read explicit values. We also skip the first transaction.
    min_validation_idx: AtomicUsize,
    // The number of validated transactions
    num_validated: AtomicUsize,
    // True if the scheduler has been aborted, likely due to fatal execution
    // errors.
    aborted: AtomicBool,
}

// TODO: Better error handling.
// Like returning errors instead of panicking on [unreachable]s.
impl Scheduler {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            block_size,
            execution_idx: AtomicUsize::new(0),
            transactions_status: (0..block_size)
                .map(|_| {
                    Mutex::new(TxStatus {
                        incarnation: 0,
                        status: IncarnationStatus::ReadyToExecute,
                    })
                })
                .collect(),
            done_flags: (0..block_size).map(|_| AtomicBool::new(false)).collect(),
            transactions_dependents: (0..block_size).map(|_| Mutex::default()).collect(),
            // We won't validate until we find the first non-lazy transaction that
            // needs to read explicit values. We also skip the first transaction.
            validation_idx: AtomicUsize::new(block_size),
            min_validation_idx: AtomicUsize::new(block_size),
            num_validated: AtomicUsize::new(0),
            aborted: AtomicBool::new(false),
        }
    }

    pub(crate) fn block_size(&self) -> usize {
        self.block_size
    }

    pub(crate) fn abort(&self) {
        self.aborted.store(true, Ordering::Relaxed);
    }

    /// Final incarnation index per tx after the block (0 = succeeded on first try).
    pub(crate) fn incarnation_snapshot(&self) -> Vec<usize> {
        (0..self.block_size)
            .map(|tx_idx| {
                let tx = index_mutex!(self.transactions_status, tx_idx);
                tx.incarnation
            })
            .collect()
    }

    fn try_execute(&self, tx_idx: TxIdx) -> Option<TxVersion> {
        if tx_idx < self.block_size {
            let mut tx = index_mutex!(self.transactions_status, tx_idx);
            if tx.status == IncarnationStatus::ReadyToExecute {
                tx.status = IncarnationStatus::Executing;
                self.set_done_flag(tx_idx, false);
                return Some(TxVersion {
                    tx_idx,
                    tx_incarnation: tx.incarnation,
                });
            }
        }
        None
    }

    /// Prefer SpecFence wave ready deque (lower TxIdx first), then collaborative indices.
    #[allow(dead_code)]
    pub(crate) fn next_task(&self) -> Option<Task> {
        self.next_task_with_wave(None)
    }

    pub(crate) fn next_task_with_wave(&self, wave: Option<&WaveParkTable>) -> Option<Task> {
        if let Some(wave) = wave {
            // After park: prefer wave ready + one cautious execution steal so the core
            // does not idle when Ready work exists (avoid validation-first stampede).
            if wave.steal_after_park_pending() {
                if let Some(task) = self.next_task_steal_after_park(wave) {
                    return Some(task);
                }
            }
            while let Some(tx_idx) = wave.pop_ready() {
                if let Some(tx_version) = self.try_execute(tx_idx) {
                    wave.note_ready_steal_if_after_park();
                    return Some(Task::Execution(tx_version));
                }
            }
        }
        while !self.aborted.load(Ordering::Relaxed) {
            let execution_idx = self.execution_idx.load(Ordering::Relaxed);
            let validation_idx = self.validation_idx.load(Ordering::Relaxed);
            if execution_idx >= self.block_size && validation_idx >= self.block_size {
                if self.num_validated.load(Ordering::Relaxed)
                    >= self.block_size - self.min_validation_idx.load(Ordering::Relaxed)
                {
                    break;
                }
                // Re-check wave ready before yield — a producer may have just pushed.
                if let Some(wave) = wave {
                    while let Some(tx_idx) = wave.pop_ready() {
                        if let Some(tx_version) = self.try_execute(tx_idx) {
                            wave.note_ready_steal_if_after_park();
                            return Some(Task::Execution(tx_version));
                        }
                    }
                }
                thread::yield_now();
                continue;
            }

            // Prioritize a validation task to minimize re-execution
            if validation_idx < execution_idx {
                let tx_idx = self.validation_idx.fetch_add(1, Ordering::Relaxed);
                if tx_idx < self.block_size {
                    let mut tx = index_mutex!(self.transactions_status, tx_idx);
                    // "Steal" execution job while holding the lock
                    if tx.status == IncarnationStatus::ReadyToExecute {
                        tx.status = IncarnationStatus::Executing;
                        self.set_done_flag(tx_idx, false);
                        if let Some(wave) = wave {
                            wave.note_ready_steal_if_after_park();
                        }
                        return Some(Task::Execution(TxVersion {
                            tx_idx,
                            tx_incarnation: tx.incarnation,
                        }));
                    }
                    // Start a typical validation task
                    if matches!(
                        tx.status,
                        IncarnationStatus::Executed | IncarnationStatus::Validated
                    ) {
                        return Some(Task::Validation(TxVersion {
                            tx_idx,
                            tx_incarnation: tx.incarnation,
                        }));
                    }
                    // Validation index is still catching up so continue a
                    // new loop iteration to refetch the latest indices
                    // before deciding again.
                    if tx.status == IncarnationStatus::Aborting {
                        continue;
                    }
                    // Fall back to execution job as this executing tx will
                    // decide if validation is needed when it's done. If it
                    // does, all validation tasks here would be redone anyway.
                }
            }

            // Prioritize execution task
            if let Some(tx_version) =
                self.try_execute(self.execution_idx.fetch_add(1, Ordering::Relaxed))
            {
                if let Some(wave) = wave {
                    wave.note_ready_steal_if_after_park();
                }
                return Some(Task::Execution(tx_version));
            }
        }
        if let Some(wave) = wave {
            wave.clear_steal_flag();
        }
        None
    }

    /// Park→steal: wave ready first, then one cautious `execution_idx` fetch_add.
    /// Counts `ready_steal_on_wait` only for Execution steals (not validation).
    pub(crate) fn next_task_steal_after_park(&self, wave: &WaveParkTable) -> Option<Task> {
        self.next_task_steal_after_park_prefer(wave, None)
    }

    pub(crate) fn next_task_steal_after_park_prefer(
        &self,
        wave: &WaveParkTable,
        prefer: Option<TxIdx>,
    ) -> Option<Task> {
        if let Some(idx) = prefer
            && let Some(tx_version) = self.try_execute(idx)
        {
            wave.note_ready_steal_if_after_park();
            return Some(Task::Execution(tx_version));
        }
        while let Some(tx_idx) = wave.pop_ready() {
            if let Some(tx_version) = self.try_execute(tx_idx) {
                wave.note_ready_steal_if_after_park();
                return Some(Task::Execution(tx_version));
            }
        }
        let idx = self.execution_idx.fetch_add(1, Ordering::Relaxed);
        if let Some(tx_version) = self.try_execute(idx) {
            wave.note_ready_steal_if_after_park();
            return Some(Task::Execution(tx_version));
        }
        None
    }

    // Add [tx_idx] as a dependent of [blocking_tx_idx] so [tx_idx] is
    // re-executed when the next [blocking_tx_idx] incarnation is executed.
    // Return [false] if we encounter a race condition when [blocking_tx_idx]
    // gets re-executed before the dependency can be added.
    pub(crate) fn add_dependency(&self, tx_idx: TxIdx, blocking_tx_idx: TxIdx) -> bool {
        // This is an important lock to prevent a race condition where the blocking
        // transaction completes re-execution before this dependency can be added.
        let blocking_tx = index_mutex!(self.transactions_status, blocking_tx_idx);
        if matches!(
            blocking_tx.status,
            IncarnationStatus::Executed | IncarnationStatus::Validated
        ) {
            return false;
        }

        let mut tx = index_mutex!(self.transactions_status, tx_idx);
        debug_assert_eq!(tx.status, IncarnationStatus::Executing);
        tx.status = IncarnationStatus::Aborting;
        self.set_done_flag(tx_idx, false);

        let mut blocking_dependents = index_mutex!(self.transactions_dependents, blocking_tx_idx);
        blocking_dependents.push(tx_idx);

        true
    }

    /// Iter3 serial-barrier resolve: after validation abort the tx is already
    /// `Aborting`. Park it behind an unfinished writer (`blocking_tx_idx`) so the
    /// FullRestart runs once writers have published Data — SpecFence-native
    /// barrier, not a global OCC serialize.
    ///
    /// Returns `false` if the writer is already Executed|Validated (race).
    pub(crate) fn add_dependency_from_aborting(
        &self,
        tx_idx: TxIdx,
        blocking_tx_idx: TxIdx,
    ) -> bool {
        let blocking_tx = index_mutex!(self.transactions_status, blocking_tx_idx);
        if matches!(
            blocking_tx.status,
            IncarnationStatus::Executed | IncarnationStatus::Validated
        ) {
            return false;
        }
        {
            let tx = index_mutex!(self.transactions_status, tx_idx);
            debug_assert_eq!(tx.status, IncarnationStatus::Aborting);
        }
        let mut blocking_dependents = index_mutex!(self.transactions_dependents, blocking_tx_idx);
        blocking_dependents.push(tx_idx);
        true
    }

    /// Abort finish that arms Ready + cascade rewind but does **not** immediately
    /// `try_execute` the aborted tx (steal-first). Kept for Iter4 clique experiments;
    /// Iter3 production path uses writer-barrier park only.
    #[allow(dead_code)]
    pub(crate) fn finish_validation_fenced_defer_exec(
        &self,
        tx_version: &TxVersion,
        rewind_to: Option<TxIdx>,
        wave: Option<&crate::specfence::WaveParkTable>,
    ) -> Option<Task> {
        self.set_ready_status(tx_version.tx_idx);
        if let Some(wave) = wave {
            wave.push_ready(tx_version.tx_idx);
        }
        if self.execution_idx.load(Ordering::Relaxed) > tx_version.tx_idx {
            self.execution_idx
                .fetch_min(tx_version.tx_idx, Ordering::Relaxed);
        }
        if let Some(to) = rewind_to {
            let to = to.clamp(tx_version.tx_idx + 1, self.block_size);
            self.validation_idx.fetch_min(to, Ordering::Relaxed);
        }
        None
    }

    /// Validation abort parked on a writer: leave `Aborting`, rewind cascade only.
    /// Writer `finish_execution` drains dependents → Ready.
    pub(crate) fn finish_validation_fenced_barrier_park(
        &self,
        tx_version: &TxVersion,
        rewind_to: Option<TxIdx>,
    ) -> Option<Task> {
        // Status stays Aborting (dependency already registered).
        if let Some(to) = rewind_to {
            let to = to.clamp(tx_version.tx_idx + 1, self.block_size);
            self.validation_idx.fetch_min(to, Ordering::Relaxed);
        }
        None
    }

    fn set_ready_status(&self, tx_idx: TxIdx) {
        let mut tx = index_mutex!(self.transactions_status, tx_idx);
        debug_assert_eq!(tx.status, IncarnationStatus::Aborting);
        tx.status = IncarnationStatus::ReadyToExecute;
        tx.incarnation += 1;
        self.set_done_flag(tx_idx, false);
    }

    #[allow(dead_code)]
    pub(crate) fn finish_execution(
        &self,
        tx_version: TxVersion,
        flags: FinishExecFlags,
    ) -> Option<Task> {
        self.finish_execution_with_wave(tx_version, flags, None)
    }

    /// Like [`finish_execution`], and on SpecFence push woken waiters onto the
    /// wave ready deque (lower TxIdx first). Also runs `wake_writer_done` so
    /// location-keyed parks accumulate `wait_park_ns`.
    pub(crate) fn finish_execution_with_wave(
        &self,
        tx_version: TxVersion,
        flags: FinishExecFlags,
        wave: Option<&WaveParkTable>,
    ) -> Option<Task> {
        self.finish_execution_with_wave_fence(tx_version, flags, wave, None)
    }

    /// SpecFence P2: also clear FenceGraph SoftWaits for the finishing writer.
    pub(crate) fn finish_execution_with_wave_fence(
        &self,
        tx_version: TxVersion,
        flags: FinishExecFlags,
        wave: Option<&WaveParkTable>,
        fence: Option<&FenceGraph>,
    ) -> Option<Task> {
        let mut tx = index_mutex!(self.transactions_status, tx_version.tx_idx);
        debug_assert_eq!(tx.status, IncarnationStatus::Executing);
        debug_assert_eq!(tx.incarnation, tx_version.tx_incarnation);

        // Drain dependents first; set Executed/Validated *before* waking so SoftWait
        // `is_done` is true as soon as the writer lock is released / waiters proceed.
        let mut drained: SmallVec<[TxIdx; 4]> = SmallVec::new();
        {
            let mut dependents = index_mutex!(self.transactions_dependents, tx_version.tx_idx);
            drained.extend(dependents.drain(..));
        }

        // TODO: Simplify or better document this logic.
        // Decide where to validate from next
        let min_validation_idx = if flags.contains(FinishExecFlags::NeedValidation) {
            min(
                self.min_validation_idx
                    .fetch_min(tx_version.tx_idx, Ordering::Relaxed),
                tx_version.tx_idx,
            )
        } else {
            self.min_validation_idx.load(Ordering::Relaxed)
        };

        let mut early_return_validation = false;
        // Have found a min validation index to even bother
        if min_validation_idx < self.block_size {
            // Must re-validate from min as this transaction is lower
            if tx_version.tx_idx < min_validation_idx {
                if flags.contains(FinishExecFlags::WroteNewLocation) {
                    self.validation_idx
                        .fetch_min(min_validation_idx, Ordering::Relaxed);
                }
            }
            // Validate from this transaction as it's in between min and the current
            // validation index.
            else if tx_version.tx_idx < self.validation_idx.load(Ordering::Relaxed) {
                if flags.contains(FinishExecFlags::WroteNewLocation) {
                    self.validation_idx
                        .fetch_min(tx_version.tx_idx + 1, Ordering::Relaxed);
                }
                if flags.contains(FinishExecFlags::NeedValidation) {
                    tx.status = IncarnationStatus::Executed;
                    early_return_validation = true;
                } else {
                    tx.status = IncarnationStatus::Validated;
                    self.num_validated.fetch_add(1, Ordering::Relaxed);
                }
            }
            // Don't need to validate anything if the current validation index is
            // lower or equal -- it will catch up later.
        }

        if !early_return_validation {
            if flags.contains(FinishExecFlags::NeedValidation) {
                tx.status = IncarnationStatus::Executed;
            } else {
                tx.status = IncarnationStatus::Validated;
                self.num_validated.fetch_add(1, Ordering::Relaxed);
            }
        }
        // Publish lock-free done before releasing status mutex / waking waiters.
        self.set_done_flag(
            tx_version.tx_idx,
            matches!(
                tx.status,
                IncarnationStatus::Executed | IncarnationStatus::Validated
            ),
        );

        // Wake after status is Data-ready (`is_done`), still under writer lock so
        // add_dependency cannot lose a waiter between drain and Ready.
        for tx_idx in drained {
            self.set_ready_status(tx_idx);
            self.execution_idx.fetch_min(tx_idx, Ordering::Relaxed);
            if let Some(wave) = wave {
                wave.push_ready(tx_idx);
            }
        }
        // Location-keyed WaitHard parks: wake → ready deque + park_ns.
        if let Some(wave) = wave {
            for waiter in wave.wake_writer_done(tx_version.tx_idx) {
                // Dependents path already set Ready; location-only waiters still
                // need Ready (should not happen if park always used add_dependency).
                let _ = waiter;
            }
        }
        // P2: FenceGraph SoftWait source of truth — clear arms on Publish/finish.
        if let Some(fence) = fence {
            let _ = fence.clear_for_writer(tx_version.tx_idx);
        }

        if early_return_validation {
            Some(Task::Validation(tx_version))
        } else {
            None
        }
    }

    // Return whether the abort was successful. A successful abort leads to
    // scheduling the transaction for re-execution and the higher transactions
    // for validation during [finish_validation]. The scheduler ensures that only
    // one failing validation per version can lead to a successful abort.
    pub(crate) fn try_validation_abort(&self, tx_version: &TxVersion) -> bool {
        let mut tx = index_mutex!(self.transactions_status, tx_version.tx_idx);
        if tx.status == IncarnationStatus::Validated {
            self.num_validated.fetch_sub(1, Ordering::Relaxed);
        }

        let aborting = matches!(
            tx.status,
            IncarnationStatus::Executed | IncarnationStatus::Validated
        );
        if aborting {
            tx.status = IncarnationStatus::Aborting;
            self.set_done_flag(tx_version.tx_idx, false);
        }
        aborting
    }

    // When there is a successful abort, schedule the transaction for re-execution
    // and the higher transactions for validation. The re-execution task is returned
    // for the aborted transaction.
    /// True when this transaction has finished the current incarnation enough
    /// for a Wait-mode reader to consume its writes (`Executed` or `Validated`).
    /// Lock-free via `done_flags` (kept in sync with status transitions).
    #[inline]
    pub(crate) fn is_done(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size {
            return true;
        }
        // SAFETY: tx_idx checked against block_size above.
        unsafe { self.done_flags.get_unchecked(tx_idx).load(Ordering::Acquire) }
    }

    /// True when the incarnation is actively `Executing` (not merely Ready/Aborting).
    /// Used by Iter3 serial-barrier to avoid parking behind idle/queued writers.
    #[inline]
    pub(crate) fn is_executing(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size {
            return false;
        }
        let tx = index_mutex!(self.transactions_status, tx_idx);
        tx.status == IncarnationStatus::Executing
    }

    /// True when status is `Aborting` (Iter4 clique sibling park).
    #[inline]
    pub(crate) fn is_aborting(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size {
            return false;
        }
        let tx = index_mutex!(self.transactions_status, tx_idx);
        tx.status == IncarnationStatus::Aborting
    }

    #[inline]
    fn set_done_flag(&self, tx_idx: TxIdx, done: bool) {
        // SAFETY: callers only use inbound tx indices.
        unsafe {
            self.done_flags
                .get_unchecked(tx_idx)
                .store(done, Ordering::Release);
        }
    }

    /// Research label for producer readiness at discovery (finegrain journal).
    pub(crate) fn status_label(&self, tx_idx: TxIdx) -> &'static str {
        if tx_idx >= self.block_size {
            return "oob";
        }
        let tx = index_mutex!(self.transactions_status, tx_idx);
        match tx.status {
            IncarnationStatus::ReadyToExecute => "ready",
            IncarnationStatus::Executing => "executing",
            IncarnationStatus::Executed => "executed",
            IncarnationStatus::Validated => "validated",
            IncarnationStatus::Aborting => "aborting",
        }
    }

    pub(crate) fn finish_validation(&self, tx_version: &TxVersion, aborted: bool) -> Option<Task> {
        // Classic Block-STM: abort cascades validation from aborted_idx+1 through
        // the rest of the block.
        self.finish_validation_fenced(
            tx_version,
            aborted,
            aborted.then_some(tx_version.tx_idx + 1),
            None,
        )
    }

    /// Like [`finish_validation`], but on abort only rewinds `validation_idx` to
    /// `rewind_to` (the first higher tx that read an aborted write). `None` means
    /// no higher dependent reader was found — do not force a suffix cascade.
    /// When `wave` is set, push the aborted tx onto the ready deque and
    /// `execution_idx.fetch_min` so SoftWait/EarlyAbort park workers can steal it.
    pub(crate) fn finish_validation_fenced(
        &self,
        tx_version: &TxVersion,
        aborted: bool,
        rewind_to: Option<TxIdx>,
        wave: Option<&crate::specfence::WaveParkTable>,
    ) -> Option<Task> {
        if aborted {
            self.set_ready_status(tx_version.tx_idx);
            // Park→steal: always push wave ready so SoftWait park workers see Ready txs.
            // Only rewind execution_idx when it already passed this tx (avoid stampede
            // reexec of low-idx aborts that inflates median abort cascades).
            if let Some(wave) = wave {
                wave.push_ready(tx_version.tx_idx);
            }
            if self.execution_idx.load(Ordering::Relaxed) > tx_version.tx_idx {
                self.execution_idx
                    .fetch_min(tx_version.tx_idx, Ordering::Relaxed);
            }
            if let Some(to) = rewind_to {
                // Never rewind past the aborted tx itself; clamp to [tx_idx+1, block_size].
                let to = to.clamp(tx_version.tx_idx + 1, self.block_size);
                self.validation_idx.fetch_min(to, Ordering::Relaxed);
            }
            return self.try_execute(tx_version.tx_idx).map(Task::Execution);
        } else {
            let mut tx = index_mutex!(self.transactions_status, tx_version.tx_idx);
            if tx.status == IncarnationStatus::Executed {
                tx.status = IncarnationStatus::Validated;
                self.num_validated.fetch_add(1, Ordering::Relaxed);
            }
        }
        None
    }

}

