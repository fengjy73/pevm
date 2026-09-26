//! Index-seeded Chase-Lev scheduler and the commit prefix.
//!
//! Tasks are transaction indexes. Each worker owns one deque. The owner pops
//! LIFO, thieves pop FIFO. Status transitions sit behind one mutex so a park
//! and a publish cannot lose a wakeup. The mutex is not held across the EVM.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::TxIdx;

use super::deque::LocalDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Ready,
    Executing,
    Executed,
    Committed,
    /// Claimed by the worker that published the previous read-modify-write.
    /// Not on a stealable deque. The owner takes it without a wakeup.
    Sticky,
    /// `until_final` waits until the predecessor's incarnation is
    /// dependency-closed. Execution alone is not enough: a published
    /// incarnation can still be aborted. Commit is not required.
    Parked {
        pred: TxIdx,
        until_final: bool,
    },
}

struct TxState {
    incarnation: usize,
    phase: Phase,
    queued: bool,
}

struct Inner {
    status: Vec<TxState>,
    dependents: Vec<Vec<TxIdx>>,
    /// Non-stealable handoff. Only the owning worker pops it.
    sticky: Vec<Vec<TxIdx>>,
}

pub(crate) struct Runtime {
    n: usize,
    inner: Mutex<Inner>,
    deques: Vec<LocalDeque>,
    committed: AtomicUsize,
    /// `usize::MAX` means this transaction's current incarnation is not final.
    /// A stored incarnation is final: execution finished, its reads came from
    /// final origins or storage, and validation kept that incarnation.
    validated: Vec<AtomicUsize>,
    executing: AtomicUsize,
    waiting: AtomicUsize,
    mu: Mutex<()>,
    /// Idle workers inside the active set wait here.
    cv: Condvar,
    /// Workers outside the active set wait here, so a work signal cannot
    /// land on a parked thread and get lost.
    sleep_cv: Condvar,
    abort: AtomicBool,
    workers: usize,
    /// How many workers may steal and run ordinary tasks. Starts at 1 and
    /// moves from idle ratio and chain wait. Never exceeds `workers`.
    active: AtomicUsize,
    peak_active: AtomicUsize,
    parked: AtomicUsize,
    woken: AtomicUsize,
    idle_window: AtomicUsize,
    busy_window: AtomicUsize,
    /// Half-life moving averages, nanoseconds. Diagnostics only: a hop gap
    /// does not change the active set.
    gap_ema: AtomicU64,
    exec_ema: AtomicU64,
    /// Ready-queue width, in 1/256 of a transaction. The active set follows this.
    width_ema_q8: AtomicU64,
    grow_streak: AtomicUsize,
    shrink_streak: AtomicUsize,
    control_tick: AtomicUsize,
    ready_depth: AtomicUsize,
    /// Tasks moved off a worker that is staying on a chain handoff.
    injector: Mutex<Vec<TxIdx>>,
    /// Active-set size at each control decision, in order, capped.
    active_samples: Mutex<Vec<u16>>,
    /// Bumped under `mu` on every wake of the work condvar.
    work_seq: AtomicUsize,
    sticky_flag: Vec<AtomicBool>,
}

impl Runtime {
    pub(crate) fn new(n: usize, workers: usize) -> Self {
        let workers = workers.max(1);
        Self {
            n,
            inner: Mutex::new(Inner {
                status: (0..n)
                    .map(|_| TxState {
                        incarnation: 0,
                        phase: Phase::Ready,
                        queued: false,
                    })
                    .collect(),
                dependents: (0..n).map(|_| Vec::new()).collect(),
                sticky: (0..workers).map(|_| Vec::new()).collect(),
            }),
            deques: (0..workers).map(|_| LocalDeque::with_capacity(n)).collect(),
            committed: AtomicUsize::new(0),
            validated: (0..n).map(|_| AtomicUsize::new(usize::MAX)).collect(),
            executing: AtomicUsize::new(0),
            waiting: AtomicUsize::new(0),
            mu: Mutex::new(()),
            cv: Condvar::new(),
            sleep_cv: Condvar::new(),
            abort: AtomicBool::new(false),
            workers,
            active: AtomicUsize::new(1),
            peak_active: AtomicUsize::new(1),
            parked: AtomicUsize::new(0),
            woken: AtomicUsize::new(0),
            idle_window: AtomicUsize::new(0),
            busy_window: AtomicUsize::new(0),
            gap_ema: AtomicU64::new(0),
            exec_ema: AtomicU64::new(1),
            width_ema_q8: AtomicU64::new(0),
            grow_streak: AtomicUsize::new(0),
            shrink_streak: AtomicUsize::new(0),
            control_tick: AtomicUsize::new(0),
            ready_depth: AtomicUsize::new(0),
            injector: Mutex::new(Vec::new()),
            active_samples: Mutex::new(Vec::new()),
            work_seq: AtomicUsize::new(0),
            sticky_flag: (0..workers).map(|_| AtomicBool::new(false)).collect(),
        }
    }

