use std::{
    cmp::min,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Instant,
};

use smallvec::SmallVec;

use crate::{
    FinishExecFlags, IncarnationStatus, Task, TxIdx, TxStatus, TxVersion,
    specfence::{FenceGraph, ReadyEdgeTable, WaveParkTable, profile_timing_enabled},
};

/// After refuse, steal one nearby independent. No full-block scan (PRIMARY tax).
const WAVE_FILL_WINDOW: usize = 32;

/// Result of the exhausted-idx Ready scan.
enum MinRun {
    Hit(TxIdx),
    Empty,
    Busy,
}

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
    // Lock-free mirror: true iff status is Executed|Validated (OrderedAdmit/Wait hot path).
    done_flags: Vec<AtomicBool>,
    // Lock-free mirror: true iff status is Validated (Iter12 ESTIMATE-race gate).
    // OrderedAdmit-on-Executed can still see ESTIMATE if the writer later aborts; 2nd-repair
    // / serial-barrier paths spin for Validated without SoftWait Soft park tax.
    validated_flags: Vec<AtomicBool>,
    // The list of dependent transactions to resume when the
    // key transaction is re-executed.
    transactions_dependents: Vec<Mutex<SmallVec<[TxIdx; 1]>>>,
    /// `add_dependency` target while status is `Aborting`. `usize::MAX` if none.
    /// Heal must not `recover_aborting` (incarnation++) while this writer is
    /// still unpublished — that mill hit 19807137 (`root_inc` tens of thousands).
    blocked_on: Vec<AtomicUsize>,
    /// wait_for_dependency waiters: park without `Aborting`; wake keeps incarnation.
    wait_for_dependency_waiters: Vec<Mutex<SmallVec<[TxIdx; 1]>>>,
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
    /// Cursor for the exhausted-idx Ready scan (skip already-done txs).
    ready_hint: AtomicUsize,
    /// Single-flight the O(n) Ready scan so 8 workers cannot mutex-walk
    /// a 37k ERC-20 block on every yield.
    ready_scan: AtomicBool,
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
            validated_flags: (0..block_size).map(|_| AtomicBool::new(false)).collect(),
            transactions_dependents: (0..block_size).map(|_| Mutex::default()).collect(),
            blocked_on: (0..block_size)
                .map(|_| AtomicUsize::new(usize::MAX))
                .collect(),
            wait_for_dependency_waiters: (0..block_size).map(|_| Mutex::default()).collect(),
            // We won't validate until we find the first non-lazy transaction that
            // needs to read explicit values. We also skip the first transaction.
            validation_idx: AtomicUsize::new(block_size),
            min_validation_idx: AtomicUsize::new(block_size),
            num_validated: AtomicUsize::new(0),
            aborted: AtomicBool::new(false),
            ready_hint: AtomicUsize::new(0),
            ready_scan: AtomicBool::new(false),
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
        self.try_execute_ready(tx_idx, None, None)
    }

    /// Execute a reserved ProducerStage — never PE-refused (PC progress path).
    pub(crate) fn try_execute_producer(&self, tx_idx: TxIdx) -> Option<TxVersion> {
        self.try_execute_ready(tx_idx, None, None)
    }

    /// True when writer \(w\) has a live Stage (Ready/Executing/Done/Validated).
    /// Aborting is **not** a progress path — refusing behind it deadlocks.
    #[inline]
    pub(crate) fn producer_stage_runnable(&self, writer: TxIdx) -> bool {
        self.is_done(writer)
            || self.is_ready(writer)
            || self.is_executing(writer)
            || self.is_validated(writer)
    }

    /// OCC-class execute: no ReadyEdge probe. Used for ungated txs and for
    /// gated txs whose edge is already open (`may_execute`).
    #[inline]
    fn try_occ_execute(&self, tx_idx: TxIdx) -> Option<TxVersion> {
        self.try_execute_ready(tx_idx, None, None)
    }

    /// Dual-path pick helper: ungated = Avoid=noop `try_execute`.
    /// Gated-not-ready is a Detect-edge skip (wave-fill another runnable) —
    /// not a global OCC mode switch.
    #[inline]
    fn try_occ_or_skip_gate(
        &self,
        tx_idx: TxIdx,
        ready: Option<&ReadyEdgeTable>,
    ) -> Option<TxVersion> {
        if let Some(edges) = ready
            && edges.is_gated(tx_idx)
            && !edges.may_execute(tx_idx)
        {
            edges.note_skip_gate(tx_idx);
            return None;
        }
        self.try_occ_execute(tx_idx)
    }

    /// Refuse Execute when CC ReadyEdge is live **and** PC ProducerStage(w)
    /// is runnable. Known consumers (**any** incarnation, including first wave)
    /// wait. If the producer has no Stage, do **not** refuse (v6 deadlock).
    fn try_execute_ready(
        &self,
        tx_idx: TxIdx,
        wave: Option<&WaveParkTable>,
        ready: Option<&ReadyEdgeTable>,
    ) -> Option<TxVersion> {
        if tx_idx < self.block_size {
            let mut tx = index_mutex!(self.transactions_status, tx_idx);
            if tx.status == IncarnationStatus::ReadyToExecute {
                // Ungated / A0: OCC-class pick — no ReadyEdge refuse probe.
                if let Some(edges) = ready
                    && edges.is_gated(tx_idx)
                    && !edges.may_execute(tx_idx)
                {
                    let Some(w) = edges.blocking_producer(tx_idx) else {
                        // Gated, edge not visible yet — do not OCC-steal.
                        return None;
                    };
                    // PC-W1: already refused this A1 head — do not re-hammer
                    // until pred completion wakes it.
                    if edges.is_sleeping(tx_idx) {
                        return None;
                    }
                    // Refuse while producer is Executing **or Ready** (prefer-admit
                    // ProducerStage(w) — no ReadyCanary Execute). Validated/Done
                    // already published; Aborting is not a progress path (v6 hang).
                    // First wave (inc==0) must refuse too — SoT schedule-first Avoid.
                    if self.is_executing(w) || self.is_ready(w) {
                        edges.defer(tx_idx);
                        if let Some(wave) = wave {
                            drop(tx);
                            // Wave-admit the predecessor; do **not** fetch_min
                            // execution_idx (that thrashes independents on a
                            // long WAW spine / many same-from pairs).
                            self.admit_spine_heat(w, wave, true);
                        }
                        return None;
                    }
                    if let Some(wave) = wave {
                        self.admit_spine_heat(w, wave, true);
                    }
                    // Fall through: Aborting/Validated canary; ProducerStage reserved.
                }
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
    pub(crate) fn next_task(&self) -> Option<Task> {
        self.next_task_with_wave(None)
    }

    pub(crate) fn next_task_with_wave(&self, wave: Option<&WaveParkTable>) -> Option<Task> {
        self.next_task_with_wave_ready(wave, None)
    }

    /// SpecFence host walk over RunnableSet. Gated holes are Detect
    /// edges: refuse and wave-fill the next independent (Avoid=noop Opt).
    /// This is **not** `next_occ_task`. Independents keep issuing while
    /// OrderedAdmit waiters sleep.
    pub(crate) fn next_task_with_wave_ready(
        &self,
        wave: Option<&WaveParkTable>,
        ready: Option<&ReadyEdgeTable>,
    ) -> Option<Task> {
        if let Some(wave) = wave {
            // After park: prefer wave ready + one cautious execution steal so the core
            // does not idle when Ready work exists (avoid validation-first stampede).
            if wave.steal_after_park_pending() {
                if let Some(task) = self.next_task_steal_after_park_ready(wave, ready) {
                    return Some(task);
                }
            }
            while let Some(tx_idx) = wave.pop_ready() {
                if let Some(tx_version) = self.try_occ_or_skip_gate(tx_idx, ready) {
                    wave.note_ready_steal_if_after_park();
                    return Some(Task::Execution(tx_version));
                }
            }
        }
        while !self.aborted.load(Ordering::Relaxed) {
            let execution_idx = self.execution_idx.load(Ordering::Relaxed);
            let validation_idx = self.validation_idx.load(Ordering::Relaxed);
            if execution_idx >= self.block_size && validation_idx >= self.block_size {
                // Re-check wave ready before yield — a producer may have just pushed.
                if let Some(edges) = ready {
                    // I1: scheduler-Done pred with a missing ReadyEdge stamp
                    // (O5 skip / flush race) must not refuse forever.
                    edges.heal_finished_preds(|w| self.is_done(w));
                }
                if let Some(wave) = wave {
                    if let Some(edges) = ready {
                        let _ = edges.wake_ready_sleepers(wave);
                    }
                    while let Some(tx_idx) = wave.pop_ready() {
                        if let Some(tx_version) = self.try_occ_or_skip_gate(tx_idx, ready) {
                            wave.note_ready_steal_if_after_park();
                            return Some(Task::Execution(tx_version));
                        }
                    }
                }
                let waiting = ready.is_some_and(|e| e.has_sleeping_waiters());
                let busy =
                    waiting && ready.is_some_and(|e| e.sleeper_pred_busy(|w| self.is_executing(w)));
                let validated_done = self.num_validated.load(Ordering::Relaxed)
                    >= self.block_size - self.min_validation_idx.load(Ordering::Relaxed);
                // C4: a short-ℓ sleeper must not skip min_runnable. Independents
                // and a Ready pred lost from the wave stay issuable. Only skip
                // the scan while the sleeper's pred is actually Executing.
                if !busy {
                    match self.min_runnable(ready) {
                        MinRun::Hit(idx) => {
                            self.execution_idx.fetch_min(idx, Ordering::Relaxed);
                            if let Some(wave) = wave {
                                wave.push_ready(idx);
                            }
                            continue;
                        }
                        MinRun::Busy => {}
                        MinRun::Empty if validated_done && !self.has_undone() => {
                            break;
                        }
                        MinRun::Empty => {}
                    }
                }
                // O6: spin only while a sleeper's pred is actually Executing.
                // Sleeping on a not-yet-started hole is skip, not busy-wait.
                // Instant idle ≠ ĉ (S2).
                let idle_t0 = Instant::now();
                if busy {
                    for _ in 0..16 {
                        std::hint::spin_loop();
                    }
                }
                thread::yield_now();
                let ns = idle_t0.elapsed().as_nanos() as u64;
                if let Some(edges) = ready {
                    edges.add_yield_ns(ns);
                    if profile_timing_enabled() {
                        edges.add_idle_ns(ns);
                    }
                }
                continue;
            }

            // OCC-class collaborative execute. Gated-not-ready is a hole:
            // fetch_max past it and keep issuing independents. Do **not**
            // mutex-defer / admit-spine / 32-wide fill on every skip.
            if execution_idx < self.block_size {
                if let Some(edges) = ready
                    && edges.is_gated(execution_idx)
                    && !edges.may_execute(execution_idx)
                {
                    edges.note_skip_gate(execution_idx);
                    self.execution_idx
                        .fetch_max(execution_idx + 1, Ordering::Relaxed);
                    continue;
                }
                if let Some(tx_version) = self.try_occ_execute(execution_idx) {
                    self.execution_idx
                        .fetch_max(execution_idx + 1, Ordering::Relaxed);
                    if let Some(wave) = wave {
                        wave.note_ready_steal_if_after_park();
                    }
                    return Some(Task::Execution(tx_version));
                }
            }

            // Prioritize a validation task to minimize re-execution
            if validation_idx < execution_idx {
                let tx_idx = self.validation_idx.fetch_add(1, Ordering::Relaxed);
                if tx_idx < self.block_size {
                    // Steal execution only when the edge is open. Gated holes
                    // stay skipped so validation-first cannot serialize the block.
                    if let Some(edges) = ready
                        && edges.is_gated(tx_idx)
                        && !edges.may_execute(tx_idx)
                    {
                        edges.note_skip_gate(tx_idx);
                        continue;
                    }
                    let mut tx = index_mutex!(self.transactions_status, tx_idx);
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

            // Prioritize execution task (OCC fetch_add; skip gated holes).
            let next_exec = self.execution_idx.fetch_add(1, Ordering::Relaxed);
            if let Some(tx_version) = self.try_occ_or_skip_gate(next_exec, ready) {
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
        self.next_task_steal_after_park_ready(wave, None)
    }

    fn next_task_steal_after_park_ready(
        &self,
        wave: &WaveParkTable,
        ready: Option<&ReadyEdgeTable>,
    ) -> Option<Task> {
        self.next_task_steal_after_park_prefer(wave, None, ready)
    }

    pub(crate) fn next_task_steal_after_park_prefer(
        &self,
        wave: &WaveParkTable,
        prefer: Option<TxIdx>,
        ready: Option<&ReadyEdgeTable>,
    ) -> Option<Task> {
        if let Some(idx) = prefer
            && let Some(tx_version) = self.try_occ_or_skip_gate(idx, ready)
        {
            wave.note_ready_steal_if_after_park();
            return Some(Task::Execution(tx_version));
        }
        while let Some(tx_idx) = wave.pop_ready() {
            if let Some(tx_version) = self.try_occ_or_skip_gate(tx_idx, ready) {
                wave.note_ready_steal_if_after_park();
                return Some(Task::Execution(tx_version));
            }
        }
        let idx = self.execution_idx.fetch_add(1, Ordering::Relaxed);
        if let Some(tx_version) = self.try_occ_or_skip_gate(idx, ready) {
            wave.note_ready_steal_if_after_park();
            return Some(Task::Execution(tx_version));
        }
        self.try_fill_independent_after_refuse(idx, wave, ready)
    }

    /// After `refuse_admit` of `from`, run the next independent.
    /// P3: bag first, then a short window. No full-block mutex scan — that
    /// prepaid the thin A0 path worse than OCC abort. Waiters wake into the bag.
    fn try_fill_independent_after_refuse(
        &self,
        from: TxIdx,
        wave: &WaveParkTable,
        ready: Option<&ReadyEdgeTable>,
    ) -> Option<Task> {
        let Some(edges) = ready else {
            return None;
        };
        if edges.may_execute(from) {
            return None;
        }
        while let Some(tx_idx) = wave.pop_ready() {
            if edges.is_sleeping(tx_idx) {
                continue;
            }
            if let Some(tx_version) = self.try_execute_ready(tx_idx, Some(wave), ready) {
                // P3: bag depth only — never the full-block scan count.
                edges.sample_ready_width(wave.ready_depth().max(1));
                wave.note_ready_steal_if_after_park();
                return Some(Task::Execution(tx_version));
            }
        }
        let end = from.saturating_add(WAVE_FILL_WINDOW).min(self.block_size);
        for cand in (from + 1)..end {
            if cand >= self.block_size || self.is_done(cand) {
                continue;
            }
            if edges.is_gated(cand) && (edges.is_sleeping(cand) || !edges.may_execute(cand)) {
                continue;
            }
            if let Some(tx_version) = self.try_execute_ready(cand, Some(wave), ready) {
                edges.sample_ready_width(wave.ready_depth().max(1));
                wave.note_ready_steal_if_after_park();
                return Some(Task::Execution(tx_version));
            }
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
        drop(tx);
        self.blocked_on[tx_idx].store(blocking_tx_idx, Ordering::Release);

        let mut blocking_dependents = index_mutex!(self.transactions_dependents, blocking_tx_idx);
        blocking_dependents.push(tx_idx);

        true
    }

    /// wait_for_dependency: park `tx_idx` behind `blocking_tx_idx` **without**
    /// `Aborting` or incarnation++. Status stays `Executing` (worker has left)
    /// until the writer finishes and [`Self::set_wait_for_dependency_ready`] restores Ready
    /// at the same incarnation.
    ///
    /// Returns `false` if the writer is already Executed|Validated (race).
    pub(crate) fn add_wait_for_dependency(&self, tx_idx: TxIdx, blocking_tx_idx: TxIdx) -> bool {
        let blocking_tx = index_mutex!(self.transactions_status, blocking_tx_idx);
        if matches!(
            blocking_tx.status,
            IncarnationStatus::Executed | IncarnationStatus::Validated
        ) {
            return false;
        }

        let tx = index_mutex!(self.transactions_status, tx_idx);
        debug_assert_eq!(tx.status, IncarnationStatus::Executing);
        drop(tx);

        let mut wait_for_dependency_waiters =
            index_mutex!(self.wait_for_dependency_waiters, blocking_tx_idx);
        wait_for_dependency_waiters.push(tx_idx);
        true
    }

    /// Wake a WaitForDependency waiter: Ready at the **same** incarnation (no FullAbortReexecute throw).
    fn set_wait_for_dependency_ready(&self, tx_idx: TxIdx) {
        let mut tx = index_mutex!(self.transactions_status, tx_idx);
        if tx.status != IncarnationStatus::Executing {
            return;
        }
        tx.status = IncarnationStatus::ReadyToExecute;
        self.set_done_flag(tx_idx, false);
    }

    /// Iter3 serial-barrier resolve: after validation abort the tx is already
    /// `Aborting`. Park it behind an unfinished writer (`blocking_tx_idx`) so the
    /// FullAbortReexecute runs once writers have published Data — SpecFence-native
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
    /// Iter16: validate-defer — park an *Executed* consumer behind an unfinished
    /// writer without abort/invalidate. On writer finish, wake re-queues Validation
    /// (same incarnation) so RebindOnly / validate-ok can absorb once Data lands.
    /// Only safe when caller has no true_suffix writes (poison risk otherwise).
    /// Returns `false` if the writer is already Executed|Validated (race).
    pub(crate) fn defer_validation_behind(&self, tx_idx: TxIdx, blocking_tx_idx: TxIdx) -> bool {
        let blocking_tx = index_mutex!(self.transactions_status, blocking_tx_idx);
        if matches!(
            blocking_tx.status,
            IncarnationStatus::Executed | IncarnationStatus::Validated
        ) {
            return false;
        }
        {
            let tx = index_mutex!(self.transactions_status, tx_idx);
            // Must still be Executed (or rare Validated recheck) — not Aborting.
            if !matches!(
                tx.status,
                IncarnationStatus::Executed | IncarnationStatus::Validated
            ) {
                return false;
            }
        }
        let mut blocking_dependents = index_mutex!(self.transactions_dependents, blocking_tx_idx);
        blocking_dependents.push(tx_idx);
        true
    }

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
        if tx.status != IncarnationStatus::Aborting {
            return;
        }
        tx.status = IncarnationStatus::ReadyToExecute;
        tx.incarnation += 1;
        self.set_done_flag(tx_idx, false);
        drop(tx);
        self.blocked_on[tx_idx].store(usize::MAX, Ordering::Release);
    }

    /// Drop a scheduler park. Used when the writer is `leftover_passed`
    /// and not inside execute — the claim already committed, so the
    /// waiter must not stay `Aborting` on it.
    pub(crate) fn clear_stale_block(&self, waiter: TxIdx, writer: TxIdx) {
        if waiter >= self.block_size {
            return;
        }
        let cur = self.blocked_on[waiter].load(Ordering::Acquire);
        if cur == writer {
            self.blocked_on[waiter].store(usize::MAX, Ordering::Release);
        }
        if writer < self.block_size {
            let _ = self.detach_dependent(writer, waiter);
        }
    }

    /// Writer still owed by an `Aborting` park. `None` once that writer is
    /// `Executed`/`Validated` or the park was cleared.
    #[inline]
    pub(crate) fn live_block(&self, tx_idx: TxIdx) -> Option<TxIdx> {
        if tx_idx >= self.block_size {
            return None;
        }
        let b = self.blocked_on[tx_idx].load(Ordering::Acquire);
        if b >= self.block_size || self.is_done(b) {
            None
        } else {
            Some(b)
        }
    }

    /// A1/D6: pull the spine writer into the ready queue so WaitFor targets
    /// make progress without OptimisticRead on known essentials.
    pub(crate) fn admit_spine(&self, tx_idx: TxIdx, wave: &WaveParkTable) {
        self.admit_spine_heat(tx_idx, wave, false);
    }

    /// Park-budget admit: `push_ready` always. `fetch_min` only when heat is
    /// off — otherwise execution_idx stays on independents (fan_out Wait).
    pub(crate) fn admit_spine_heat(&self, tx_idx: TxIdx, wave: &WaveParkTable, park_heat: bool) {
        if tx_idx >= self.block_size {
            return;
        }
        wave.push_ready(tx_idx);
        if !park_heat {
            self.execution_idx.fetch_min(tx_idx, Ordering::Relaxed);
        }
    }

    /// S4: admit every unfinished writer on one ℓ (096/097 multi-spine), not
    /// only the closest / max-writers tip.
    pub(crate) fn admit_spine_writers(&self, writers: &[TxIdx], wave: &WaveParkTable) {
        self.admit_spine_writers_heat(writers, wave, false);
    }

    pub(crate) fn admit_spine_writers_heat(
        &self,
        writers: &[TxIdx],
        wave: &WaveParkTable,
        park_heat: bool,
    ) {
        for &w in writers {
            self.admit_spine_heat(w, wave, park_heat);
        }
    }

    /// Lowest runnable ReadyToExecute index. Exhausted-idx safety net so a
    /// skipped-but-open gate or aborted incarnation cannot leave ESTIMATE.
    /// Gated-not-ready holes stay sleeping (not a livelock rewind).
    ///
    /// Single-flight + lock-free done skip: a 37k ERC-20 block must not pay
    /// 8× full mutex walks on every yield.
    fn min_runnable(&self, ready: Option<&ReadyEdgeTable>) -> MinRun {
        if self.ready_scan.swap(true, Ordering::AcqRel) {
            return MinRun::Busy;
        }
        let start = self.ready_hint.load(Ordering::Relaxed);
        let start = if start < self.block_size { start } else { 0 };
        let mut found = None;
        for off in 0..self.block_size {
            let i = start + off;
            let i = if i >= self.block_size {
                i - self.block_size
            } else {
                i
            };
            if self.is_done(i) || self.is_validated(i) {
                continue;
            }
            if self.is_ready(i) && ready.is_none_or(|e| e.may_execute(i)) {
                found = Some(i);
                self.ready_hint
                    .store(i.saturating_add(1), Ordering::Relaxed);
                break;
            }
        }
        self.ready_scan.store(false, Ordering::Release);
        match found {
            Some(i) => MinRun::Hit(i),
            None => MinRun::Empty,
        }
    }

    /// Lock-free: any tx still Ready / Executing / Aborting.
    /// Do not exit pick while an incarnation is unfinished (ERC-20 OCC).
    #[inline]
    fn has_undone(&self) -> bool {
        (0..self.block_size).any(|i| !self.is_done(i) && !self.is_validated(i))
    }

    /// Public unfinished probe for the SpecFence RunnableSet worker.
    #[inline]
    pub(crate) fn has_unfinished(&self) -> bool {
        self.has_undone()
    }

    /// True when this incarnation is `Executed` (needs validate / revalidate).
    #[inline]
    pub(crate) fn is_executed(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size {
            return false;
        }
        let tx = index_mutex!(self.transactions_status, tx_idx);
        tx.status == IncarnationStatus::Executed
    }

    /// Current incarnation version (ledger, not a pick).
    #[inline]
    pub(crate) fn current_version(&self, tx_idx: TxIdx) -> Option<TxVersion> {
        if tx_idx >= self.block_size {
            return None;
        }
        let tx = index_mutex!(self.transactions_status, tx_idx);
        Some(TxVersion {
            tx_idx,
            tx_incarnation: tx.incarnation,
        })
    }

    /// Demote Validated → Executed so Resolve can re-check the read set.
    pub(crate) fn prepare_revalidate(&self, tx_idx: TxIdx) -> Option<TxVersion> {
        if tx_idx >= self.block_size {
            return None;
        }
        let mut tx = index_mutex!(self.transactions_status, tx_idx);
        if tx.status == IncarnationStatus::Validated {
            tx.status = IncarnationStatus::Executed;
            self.num_validated.fetch_sub(1, Ordering::Relaxed);
            self.set_validated_flag(tx_idx, false);
        }
        if tx.status == IncarnationStatus::Executed {
            Some(TxVersion {
                tx_idx,
                tx_incarnation: tx.incarnation,
            })
        } else {
            None
        }
    }

    /// All txs have a Validated stamp (SF block-complete).
    #[inline]
    pub(crate) fn all_validated(&self) -> bool {
        self.num_validated.load(Ordering::Relaxed) >= self.block_size
            || (0..self.block_size).all(|i| self.is_validated(i))
    }

    /// SpecFence validation finish: never returns a Block-STM next task.
    /// Abort leaves the tx Ready; the caller requeues on RunnableSet.
    pub(crate) fn finish_validation_sf(
        &self,
        tx_version: &TxVersion,
        aborted: bool,
        rewind_to: Option<TxIdx>,
    ) {
        if aborted {
            self.set_ready_status(tx_version.tx_idx);
            if let Some(to) = rewind_to {
                let to = to.clamp(tx_version.tx_idx + 1, self.block_size);
                self.validation_idx.fetch_min(to, Ordering::Relaxed);
            }
            return;
        }
        let mut tx = index_mutex!(self.transactions_status, tx_version.tx_idx);
        if tx.status == IncarnationStatus::Executed {
            tx.status = IncarnationStatus::Validated;
            self.num_validated.fetch_add(1, Ordering::Relaxed);
            self.set_validated_flag(tx_version.tx_idx, true);
        }
    }

    /// True when the incarnation is queued `ReadyToExecute` (S1 prefer-admit).
    #[inline]
    pub(crate) fn is_ready(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size {
            return false;
        }
        let tx = index_mutex!(self.transactions_status, tx_idx);
        tx.status == IncarnationStatus::ReadyToExecute
    }

    /// Detach a waiter from a writer's dependents (Data-publish progressive wake).
    pub(crate) fn detach_dependent(&self, writer: TxIdx, waiter: TxIdx) -> bool {
        if writer >= self.block_size {
            return false;
        }
        let mut deps = index_mutex!(self.transactions_dependents, writer);
        let before = deps.len();
        deps.retain(|t| *t != waiter);
        before != deps.len()
    }

    /// Ready an Aborting waiter after Data publish (incarnation++). No-op otherwise.
    pub(crate) fn try_ready_waiter(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size {
            return false;
        }
        let mut tx = index_mutex!(self.transactions_status, tx_idx);
        if tx.status != IncarnationStatus::Aborting {
            return false;
        }
        tx.status = IncarnationStatus::ReadyToExecute;
        tx.incarnation += 1;
        self.set_done_flag(tx_idx, false);
        drop(tx);
        self.blocked_on[tx_idx].store(usize::MAX, Ordering::Release);
        true
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
        let mut wait_for_dependency_drained: SmallVec<[TxIdx; 4]> = SmallVec::new();
        {
            let mut dependents = index_mutex!(self.transactions_dependents, tx_version.tx_idx);
            drained.extend(dependents.drain(..));
        }
        {
            let mut pins = index_mutex!(self.wait_for_dependency_waiters, tx_version.tx_idx);
            wait_for_dependency_drained.extend(pins.drain(..));
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
        // Publish lock-free done/validated before releasing status mutex / waking.
        let is_validated = matches!(tx.status, IncarnationStatus::Validated);
        self.set_done_flag(
            tx_version.tx_idx,
            matches!(
                tx.status,
                IncarnationStatus::Executed | IncarnationStatus::Validated
            ),
        );
        self.set_validated_flag(tx_version.tx_idx, is_validated);

        // Wake after status is Data-ready (`is_done`), still under writer lock so
        // add_dependency cannot lose a waiter between drain and Ready.
        // Iter16: validate-defer waiters stay Executed|Validated — re-queue
        // Validation (same incarnation, no reexec / no invalidate). Aborting
        // dependents still go Ready→reexec (SuffixRepair / FullAbortReexecute).
        for tx_idx in drained {
            let revalidate = {
                let tx = index_mutex!(self.transactions_status, tx_idx);
                matches!(
                    tx.status,
                    IncarnationStatus::Executed | IncarnationStatus::Validated
                )
            };
            if revalidate {
                self.validation_idx.fetch_min(tx_idx, Ordering::Relaxed);
            } else {
                self.set_ready_status(tx_idx);
                self.execution_idx.fetch_min(tx_idx, Ordering::Relaxed);
                if let Some(wave) = wave {
                    wave.push_ready(tx_idx);
                }
            }
        }
        // WaitForDependency: same-incarnation Ready (no Aborting / no incarnation++).
        for tx_idx in wait_for_dependency_drained {
            self.set_wait_for_dependency_ready(tx_idx);
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
            drop(tx);
            // Validation abort is not an `add_dependency` park.
            self.blocked_on[tx_version.tx_idx].store(usize::MAX, Ordering::Release);
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
        unsafe {
            self.done_flags
                .get_unchecked(tx_idx)
                .load(Ordering::Acquire)
        }
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

    /// Hang-trace: incarnation and scheduler deps that name `waiter`.
    pub(crate) fn hang_dep_of(&self, waiter: TxIdx) -> (usize, usize, usize) {
        if waiter >= self.block_size {
            return (0, usize::MAX, usize::MAX);
        }
        let inc = {
            let tx = index_mutex!(self.transactions_status, waiter);
            tx.incarnation
        };
        let mut dep = usize::MAX;
        let mut wfd = usize::MAX;
        for b in 0..self.block_size {
            if dep == usize::MAX {
                let deps = index_mutex!(self.transactions_dependents, b);
                if deps.iter().any(|&t| t == waiter) {
                    dep = b;
                }
            }
            if wfd == usize::MAX {
                let waits = index_mutex!(self.wait_for_dependency_waiters, b);
                if waits.iter().any(|&t| t == waiter) {
                    wfd = b;
                }
            }
            if dep != usize::MAX && wfd != usize::MAX {
                break;
            }
        }
        (inc, dep, wfd)
    }

    /// Lost-wakeup recover: `Aborting` → Ready (incarnation++).
    #[inline]
    pub(crate) fn recover_aborting(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size || !self.is_aborting(tx_idx) {
            return false;
        }
        self.set_ready_status(tx_idx);
        self.is_ready(tx_idx)
    }

    /// WaitForDependency leftover: status stayed `Executing` after the worker
    /// returned `Blocked`. Same incarnation → Ready.
    #[inline]
    pub(crate) fn recover_executing_waiter(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size {
            return false;
        }
        let mut tx = index_mutex!(self.transactions_status, tx_idx);
        if tx.status != IncarnationStatus::Executing {
            return false;
        }
        tx.status = IncarnationStatus::ReadyToExecute;
        self.set_done_flag(tx_idx, false);
        true
    }

    #[inline]
    fn set_done_flag(&self, tx_idx: TxIdx, done: bool) {
        // SAFETY: callers only use inbound tx indices.
        unsafe {
            self.done_flags
                .get_unchecked(tx_idx)
                .store(done, Ordering::Release);
            // Clearing done always clears validated; setting done alone does not
            // publish Validated (Executed path sets validated=false explicitly).
            if !done {
                self.validated_flags
                    .get_unchecked(tx_idx)
                    .store(false, Ordering::Release);
            }
        }
    }

    #[inline]
    fn set_validated_flag(&self, tx_idx: TxIdx, validated: bool) {
        // SAFETY: callers only use inbound tx indices.
        unsafe {
            self.validated_flags
                .get_unchecked(tx_idx)
                .store(validated, Ordering::Release);
        }
    }

    /// True when status is `Validated` (stronger than [`Self::is_done`]).
    /// Iter12: 2nd-repair / serial-barrier spin for Validated to cut ESTIMATE races
    /// without SoftWait Soft park tax.
    #[inline]
    pub(crate) fn is_validated(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.block_size {
            return true;
        }
        // SAFETY: tx_idx checked against block_size above.
        unsafe {
            self.validated_flags
                .get_unchecked(tx_idx)
                .load(Ordering::Acquire)
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
                // Iter12: publish Validated for ESTIMATE-race spins (lock-free).
                self.set_validated_flag(tx_version.tx_idx, true);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specfence::ReadyEdgeTable;

    #[test]
    fn refuse_known_consumer_on_incarnation_zero() {
        let s = Scheduler::new(4);
        let ready = ReadyEdgeTable::new();
        ready.note_consumer(2, 0);
        let v0 = s.try_execute(0).expect("producer");
        assert_eq!(v0.tx_idx, 0);
        assert!(s.is_executing(0));
        assert!(
            s.try_execute_ready(2, None, Some(&ready)).is_none(),
            "first-wave (inc==0) must refuse while ProducerStage(w) Executing"
        );
        assert!(ready.refuse_count() >= 1);
    }

    #[test]
    fn dual_path_occ_pick_while_gated_hole() {
        let s = Scheduler::new(6);
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        ready.note_consumer(1, 0);
        let first = s
            .next_task_with_wave_ready(Some(&wave), Some(&ready))
            .expect("producer");
        let Task::Execution(v0) = first else {
            panic!("expected Execution");
        };
        assert_eq!(v0.tx_idx, 0);
        let second = s
            .next_task_with_wave_ready(Some(&wave), Some(&ready))
            .expect("S1: ungated 2 must issue while 1 is gated");
        let Task::Execution(v) = second else {
            panic!("expected Execution, got {second:?}");
        };
        assert_eq!(v.tx_idx, 2, "gated hole must not stall independent pick");
        assert!(ready.skip_gate_n() >= 1);
        assert!(s.try_execute_ready(1, Some(&wave), Some(&ready)).is_none());
    }

    #[test]
    fn refuse_wave_fill_runs_independent() {
        let s = Scheduler::new(6);
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        ready.note_consumer(1, 0);
        let first = s
            .next_task_with_wave_ready(Some(&wave), Some(&ready))
            .expect("producer");
        let Task::Execution(v0) = first else {
            panic!("expected Execution");
        };
        assert_eq!(v0.tx_idx, 0);
        let second = s
            .next_task_with_wave_ready(Some(&wave), Some(&ready))
            .expect("wave fill must not idle on refused consumer 1");
        let Task::Execution(v) = second else {
            panic!("expected Execution, got {second:?}");
        };
        assert_eq!(
            v.tx_idx, 2,
            "refuse 1 while 0 Executing must steal independent 2"
        );
        assert_eq!(ready.refuse_count(), 1, "defer is idempotent");
        assert!(s.try_execute_ready(1, Some(&wave), Some(&ready)).is_none());
        assert_eq!(
            ready.refuse_count(),
            1,
            "second refuse of the same consumer must not spin-count"
        );
    }

    #[test]
    fn short_hole_does_not_block_independent_min_runnable() {
        let s = Scheduler::new(4);
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        ready.note_consumer(1, 0);
        ready.note_skip_gate(1);
        assert!(ready.has_sleeping_waiters());
        assert!(s.is_ready(0), "pred still Ready");
        let first = s
            .next_task_with_wave_ready(Some(&wave), Some(&ready))
            .expect("C4: sleeper on 1 must not hide Ready pred 0");
        let Task::Execution(v0) = first else {
            panic!("expected Execution");
        };
        assert_eq!(v0.tx_idx, 0);
        let second = s
            .next_task_with_wave_ready(Some(&wave), Some(&ready))
            .expect("C4: independent 2 issues while 1 sleeps");
        let Task::Execution(v2) = second else {
            panic!("expected Execution, got {second:?}");
        };
        assert_eq!(v2.tx_idx, 2);
    }

    #[test]
    fn heal_done_pred_wakes_late_flush_sleeper() {
        let s = Scheduler::new(3);
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let v0 = s.try_execute(0).expect("pred");
        let _ = s.finish_execution(v0, FinishExecFlags::empty());
        let v2 = s.try_execute(2).expect("independent");
        let _ = s.finish_execution(v2, FinishExecFlags::empty());
        assert!(s.is_done(0));
        // Late pick-quantum plant after the pred already published (O5 race).
        ready.note_consumer(1, 0);
        ready.note_skip_gate(1);
        assert!(!ready.may_execute(1));
        let mut saw = None;
        for _ in 0..16 {
            match s.next_task_with_wave_ready(Some(&wave), Some(&ready)) {
                Some(Task::Execution(v)) => {
                    saw = Some(v.tx_idx);
                    break;
                }
                Some(Task::Validation(v)) => {
                    let _ = s.finish_validation(&v, false);
                }
                None => break,
            }
        }
        assert_eq!(
            saw,
            Some(1),
            "I1: heal+wake must issue the late-gated successor"
        );
    }

    #[test]
    fn refuse_known_consumer_while_producer_ready() {
        let s = Scheduler::new(4);
        let ready = ReadyEdgeTable::new();
        ready.note_consumer(2, 0);
        assert!(s.is_ready(0), "producer starts ReadyToExecute");
        assert!(
            s.try_execute_ready(2, None, Some(&ready)).is_none(),
            "known consumer must not ReadyCanary while producer is still Ready"
        );
        assert!(ready.refuse_count() >= 1);
        assert!(
            s.try_execute_ready(0, None, Some(&ready)).is_some(),
            "prefer-admit ProducerStage(w) still runs"
        );
    }

    #[test]
    fn wait_for_dependency_does_not_abort_or_bump_incarnation() {
        let s = Scheduler::new(3);
        let p = s.try_execute(0).unwrap();
        let c = s.try_execute(1).unwrap();
        assert_eq!(c.tx_incarnation, 0);
        assert!(s.add_wait_for_dependency(1, 0));
        assert!(
            s.is_executing(1),
            "WaitForDependency must not mark Aborting"
        );
        assert!(!s.is_aborting(1));
        let _ = s.finish_execution(
            TxVersion {
                tx_idx: p.tx_idx,
                tx_incarnation: p.tx_incarnation,
            },
            FinishExecFlags::empty(),
        );
        assert!(s.is_ready(1));
        let again = s.try_execute(1).expect("same-incarnation resume");
        assert_eq!(
            again.tx_incarnation, 0,
            "wait_for_dependency keeps incarnation"
        );
    }

    #[test]
    fn wait_for_dependency_loses_race_when_writer_done() {
        let s = Scheduler::new(2);
        let p = s.try_execute(0).unwrap();
        let _ = s.finish_execution(
            TxVersion {
                tx_idx: p.tx_idx,
                tx_incarnation: p.tx_incarnation,
            },
            FinishExecFlags::empty(),
        );
        let _ = s.try_execute(1).unwrap();
        assert!(
            !s.add_wait_for_dependency(1, 0),
            "writer already Done must not wait_for_dependency forever"
        );
    }
}