    /// Strided index seeding. Worker `w` owns `w, w+C, w+2C, ...`, pushed
    /// high-to-low so the owner LIFO runs the lowest index first. The first
    /// wave is transactions `0..C`, which lets a class head publish before
    /// later strides start.
    ///
    /// A commit window that hid the tail was measured on 15274915. Readers
    /// three transactions apart still aborted, and the hot-chain gap grew,
    /// so the whole block stays on the deques.
    pub(crate) fn seed(&self) {
        let mut inner = self.inner.lock().unwrap();
        for w in 0..self.workers {
            let mut owned = Vec::new();
            let mut tx = w;
            while tx < self.n {
                owned.push(tx);
                tx += self.workers;
            }
            for tx in owned.into_iter().rev() {
                inner.status[tx].queued = true;
                self.ready_depth.fetch_add(1, Ordering::Relaxed);
                self.deques[w].push_bottom(tx);
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn worker_count(&self) -> usize {
        self.workers
    }

    pub(crate) fn committed(&self) -> usize {
        self.committed.load(Ordering::Acquire)
    }

    /// One worker already ran `tx` to completion. Nonce checks read this phase.
    pub(crate) fn commit_serial(&self, tx: TxIdx) {
        if tx >= self.n {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.status[tx].phase = Phase::Committed;
        self.validated[tx].store(0, Ordering::Release);
        self.committed.store(tx + 1, Ordering::Release);
    }

    pub(crate) fn request_abort(&self) {
        self.abort.store(true, Ordering::Release);
        let _guard = self.mu.lock().unwrap();
        self.work_seq.fetch_add(1, Ordering::Release);
        self.cv.notify_all();
        self.sleep_cv.notify_all();
    }

    pub(crate) fn work_ticket(&self) -> usize {
        self.work_seq.load(Ordering::Acquire)
    }

    pub(crate) fn active_now(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn peak_active(&self) -> usize {
        self.peak_active.load(Ordering::Relaxed)
    }

    pub(crate) fn parked_count(&self) -> usize {
        self.parked.load(Ordering::Relaxed)
    }

    pub(crate) fn woken_count(&self) -> usize {
        self.woken.load(Ordering::Relaxed)
    }

    pub(crate) fn ready_now(&self) -> usize {
        self.ready_depth.load(Ordering::Relaxed)
    }

    pub(crate) fn note_busy(&self) {
        self.busy_window.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_idle_sample(&self) {
        self.idle_window.fetch_add(1, Ordering::Relaxed);
    }

    /// Chain-hop gap and the execution that just finished, both in nanoseconds.
    ///
    /// The averages stay for the trace. They do not shrink the active set:
    /// one hot chain's wait is not a measure of how much other work is ready.
    pub(crate) fn observe_hop_time(&self, gap_ns: u64, exec_ns: u64) {
        fn ema(slot: &AtomicU64, sample: u64) {
            let prev = slot.load(Ordering::Relaxed);
            let next = if prev == 0 {
                sample
            } else {
                prev / 2 + sample / 2
            };
            slot.store(next, Ordering::Relaxed);
        }
        ema(&self.gap_ema, gap_ns);
        ema(&self.exec_ema, exec_ns.max(1));
    }

    /// Active-set samples taken at control decisions, in order.
    pub(crate) fn active_samples(&self) -> Vec<u16> {
        self.active_samples.lock().unwrap().clone()
    }

    fn record_sample(&self, active: usize) {
        let mut log = self.active_samples.lock().unwrap();
        // Keep the whole block. When the log fills, drop every other sample
        // so the tail is still visible instead of only the opening.
        if log.len() == 64 {
            let kept: Vec<u16> = log.iter().copied().step_by(2).collect();
            *log = kept;
        }
        log.push(active as u16);
    }

    /// Move this worker's stealable tasks onto the shared injector.
    ///
    /// Called when the worker is about to stay on a chain handoff. Those
    /// tasks would otherwise sit under the handoff until this worker popped
    /// again. Only the owner pops its bottom, so the move is safe.
    pub(crate) fn spill_owned(&self, worker: usize) {
        if worker >= self.workers {
            return;
        }
        let mut moved = Vec::new();
        while let Some(tx) = self.deques[worker].pop_bottom() {
            moved.push(tx);
        }
        if moved.is_empty() {
            return;
        }
        self.injector.lock().unwrap().extend(moved);
        self.poke_work();
    }

    fn pop_injector(&self) -> Option<TxIdx> {
        self.injector.lock().unwrap().pop()
    }

    /// Grow and shrink from the measured ready width.
    ///
    /// The width is an exponential moving average of the ready queue. One
    /// sample above the set grows it. Shrinking waits for two decisions where
    /// the average sits at least two below the set and the live queue is
    /// smaller than the set. A chain hop is not an input.
    pub(crate) fn maybe_grow(&self) {
        let ready = self.ready_now();
        let prev = self.width_ema_q8.load(Ordering::Relaxed);
        let sample = (ready as u64).saturating_mul(256);
        let next = if prev == 0 {
            sample
        } else {
            prev - prev / 4 + sample / 4
        };
        self.width_ema_q8.store(next, Ordering::Relaxed);

        let tick = self.control_tick.fetch_add(1, Ordering::Relaxed);
        let period = self.active.load(Ordering::Relaxed).clamp(1, 8);
        if tick % period != period - 1 {
            return;
        }
        let width = ((next + 128) / 256) as usize;
        let mut active = self.active.load(Ordering::Relaxed);
        let cap = self.workers;
        if width > active {
            self.shrink_streak.store(0, Ordering::Relaxed);
            self.grow_streak.fetch_add(1, Ordering::Relaxed);
            active = width.min(cap);
        } else if active > 1 && width + 1 < active && ready < active {
            self.grow_streak.store(0, Ordering::Relaxed);
            let streak = self.shrink_streak.fetch_add(1, Ordering::Relaxed) + 1;
            if streak >= 2 {
                let floor = ready.max(1);
                active = width.max(floor).min(active - 1).max(1);
                self.shrink_streak.store(0, Ordering::Relaxed);
            }
        } else {
            self.grow_streak.store(0, Ordering::Relaxed);
            self.shrink_streak.store(0, Ordering::Relaxed);
        }
        active = active.clamp(1, cap);
        let guard = self.mu.lock().unwrap();
        let prev_active = self.active.swap(active, Ordering::Release);
        self.peak_active.fetch_max(active, Ordering::Relaxed);
        if active > prev_active {
            self.sleep_cv.notify_all();
        }
        drop(guard);
        self.record_sample(active);
    }

    /// Park until this worker is inside the active set or holds a chain handoff.
    /// Returns true when the block should stop.
    pub(crate) fn wait_inactive(&self, worker: usize) -> bool {
        let mut guard = self.mu.lock().unwrap();
        loop {
            if self.aborted() || self.committed() >= self.n {
                return true;
            }
            let active = self.active.load(Ordering::Acquire);
            let sticky = self
                .sticky_flag
                .get(worker)
                .is_some_and(|flag| flag.load(Ordering::Acquire));
            if worker < active || sticky {
                return false;
            }
            self.parked.fetch_add(1, Ordering::Relaxed);
            guard = self.sleep_cv.wait(guard).unwrap();
            self.woken.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Park an active worker that found nothing to run. Not a spin.
    ///
    /// `ticket` is `work_ticket()` from before the last empty pop. A wake that
    /// landed in between returns immediately instead of sleeping the timeout.
    pub(crate) fn wait_work(&self, ticket: usize) {
        let guard = self.mu.lock().unwrap();
        if self.work_seq.load(Ordering::Acquire) != ticket
            || self.aborted()
            || self.committed() >= self.n
        {
            return;
        }
        self.parked.fetch_add(1, Ordering::Relaxed);
        let _ = self.cv.wait_timeout(guard, Duration::from_millis(200));
        self.woken.fetch_add(1, Ordering::Relaxed);
    }

    fn poke_work(&self) {
        let _guard = self.mu.lock().unwrap();
        self.work_seq.fetch_add(1, Ordering::Release);
        self.cv.notify_one();
    }

    fn dec_ready(&self) {
        let _ = self
            .ready_depth
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_sub(1))
            });
    }

    pub(crate) fn aborted(&self) -> bool {
        self.abort.load(Ordering::Acquire)
    }

    pub(crate) fn executing_add(&self, delta: isize) {
        if delta > 0 {
            self.executing.fetch_add(delta as usize, Ordering::Relaxed);
        } else {
            self.executing
                .fetch_sub((-delta) as usize, Ordering::Relaxed);
        }
    }

    pub(crate) fn waiting_add(&self, delta: isize) {
        if delta > 0 {
            self.waiting.fetch_add(delta as usize, Ordering::Relaxed);
        } else {
            self.waiting.fetch_sub((-delta) as usize, Ordering::Relaxed);
        }
    }

    pub(crate) fn all_executors_waiting(&self) -> bool {
        let waiting = self.waiting.load(Ordering::Relaxed);
        let executing = self.executing.load(Ordering::Relaxed);
        executing > 0 && waiting >= executing
    }

    pub(crate) fn wait_brief(&self) {
        let guard = self.mu.lock().unwrap();
        let _ = self.cv.wait_timeout(guard, Duration::from_micros(50));
    }

    #[allow(dead_code)]
    pub(crate) fn notify(&self) {
        let _guard = self.mu.lock().unwrap();
        self.work_seq.fetch_add(1, Ordering::Release);
        self.cv.notify_all();
        self.sleep_cv.notify_all();
    }

    pub(crate) fn is_executing(&self, tx: TxIdx) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .status
            .get(tx)
            .is_some_and(|s| s.phase == Phase::Executing)
    }

    pub(crate) fn is_committed(&self, tx: TxIdx) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .status
            .get(tx)
            .is_some_and(|s| s.phase == Phase::Committed)
    }

    /// Not executed and not committed. Prediction skips writers that already finished.
    pub(crate) fn still_open(&self, tx: TxIdx) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.status.get(tx).is_some_and(|s| {
            matches!(
                s.phase,
                Phase::Ready | Phase::Executing | Phase::Sticky | Phase::Parked { .. }
            )
        })
    }

    pub(crate) fn incarnation(&self, tx: TxIdx) -> usize {
        self.inner.lock().unwrap().status[tx].incarnation
    }

    /// This incarnation finished, its origins are final, and it has not been aborted.
    pub(crate) fn is_final_inc(&self, tx: TxIdx, inc: usize) -> bool {
        inc != usize::MAX
            && self
                .validated
                .get(tx)
                .is_some_and(|slot| slot.load(Ordering::Acquire) == inc)
    }

    pub(crate) fn is_final_any(&self, tx: TxIdx) -> bool {
        self.validated
            .get(tx)
            .is_some_and(|slot| slot.load(Ordering::Acquire) != usize::MAX)
    }

    fn pred_final_locked(&self, inner: &Inner, pred: TxIdx) -> bool {
        let inc = inner.status[pred].incarnation;
        self.is_final_inc(pred, inc)
    }

    /// Record that `inc` is the final incarnation. Fails if this transaction
    /// was aborted or re-queued since the caller observed `inc`.
    pub(crate) fn mark_validated(&self, tx: TxIdx, inc: usize) -> bool {
        if tx >= self.n || inc == usize::MAX {
            return false;
        }
        let inner = self.inner.lock().unwrap();
        let st = &inner.status[tx];
        if st.incarnation != inc {
            return false;
        }
        if !matches!(st.phase, Phase::Executed | Phase::Committed) {
            return false;
        }
        self.validated[tx].store(inc, Ordering::Release);
        true
    }

    pub(crate) fn is_executed(&self, tx: TxIdx) -> bool {
        let inner = self.inner.lock().unwrap();
        matches!(inner.status[tx].phase, Phase::Executed | Phase::Committed)
            && inner.status[tx].phase == Phase::Executed
    }

    /// Drop an in-flight attempt without queueing it again.
    ///
    /// Used when the blocking predecessor has already finished. Requeueing
    /// that transaction on the owner deque starves the commit prefix.
    pub(crate) fn defer(&self, tx: TxIdx) {
        let mut inner = self.inner.lock().unwrap();
        if tx >= self.n {
            return;
        }
        if inner.status[tx].phase == Phase::Executing {
            inner.status[tx].phase = Phase::Ready;
            inner.status[tx].queued = false;
        }
    }

    pub(crate) fn pop(&self, worker: usize) -> Option<(TxIdx, usize)> {
        let limit = self.n.saturating_mul(2).max(64);
        for _ in 0..limit {
            let tx = self.deques[worker]
                .pop_bottom()
                .or_else(|| {
                    let n = self.workers;
                    let mut found = None;
                    for k in 1..n {
                        let victim = (worker + k) % n;
                        if let Some(tx) = self.deques[victim].pop_top() {
                            found = Some(tx);
                            break;
                        }
                    }
                    found
                })
                .or_else(|| self.pop_injector())?;
            let mut inner = self.inner.lock().unwrap();
            if tx >= self.n {
                continue;
            }
            let was_queued = inner.status[tx].queued;
            inner.status[tx].queued = false;
            if inner.status[tx].phase == Phase::Ready {
                inner.status[tx].phase = Phase::Executing;
                if was_queued {
                    self.dec_ready();
                }
                let inc = inner.status[tx].incarnation;
                return Some((tx, inc));
            }
        }
        None
    }

    /// The blocking worker gives `tx` to the chain owner. No public deque entry.
    ///
    /// Returns false when `owner` is outside the active set: a parked worker
    /// must not be woken just to take the handoff.
    pub(crate) fn handoff_sticky(&self, owner: usize, tx: TxIdx) -> bool {
        if owner >= self.workers || tx >= self.n {
            return false;
        }
        if owner >= self.active.load(Ordering::Acquire) {
            return false;
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.status[tx].phase != Phase::Executing {
            return false;
        }
        if inner.status[tx].queued {
            inner.status[tx].queued = false;
            self.dec_ready();
        }
        inner.status[tx].phase = Phase::Sticky;
        inner.sticky[owner].push(tx);
        self.sticky_flag[owner].store(true, Ordering::Release);
        drop(inner);
        // The owner is inside the active set. If it is idle it waits on `cv`,
        // not on the inactive-worker condvar. The sequence is published under
        // the same mutex the waiter holds, so the wake cannot land early and
        // be lost.
        let _guard = self.mu.lock().unwrap();
        self.work_seq.fetch_add(1, Ordering::Release);
        self.cv.notify_all();
        true
    }

    /// Finish `tx` and move its chain successors onto this worker's private slot.
    ///
    /// The slot is not a deque entry, so a thief cannot take the next
    /// read-modify-write, and a parked worker is not woken to run it.
    pub(crate) fn finish_and_stick(&self, worker: usize, tx: TxIdx, successors: &[TxIdx]) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if tx >= self.n || inner.status[tx].phase != Phase::Executing {
            return false;
        }
        inner.status[tx].phase = Phase::Executed;
        let deps = std::mem::take(&mut inner.dependents[tx]);
        let mut woke = false;
        for dep in deps {
            if successors.contains(&dep) {
                continue;
            }
            match inner.status[dep].phase {
                Phase::Parked {
                    until_final: false, ..
                } => {
                    inner.status[dep].phase = Phase::Ready;
                    super::timeline::note_wake(dep);
                    woke |= self.enqueue_locked(&mut inner, worker, dep);
                }
                Phase::Parked {
                    until_final: true, ..
                } => inner.dependents[tx].push(dep),
                _ => {}
            }
        }
        for &succ in successors.iter().rev() {
            self.claim_sticky_locked(&mut inner, worker, succ);
        }
        drop(inner);
        if woke {
            self.poke_work();
        }
        true
    }

    fn claim_sticky_locked(&self, inner: &mut Inner, worker: usize, tx: TxIdx) -> bool {
        if tx >= self.n || worker >= self.workers {
            return false;
        }
        match inner.status[tx].phase {
            Phase::Ready | Phase::Parked { .. } => {
                if inner.status[tx].queued {
                    inner.status[tx].queued = false;
                    self.dec_ready();
                }
                inner.status[tx].phase = Phase::Sticky;
                inner.sticky[worker].push(tx);
                self.sticky_flag[worker].store(true, Ordering::Release);
                true
            }
            _ => false,
        }
    }

    /// Park a still-queued transaction before any worker starts.
    pub(crate) fn hold_ready(&self, tx: TxIdx, pred: TxIdx, until_final: bool) -> bool {
        {
            let inner = self.inner.lock().unwrap();
            if tx >= self.n || inner.status[tx].phase != Phase::Ready {
                return false;
            }
        }
        self.park(0, tx, pred, false, until_final);
        true
    }

    /// Put Ready successors on this worker's private slot. Thieves skip them.
    #[cfg(test)]
    pub(crate) fn stick(&self, worker: usize, txs: &[TxIdx]) {
        if txs.is_empty() || worker >= self.workers {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        for &tx in txs {
            if tx >= self.n {
                continue;
            }
            if inner.status[tx].phase != Phase::Ready {
                continue;
            }
            if inner.status[tx].queued {
                inner.status[tx].queued = false;
                self.dec_ready();
            }
            inner.status[tx].phase = Phase::Sticky;
            inner.sticky[worker].push(tx);
            self.sticky_flag[worker].store(true, Ordering::Release);
        }
    }

    pub(crate) fn take_sticky(&self, worker: usize) -> Option<(TxIdx, usize)> {
        if worker >= self.workers {
            return None;
        }
        let mut inner = self.inner.lock().unwrap();
        while let Some(tx) = inner.sticky[worker].pop() {
            if inner.sticky[worker].is_empty() {
                self.sticky_flag[worker].store(false, Ordering::Release);
            }
            if tx >= self.n {
                continue;
            }
            if inner.status[tx].phase != Phase::Sticky {
                continue;
            }
            inner.status[tx].phase = Phase::Executing;
            inner.status[tx].queued = false;
            let inc = inner.status[tx].incarnation;
            return Some((tx, inc));
        }
        self.sticky_flag[worker].store(false, Ordering::Release);
        None
    }

    /// The handoff did not validate. Put the private slot back on the deque.
    pub(crate) fn unstick(&self, worker: usize) {
        if worker >= self.workers {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        let txs = std::mem::take(&mut inner.sticky[worker]);
        self.sticky_flag[worker].store(false, Ordering::Release);
        let mut enqueued = false;
        for tx in txs {
            if tx >= self.n {
                continue;
            }
            if inner.status[tx].phase == Phase::Sticky {
                inner.status[tx].phase = Phase::Ready;
                inner.status[tx].queued = false;
                enqueued |= self.enqueue_locked(&mut inner, worker, tx);
            }
        }
        drop(inner);
        if enqueued {
            self.poke_work();
        }
    }

    #[allow(dead_code)]
    /// Run `tx` on this worker next, even if another deque already holds a copy.
    ///
    /// The extra copy is skipped when it is popped: only a `Ready` task starts.
    /// Used to keep a read-modify-write chain on the worker that published the
    /// previous write, instead of leaving the successor under unrelated work.
    pub(crate) fn boost(&self, worker: usize, tx: TxIdx) {
        let mut inner = self.inner.lock().unwrap();
        if tx >= self.n {
            return;
        }
        if inner.status[tx].phase != Phase::Ready {
            return;
        }
        inner.status[tx].queued = true;
        self.deques[worker].push_bottom(tx);
    }

    fn enqueue_locked(&self, inner: &mut Inner, worker: usize, tx: TxIdx) -> bool {
        let st = &mut inner.status[tx];
        if st.phase != Phase::Ready || st.queued {
            return false;
        }
        st.queued = true;
        self.ready_depth.fetch_add(1, Ordering::Relaxed);
        self.deques[worker].push_bottom(tx);
        true
    }

    /// Park `tx` until `pred` has executed, or until that incarnation is final
    /// when `until_final` is set. Armed reads, nonce, and estimates use the
    /// latter. Commit is not the wakeup.
    pub(crate) fn park(
        &self,
        worker: usize,
        tx: TxIdx,
        pred: TxIdx,
        bump_inc: bool,
        until_final: bool,
    ) {
        let mut inner = self.inner.lock().unwrap();
        if tx >= self.n || pred >= self.n {
            return;
        }
        if bump_inc {
            inner.status[tx].incarnation = inner.status[tx].incarnation.saturating_add(1);
            self.validated[tx].store(usize::MAX, Ordering::Release);
        }
        if inner.status[tx].queued {
            inner.status[tx].queued = false;
            self.dec_ready();
        }
        let pred_done = self.pred_satisfied(&inner, pred, until_final);
        if pred_done {
            inner.status[tx].phase = Phase::Ready;
            super::timeline::note_wake(tx);
            self.enqueue_locked(&mut inner, worker, tx);
            return;
        }
        inner.status[tx].phase = Phase::Parked { pred, until_final };
        inner.status[tx].queued = false;
        if !inner.dependents[pred].contains(&tx) {
            inner.dependents[pred].push(tx);
        }
        let pred_ready = inner.status[pred].phase == Phase::Ready && !inner.status[pred].queued;
        if pred_ready {
            self.enqueue_locked(&mut inner, worker, pred);
        }
    }

    fn pred_satisfied(&self, inner: &Inner, pred: TxIdx, until_final: bool) -> bool {
        match inner.status[pred].phase {
            Phase::Committed => true,
            Phase::Executed => !until_final || self.pred_final_locked(inner, pred),
            _ => false,
        }
    }

    pub(crate) fn finish_ok(&self, worker: usize, tx: TxIdx) {
        let mut inner = self.inner.lock().unwrap();
        if inner.status[tx].phase != Phase::Executing {
            return;
        }
        inner.status[tx].phase = Phase::Executed;
        let deps = std::mem::take(&mut inner.dependents[tx]);
        for d in deps {
            match inner.status[d].phase {
                Phase::Parked {
                    until_final: false, ..
                } => {
                    inner.status[d].phase = Phase::Ready;
                    super::timeline::note_wake(d);
                    self.enqueue_locked(&mut inner, worker, d);
                }
                Phase::Parked {
                    until_final: true, ..
                } => inner.dependents[tx].push(d),
                _ => {}
            }
        }
        drop(inner);
        self.poke_work();
    }

    /// Wake readers parked on this transaction's final incarnation.
    ///
    /// Commit is not required. Callers store the validated incarnation first.
    pub(crate) fn note_final(&self, worker: usize, tx: TxIdx) {
        if !self.is_final_any(tx) {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        if tx >= self.n {
            return;
        }
        self.wake_final_locked(&mut inner, worker, tx);
        drop(inner);
        self.poke_work();
    }

    fn wake_final_locked(&self, inner: &mut Inner, worker: usize, tx: TxIdx) {
        let deps = std::mem::take(&mut inner.dependents[tx]);
        for d in deps {
            match inner.status[d].phase {
                Phase::Parked {
                    until_final: true, ..
                } => {
                    inner.status[d].phase = Phase::Ready;
                    super::timeline::note_wake(d);
                    self.enqueue_locked(inner, worker, d);
                }
                Phase::Parked {
                    until_final: false, ..
                } => inner.dependents[tx].push(d),
                _ => {}
            }
        }
    }

    /// Validation failed. The next incarnation is queued on `worker`.
    /// The previous incarnation is no longer final.
    pub(crate) fn requeue_abort(&self, worker: usize, tx: TxIdx) {
        let mut inner = self.inner.lock().unwrap();
        self.validated[tx].store(usize::MAX, Ordering::Release);
        inner.status[tx].incarnation = inner.status[tx].incarnation.saturating_add(1);
        inner.status[tx].phase = Phase::Ready;
        inner.status[tx].queued = false;
        self.enqueue_locked(&mut inner, worker, tx);
        drop(inner);
        self.poke_work();
    }

    pub(crate) fn try_mark_committed(&self, worker: usize, tx: TxIdx, incarnation: usize) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if self.committed.load(Ordering::Relaxed) != tx {
            return false;
        }
        let st = &mut inner.status[tx];
        if st.phase == Phase::Executed && st.incarnation == incarnation {
            st.phase = Phase::Committed;
            self.validated[tx].store(incarnation, Ordering::Release);
            self.committed.store(tx + 1, Ordering::Release);
            self.wake_final_locked(&mut inner, worker, tx);
            let finished = self.committed.load(Ordering::Relaxed) == self.n;
            drop(inner);
            let _guard = self.mu.lock().unwrap();
            self.work_seq.fetch_add(1, Ordering::Release);
            if finished {
                self.cv.notify_all();
                self.sleep_cv.notify_all();
            } else {
                self.cv.notify_one();
            }
            true
        } else {
            false
        }
    }

    /// Queue the lowest transaction that can make the commit prefix move.
    pub(crate) fn rescue(&self, worker: usize) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let start = self.committed.load(Ordering::Relaxed);
        for tx in start..self.n {
            match inner.status[tx].phase {
                Phase::Committed => continue,
                Phase::Executing | Phase::Executed => return false,
                Phase::Ready => {
                    if !inner.status[tx].queued {
                        return self.enqueue_locked(&mut inner, worker, tx);
                    }
                    return false;
                }
                Phase::Sticky => return false,
                Phase::Parked { pred, until_final } => {
                    let pred_done = self.pred_satisfied(&inner, pred, until_final);
                    if pred_done {
                        inner.status[tx].phase = Phase::Ready;
                        super::timeline::note_wake(tx);
                        return self.enqueue_locked(&mut inner, worker, tx);
                    }
                    if inner.status[pred].phase == Phase::Ready && !inner.status[pred].queued {
                        return self.enqueue_locked(&mut inner, worker, pred);
                    }
                    return false;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::Runtime;

    #[test]
    fn final_incarnation_wakes_before_commit() {
        let rt = Runtime::new(2, 2);
        rt.seed();
        let (tx0, inc) = rt.pop(0).unwrap();
        let (tx1, _) = rt.pop(1).unwrap();
        assert_eq!((tx0, tx1), (0, 1));
        rt.park(1, 1, 0, false, true);
        rt.finish_ok(0, 0);
        assert!(rt.pop(0).is_none());
        assert!(rt.pop(1).is_none());
        assert_eq!(rt.committed(), 0);
        assert!(rt.mark_validated(0, inc));
        rt.note_final(0, 0);
        assert_eq!(rt.pop(0).unwrap().0, 1);
        assert_eq!(rt.committed(), 0);
    }

    #[test]
    fn sticky_handoff_is_not_stolen() {
        let rt = Runtime::new(4, 4);
        rt.seed();
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 0);
        rt.finish_ok(0, 0);
        // Transaction 1 was seeded on worker 1. Claiming it for worker 0
        // must hide it from every other pop.
        rt.stick(0, &[1]);
        assert!(rt.pop(1).is_none() || rt.pop(1).is_some_and(|(tx, _)| tx != 1));
        assert!(rt.pop(2).is_none() || rt.take_sticky(2).is_none());
        let (got, _) = rt.take_sticky(0).unwrap();
        assert_eq!(got, 1);
        assert!(rt.take_sticky(0).is_none());
    }

    #[test]
    fn hop_gap_does_not_shrink_while_other_work_is_ready() {
        let rt = Runtime::new(32, 8);
        rt.seed();
        rt.maybe_grow();
        let before = rt.active_now();
        assert!(before > 1, "active={before}");
        rt.observe_hop_time(2_000_000, 20_000);
        assert_eq!(rt.active_now(), before);
        assert!(rt.ready_now() > 0);
    }

    #[test]
    fn ready_width_shrinks_only_after_the_queue_drains() {
        let rt = Runtime::new(8, 8);
        rt.seed();
        rt.maybe_grow();
        assert_eq!(rt.active_now(), 8);
        for w in 0..8 {
            while rt.pop(w).is_some() {}
        }
        assert_eq!(rt.ready_now(), 0);
        for _ in 0..64 {
            rt.maybe_grow();
        }
        assert!(rt.active_now() < 8, "active={}", rt.active_now());
        assert!(rt.active_now() >= 1);
    }

    #[test]
    fn sticky_owner_spills_its_deque() {
        let rt = Runtime::new(8, 2);
        rt.seed();
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 0);
        rt.spill_owned(0);
        let mut got = Vec::new();
        while let Some((tx, _)) = rt.pop(1) {
            got.push(tx);
        }
        assert!(
            got.iter().any(|tx| *tx == 2 || *tx == 4 || *tx == 6),
            "spilled={got:?}"
        );
    }
}
