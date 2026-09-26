//! Lowest-index scheduler and the commit prefix.
//!
//! Tasks are transaction indexes. Each worker owns one queue ordered by index.
//! A pop takes the lowest ready index anywhere, and a thief takes that same
//! lowest index rather than the oldest entry on a victim. Status transitions
//! sit behind one mutex so a park and a publish cannot lose a wakeup. The
//! mutex is not held across the EVM.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::TxIdx;

use super::deque::IndexQueue;

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

const PH_READY: u8 = 0;
const PH_EXEC: u8 = 1;
const PH_DONE: u8 = 2;
const PH_COMMIT: u8 = 3;
const PH_STICKY: u8 = 4;
const PH_PARK: u8 = 5;

/// Low bit set while `handler.run` is on the stack. The rest is `worker + 1`.
const GATE_RUNNING: usize = 1;

fn gate_pack(worker: usize, running: bool) -> usize {
    ((worker + 1) << 1) | usize::from(running)
}

/// What a blocked worker should do with the predecessor chain.
#[derive(Debug)]
pub(crate) enum Claim {
    /// `tx` is marked `Executing` for this worker. Run it before the reader resumes.
    Run(TxIdx, usize),
    /// `tx` is inside the interpreter. Park on it.
    Wait(TxIdx),
    /// `tx` has finished executing and is not final. The reader seals it.
    Seal(TxIdx),
    /// The predecessor already finished. Retry the reader.
    Done,
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
    deques: Vec<IndexQueue>,
    committed: AtomicUsize,
    /// `usize::MAX` means this transaction's current incarnation is not final.
    /// A stored incarnation is final: execution finished, its reads came from
    /// final origins or storage, and validation kept that incarnation.
    validated: Vec<AtomicUsize>,
    executing: AtomicUsize,
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
    /// Ready-queue width, in 1/256 of a transaction. Recorded, not a shrink input.
    width_ema_q8: AtomicU64,
    shrink_streak: AtomicUsize,
    /// Phase mirror. `is_executing` reads this instead of the scheduler mutex.
    phases: Vec<AtomicU8>,
    /// Deque entries skipped because the task was no longer `Ready`.
    pop_skips: AtomicUsize,
    control_tick: AtomicUsize,
    ready_depth: AtomicUsize,
    /// Tasks moved off a worker that is staying on a chain handoff.
    injector: IndexQueue,
    /// Who may enter the interpreter for this transaction, and whether they have.
    exec_gate: Vec<AtomicUsize>,
    /// Set while the worker is inside a condvar wait. An unstarted task owned
    /// by an idle worker is claimable; stealing from a worker that is about to
    /// enter would bounce the task and never run it.
    worker_idle: Vec<AtomicBool>,
    /// Transaction this worker has bound and not yet left. `0` means none,
    /// otherwise `tx + 1`. A task whose owner is busy on a different index is
    /// not in the interpreter and must be claimable.
    worker_task: Vec<AtomicUsize>,
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
            deques: (0..workers).map(|_| IndexQueue::new()).collect(),
            committed: AtomicUsize::new(0),
            validated: (0..n).map(|_| AtomicUsize::new(usize::MAX)).collect(),
            executing: AtomicUsize::new(0),
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
            shrink_streak: AtomicUsize::new(0),
            phases: (0..n).map(|_| AtomicU8::new(PH_READY)).collect(),
            pop_skips: AtomicUsize::new(0),
            control_tick: AtomicUsize::new(0),
            ready_depth: AtomicUsize::new(0),
            injector: IndexQueue::new(),
            exec_gate: (0..n).map(|_| AtomicUsize::new(0)).collect(),
            worker_idle: (0..workers).map(|_| AtomicBool::new(false)).collect(),
            worker_task: (0..workers).map(|_| AtomicUsize::new(0)).collect(),
            active_samples: Mutex::new(Vec::new()),
            work_seq: AtomicUsize::new(0),
            sticky_flag: (0..workers).map(|_| AtomicBool::new(false)).collect(),
        }
    }

    /// Strided index seeding. Worker `w` owns `w, w+C, w+2C, ...`. The queue
    /// is ordered by index, so the first pop is the lowest ready transaction,
    /// not the most recently pushed one. The first wave is transactions
    /// `0..C`, which lets a class head publish before later strides start.
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
            for tx in owned {
                inner.status[tx].queued = true;
                self.ready_depth.fetch_add(1, Ordering::Relaxed);
                self.deques[w].push(tx);
            }
        }
        // Every seeded task is ready. Leaving `active` at 1 parks the other
        // workers on the inactive condvar until the first task finishes.
        let width = self.workers.min(self.n).max(1);
        self.active.store(width, Ordering::Release);
        self.peak_active.store(width, Ordering::Relaxed);
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
        self.set_phase(&mut inner, tx, Phase::Committed);
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
    /// tasks would otherwise sit in this worker's queue until it popped again.
    pub(crate) fn spill_owned(&self, worker: usize) {
        if worker >= self.workers {
            return;
        }
        let moved = self.deques[worker].drain();
        if moved.is_empty() {
            return;
        }
        for tx in moved {
            self.injector.push(tx);
        }
        self.poke_work();
    }

    /// Grow and shrink from work that is queued or inside the interpreter.
    ///
    /// The ready queue alone collapses the set in the middle of the block:
    /// tasks leave the queue when they start, and a decayed average of that
    /// dip ratchets the set down to 1 while other workers are still running.
    /// The floor is `ready + executing`. Shrinking waits for two decisions.
    /// A chain hop is not an input. The moving average is only a trace.
    pub(crate) fn maybe_grow(&self) {
        let ready = self.ready_now();
        let inflight = self.executing.load(Ordering::Relaxed);
        let width_now = ready.saturating_add(inflight);
        let prev = self.width_ema_q8.load(Ordering::Relaxed);
        let sample = (width_now as u64).saturating_mul(256);
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
        let mut active = self.active.load(Ordering::Relaxed);
        let cap = self.workers;
        let floor = width_now.clamp(1, cap);
        if floor > active {
            self.shrink_streak.store(0, Ordering::Relaxed);
            active = floor;
        } else if active > floor {
            let streak = self.shrink_streak.fetch_add(1, Ordering::Relaxed) + 1;
            if streak >= 2 {
                active = floor;
                self.shrink_streak.store(0, Ordering::Relaxed);
            }
        } else {
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
            // Recheck within 0.5 ms if a grow was missed. The publisher also
            // notifies this condvar under `mu`.
            let (next, _) = self
                .sleep_cv
                .wait_timeout(guard, Duration::from_micros(500))
                .unwrap();
            guard = next;
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
        if self.ready_now() > 0 {
            return;
        }
        self.parked.fetch_add(1, Ordering::Relaxed);
        // A missed wake cannot leave this worker asleep for more than 0.5 ms
        // while a later publish, or this timeout, makes the work visible.
        let _ = self.cv.wait_timeout(guard, Duration::from_micros(500));
        self.woken.fetch_add(1, Ordering::Relaxed);
    }

    fn poke_work(&self) {
        let ready = self.ready_now();
        let inflight = self.executing.load(Ordering::Relaxed);
        let floor = ready.saturating_add(inflight).clamp(1, self.workers);
        let _guard = self.mu.lock().unwrap();
        let prev = self.active.load(Ordering::Relaxed);
        if floor > prev {
            self.active.store(floor, Ordering::Release);
            self.peak_active.fetch_max(floor, Ordering::Relaxed);
        }
        self.work_seq.fetch_add(1, Ordering::Release);
        // One sleeper is not enough: the task it pops may not be the one that
        // was just published, and the others would stay on the condvar.
        self.cv.notify_all();
        if floor > prev {
            self.sleep_cv.notify_all();
        }
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

    #[allow(dead_code)]
    pub(crate) fn notify(&self) {
        let _guard = self.mu.lock().unwrap();
        self.work_seq.fetch_add(1, Ordering::Release);
        self.cv.notify_all();
        self.sleep_cv.notify_all();
    }

    pub(crate) fn is_executing(&self, tx: TxIdx) -> bool {
        self.phases
            .get(tx)
            .is_some_and(|phase| phase.load(Ordering::Acquire) == PH_EXEC)
    }

    pub(crate) fn is_committed(&self, tx: TxIdx) -> bool {
        self.phases
            .get(tx)
            .is_some_and(|phase| phase.load(Ordering::Acquire) == PH_COMMIT)
    }

    pub(crate) fn pop_skips(&self) -> usize {
        self.pop_skips.load(Ordering::Relaxed)
    }

    fn set_phase(&self, inner: &mut Inner, tx: usize, phase: Phase) {
        let leaving_exec = inner.status[tx].phase == Phase::Executing && phase != Phase::Executing;
        let owner_gate = if leaving_exec {
            self.exec_gate[tx].load(Ordering::Acquire)
        } else {
            0
        };
        inner.status[tx].phase = phase;
        let bits = match &inner.status[tx].phase {
            Phase::Ready => PH_READY,
            Phase::Executing => PH_EXEC,
            Phase::Executed => PH_DONE,
            Phase::Committed => PH_COMMIT,
            Phase::Sticky => PH_STICKY,
            Phase::Parked { .. } => PH_PARK,
        };
        self.phases[tx].store(bits, Ordering::Release);
        if bits != PH_EXEC {
            self.exec_gate[tx].store(0, Ordering::Release);
            if leaving_exec && owner_gate != 0 {
                let id = owner_gate >> 1;
                if id > 0
                    && let Some(slot) = self.worker_task.get(id - 1)
                {
                    let _ = slot.compare_exchange(tx + 1, 0, Ordering::AcqRel, Ordering::Relaxed);
                }
            }
        }
    }

    pub(crate) fn in_interpreter(&self, tx: TxIdx) -> bool {
        self.interpreter_running(tx)
    }

    fn interpreter_running(&self, tx: TxIdx) -> bool {
        self.exec_gate
            .get(tx)
            .is_some_and(|gate| gate.load(Ordering::Acquire) & GATE_RUNNING != 0)
    }

    /// `handler.run` may start. Fails when another worker stole the unstarted task.
    pub(crate) fn try_enter_interp(&self, worker: usize, tx: TxIdx) -> bool {
        if tx >= self.n {
            return false;
        }
        let idle = gate_pack(worker, false);
        let busy = gate_pack(worker, true);
        if self.exec_gate[tx]
            .compare_exchange(idle, busy, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        if !self.is_executing(tx) {
            let _ =
                self.exec_gate[tx].compare_exchange(busy, 0, Ordering::AcqRel, Ordering::Acquire);
            return false;
        }
        true
    }

    /// Drop ownership of an unstarted or just-finished task. False when a
    /// different worker owns it.
    fn release_if_held(&self, worker: usize, tx: TxIdx) -> bool {
        if tx >= self.n {
            return false;
        }
        let cur = self.exec_gate[tx].load(Ordering::Acquire);
        if cur == 0 {
            return true;
        }
        let idle = gate_pack(worker, false);
        let busy = gate_pack(worker, true);
        if cur != idle && cur != busy {
            return false;
        }
        self.exec_gate[tx]
            .compare_exchange(cur, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn set_idle(&self, worker: usize, idle: bool) {
        if let Some(flag) = self.worker_idle.get(worker) {
            flag.store(idle, Ordering::Release);
        }
    }

    fn steal_unstarted(&self, worker: usize, tx: TxIdx) -> bool {
        if tx >= self.n {
            return false;
        }
        let mine = gate_pack(worker, false);
        loop {
            let cur = self.exec_gate[tx].load(Ordering::Acquire);
            if cur & GATE_RUNNING != 0 {
                return false;
            }
            if cur == 0 {
                return false;
            }
            if cur == mine {
                return true;
            }
            if self.exec_gate[tx]
                .compare_exchange(cur, mine, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let old = cur >> 1;
                if old > 0
                    && old - 1 != worker
                    && let Some(slot) = self.worker_task.get(old - 1)
                {
                    let _ = slot.compare_exchange(tx + 1, 0, Ordering::AcqRel, Ordering::Relaxed);
                }
                if let Some(slot) = self.worker_task.get(worker) {
                    slot.store(tx + 1, Ordering::Release);
                }
                return self.exec_gate[tx].load(Ordering::Acquire) & GATE_RUNNING == 0;
            }
        }
    }

    fn bind_owner(&self, worker: usize, tx: TxIdx) {
        self.exec_gate[tx].store(gate_pack(worker, false), Ordering::Release);
        if let Some(slot) = self.worker_task.get(worker) {
            slot.store(tx + 1, Ordering::Release);
        }
    }

    /// The owner has bound `tx` and has not gone idle. It is in the pre-check,
    /// about to enter the interpreter. Stealing here bounces the task.
    fn owner_busy_on(&self, tx: TxIdx) -> bool {
        let cur = self.exec_gate[tx].load(Ordering::Acquire);
        if cur == 0 || cur & GATE_RUNNING != 0 {
            return false;
        }
        let id = cur >> 1;
        if id == 0 {
            return false;
        }
        let owner = id - 1;
        let on_task = self
            .worker_task
            .get(owner)
            .is_some_and(|slot| slot.load(Ordering::Acquire) == tx + 1);
        let idle = self
            .worker_idle
            .get(owner)
            .is_some_and(|flag| flag.load(Ordering::Acquire));
        on_task && !idle
    }

    /// Drop every queued copy. Hints skip a queue that cannot hold `tx`.
    fn unqueue(&self, tx: usize) {
        for queue in &self.deques {
            let hint = queue.hint();
            if hint != usize::MAX && hint <= tx {
                queue.remove(tx);
            }
        }
        let hint = self.injector.hint();
        if hint != usize::MAX && hint <= tx {
            self.injector.remove(tx);
        }
    }

    /// This worker's lowest index, else the lowest index on any other queue.
    fn pop_one(&self, worker: usize) -> Option<usize> {
        if let Some(queue) = self.deques.get(worker) {
            let hint = queue.hint();
            if hint != usize::MAX
                && let Some(tx) = queue.pop_if_lowest(hint)
            {
                return Some(tx);
            }
        }
        let spins = self.workers.saturating_mul(4).max(8);
        for _ in 0..spins {
            let mut best = usize::MAX;
            let mut from_injector = false;
            let mut owner = 0usize;
            for w in 0..self.workers {
                if w == worker {
                    continue;
                }
                let hint = self.deques[w].hint();
                if hint < best {
                    best = hint;
                    from_injector = false;
                    owner = w;
                }
            }
            let inj = self.injector.hint();
            if inj < best {
                best = inj;
                from_injector = true;
            }
            if best == usize::MAX {
                // The owner's hint can move between the first try and here.
                if let Some(queue) = self.deques.get(worker) {
                    let hint = queue.hint();
                    if hint != usize::MAX {
                        return queue.pop_if_lowest(hint);
                    }
                }
                return None;
            }
            let got = if from_injector {
                self.injector.pop_if_lowest(best)
            } else {
                self.deques[owner].pop_if_lowest(best)
            };
            if let Some(tx) = got {
                return Some(tx);
            }
        }
        None
    }

    /// Not executed and not committed. Prediction skips writers that already finished.
    pub(crate) fn still_open(&self, tx: TxIdx) -> bool {
        self.phases.get(tx).is_some_and(|phase| {
            matches!(
                phase.load(Ordering::Acquire),
                PH_READY | PH_EXEC | PH_STICKY | PH_PARK
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
        self.phases
            .get(tx)
            .is_some_and(|phase| phase.load(Ordering::Acquire) == PH_DONE)
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
            self.set_phase(&mut inner, tx, Phase::Ready);
            inner.status[tx].queued = false;
        }
    }

    pub(crate) fn pop(&self, worker: usize) -> Option<(TxIdx, usize)> {
        let limit = self.n.saturating_mul(2).max(64);
        for _ in 0..limit {
            let Some(tx) = self.pop_one(worker) else {
                return None;
            };
            if tx >= self.n {
                continue;
            }
            if self.phases[tx].load(Ordering::Acquire) != PH_READY {
                self.pop_skips.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let mut inner = self.inner.lock().unwrap();
            let was_queued = inner.status[tx].queued;
            inner.status[tx].queued = false;
            if inner.status[tx].phase == Phase::Ready {
                self.bind_owner(worker, tx);
                self.set_phase(&mut inner, tx, Phase::Executing);
                if was_queued {
                    self.dec_ready();
                }
                let inc = inner.status[tx].incarnation;
                return Some((tx, inc));
            }
            self.pop_skips.fetch_add(1, Ordering::Relaxed);
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
        self.set_phase(&mut inner, tx, Phase::Sticky);
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
        self.set_phase(&mut inner, tx, Phase::Executed);
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
                    self.set_phase(&mut inner, dep, Phase::Ready);
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
                self.unqueue(tx);
                self.set_phase(inner, tx, Phase::Sticky);
                inner.sticky[worker].push(tx);
                self.sticky_flag[worker].store(true, Ordering::Release);
                true
            }
            _ => false,
        }
    }

    /// The commit-prefix transaction, if it is not inside the interpreter.
    ///
    /// Ready and sticky copies are removed from every queue, including a queue
    /// whose owner is inside a later transaction. An executing copy is taken
    /// when its owner is idle or busy on a different index. A parked prefix is
    /// replaced by the unstarted task it is waiting on.
    pub(crate) fn claim_commit_frontier(&self, worker: usize) -> Option<(TxIdx, usize)> {
        let tx = self.committed();
        if tx >= self.n {
            return None;
        }
        let phase = self.phases[tx].load(Ordering::Acquire);
        if phase == PH_EXEC {
            if self.interpreter_running(tx) || self.owner_busy_on(tx) {
                return None;
            }
            let inner = self.inner.lock().unwrap();
            if inner.status[tx].phase != Phase::Executing
                || self.interpreter_running(tx)
                || self.owner_busy_on(tx)
            {
                return None;
            }
            if self.exec_gate[tx].load(Ordering::Acquire) == 0 {
                self.bind_owner(worker, tx);
            } else if !self.steal_unstarted(worker, tx) {
                return None;
            }
            let inc = inner.status[tx].incarnation;
            return Some((tx, inc));
        }
        if phase == PH_READY || phase == PH_STICKY {
            let mut inner = self.inner.lock().unwrap();
            if !matches!(inner.status[tx].phase, Phase::Ready | Phase::Sticky) {
                return None;
            }
            self.take_over(&mut inner, worker, tx);
            let inc = inner.status[tx].incarnation;
            return Some((tx, inc));
        }
        if phase == PH_PARK {
            return match self.claim_blocker(worker, tx) {
                Claim::Run(root, inc) => Some((root, inc)),
                _ => None,
            };
        }
        None
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
            self.unqueue(tx);
            self.set_phase(&mut inner, tx, Phase::Sticky);
            inner.sticky[worker].push(tx);
            self.sticky_flag[worker].store(true, Ordering::Release);
        }
    }

    pub(crate) fn take_sticky(&self, worker: usize) -> Option<(TxIdx, usize)> {
        if worker >= self.workers {
            return None;
        }
        // The common path has no handoff. Do not take the scheduler mutex.
        if !self.sticky_flag[worker].load(Ordering::Acquire) {
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
            self.bind_owner(worker, tx);
            self.set_phase(&mut inner, tx, Phase::Executing);
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
                self.set_phase(&mut inner, tx, Phase::Ready);
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
        if !inner.status[tx].queued {
            inner.status[tx].queued = true;
            self.ready_depth.fetch_add(1, Ordering::Relaxed);
        }
        self.deques[worker].push(tx);
        drop(inner);
        self.poke_work();
    }

    fn enqueue_locked(&self, inner: &mut Inner, worker: usize, tx: TxIdx) -> bool {
        let st = &mut inner.status[tx];
        if st.phase != Phase::Ready || st.queued {
            return false;
        }
        st.queued = true;
        self.ready_depth.fetch_add(1, Ordering::Relaxed);
        self.deques[worker].push(tx);
        true
    }

    /// Put an executing handoff back on this worker's private slot.
    ///
    /// The commit frontier was claimed in its place. The handoff stays ahead
    /// of this worker's ordinary pops.
    pub(crate) fn return_sticky(&self, worker: usize, tx: TxIdx) {
        if worker >= self.workers || tx >= self.n {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        if inner.status[tx].phase != Phase::Executing {
            return;
        }
        self.set_phase(&mut inner, tx, Phase::Sticky);
        inner.sticky[worker].push(tx);
        self.sticky_flag[worker].store(true, Ordering::Release);
    }

    /// Park `tx` until `pred` has executed, or until that incarnation is final
    /// when `until_final` is set. Armed reads, nonce, and estimates use the
    /// latter. Commit is not the wakeup.
    ///
    /// Returns false when another worker owns `tx`. Any task this call makes
    /// ready wakes parked workers before it returns.
    pub(crate) fn park(
        &self,
        worker: usize,
        tx: TxIdx,
        pred: TxIdx,
        bump_inc: bool,
        until_final: bool,
    ) -> bool {
        let (publish, parked) = {
            let mut inner = self.inner.lock().unwrap();
            if tx >= self.n || pred >= self.n {
                return false;
            }
            if !self.release_if_held(worker, tx) {
                return false;
            }
            if bump_inc {
                inner.status[tx].incarnation = inner.status[tx].incarnation.saturating_add(1);
                self.validated[tx].store(usize::MAX, Ordering::Release);
            }
            if inner.status[tx].queued {
                inner.status[tx].queued = false;
                self.dec_ready();
            }
            let mut publish = false;
            let mut parked = false;
            let pred_done = self.pred_satisfied(&inner, pred, until_final);
            if pred_done {
                self.set_phase(&mut inner, tx, Phase::Ready);
                super::timeline::note_wake(tx);
                publish |= self.enqueue_locked(&mut inner, worker, tx);
            } else {
                parked = true;
                self.unqueue(tx);
                self.set_phase(&mut inner, tx, Phase::Parked { pred, until_final });
                inner.status[tx].queued = false;
                if !inner.dependents[pred].contains(&tx) {
                    inner.dependents[pred].push(tx);
                }
                // Readers already parked on `tx` are waiting on a task that just
                // left the interpreter. Point them at the predecessor that is
                // actually executing, or at the ready task this worker will run.
                publish |= self.retarget_waiters(&mut inner, worker, tx);
                let pred_ready =
                    inner.status[pred].phase == Phase::Ready && !inner.status[pred].queued;
                if pred_ready {
                    publish |= self.enqueue_locked(&mut inner, worker, pred);
                }
            }
            (publish, parked)
        };
        if parked {
            let (reason, loc) = super::timeline::block_wait();
            let reason = if reason == 0 {
                super::timeline::OTHER
            } else {
                reason
            };
            super::timeline::open_park(tx, pred, loc, 0, reason);
        }
        if publish {
            self.poke_work();
        }
        true
    }

    /// Follow parked links to the task that is not itself waiting.
    fn follow(&self, inner: &Inner, start: TxIdx) -> TxIdx {
        let mut tx = start;
        for _ in 0..self.n {
            match inner.status.get(tx).map(|st| st.phase) {
                Some(Phase::Parked { pred, .. }) if pred != tx && pred < self.n => tx = pred,
                _ => return tx,
            }
        }
        start
    }

    fn retarget_waiters(&self, inner: &mut Inner, worker: usize, tx: TxIdx) -> bool {
        let Some(pred) = (match inner.status[tx].phase {
            Phase::Parked { pred, .. } => Some(pred),
            _ => None,
        }) else {
            return false;
        };
        let root = self.follow(inner, pred);
        let waiting = std::mem::take(&mut inner.dependents[tx]);
        let mut keep = Vec::new();
        let mut publish = false;
        for d in waiting {
            if d >= self.n {
                continue;
            }
            match inner.status[d].phase {
                Phase::Parked { .. } => {
                    if self.pred_satisfied(inner, root, false) {
                        self.set_phase(inner, d, Phase::Ready);
                        super::timeline::note_wake(d);
                        publish |= self.enqueue_locked(inner, worker, d);
                    } else if d != root {
                        self.set_phase(
                            inner,
                            d,
                            Phase::Parked {
                                pred: root,
                                until_final: false,
                            },
                        );
                        if !inner.dependents[root].contains(&d) {
                            inner.dependents[root].push(d);
                        }
                    }
                }
                _ => keep.push(d),
            }
        }
        inner.dependents[tx] = keep;
        publish
    }

    /// The blocking predecessor is not in the interpreter. Run the first
    /// ready task in its park chain, or report the one that is executing.
    pub(crate) fn claim_blocker(&self, worker: usize, pred: TxIdx) -> Claim {
        let mut inner = self.inner.lock().unwrap();
        let mut tx = pred;
        for _ in 0..self.n {
            if tx >= self.n {
                return Claim::Wait(pred.min(self.n.saturating_sub(1)));
            }
            match inner.status[tx].phase {
                Phase::Executing => {
                    // Inside `handler.run`, or the owner is in the pre-check of
                    // this task. Anywhere else the task is queued on a busy
                    // worker and the waiter runs it.
                    if self.interpreter_running(tx) || self.owner_busy_on(tx) {
                        return Claim::Wait(tx);
                    }
                    if self.exec_gate[tx].load(Ordering::Acquire) == 0 {
                        self.bind_owner(worker, tx);
                        let inc = inner.status[tx].incarnation;
                        return Claim::Run(tx, inc);
                    }
                    if self.steal_unstarted(worker, tx) {
                        let inc = inner.status[tx].incarnation;
                        return Claim::Run(tx, inc);
                    }
                    return Claim::Wait(tx);
                }
                Phase::Committed => return Claim::Done,
                Phase::Executed => {
                    let inc = inner.status[tx].incarnation;
                    if self.is_final_inc(tx, inc) {
                        return Claim::Done;
                    }
                    // Finished, not final. The reader seals it. Waiting here
                    // parks on a transaction that is not in the interpreter.
                    return Claim::Seal(tx);
                }
                Phase::Parked { pred: next, .. } => {
                    if next == tx || next >= self.n {
                        return Claim::Wait(tx);
                    }
                    tx = next;
                }
                Phase::Ready | Phase::Sticky => {
                    self.take_over(&mut inner, worker, tx);
                    let inc = inner.status[tx].incarnation;
                    return Claim::Run(tx, inc);
                }
            }
        }
        Claim::Wait(pred.min(self.n.saturating_sub(1)))
    }

    fn take_over(&self, inner: &mut Inner, worker: usize, tx: TxIdx) {
        self.unqueue(tx);
        if inner.status[tx].queued {
            inner.status[tx].queued = false;
            self.dec_ready();
        }
        for slot in &mut inner.sticky {
            if slot.contains(&tx) {
                slot.retain(|item| *item != tx);
            }
        }
        for (owner, slot) in inner.sticky.iter().enumerate() {
            if slot.is_empty() {
                self.sticky_flag[owner].store(false, Ordering::Release);
            }
        }
        self.bind_owner(worker, tx);
        self.set_phase(inner, tx, Phase::Executing);
    }

    /// The predecessor finished between the block and the claim. Run `tx` again.
    pub(crate) fn retry_now(&self, worker: usize, tx: TxIdx) {
        let mut inner = self.inner.lock().unwrap();
        if tx >= self.n {
            return;
        }
        if inner.status[tx].phase == Phase::Executing {
            self.set_phase(&mut inner, tx, Phase::Ready);
            inner.status[tx].queued = false;
            self.enqueue_locked(&mut inner, worker, tx);
        }
        drop(inner);
        self.poke_work();
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
        self.set_phase(&mut inner, tx, Phase::Executed);
        let deps = std::mem::take(&mut inner.dependents[tx]);
        for d in deps {
            match inner.status[d].phase {
                Phase::Parked {
                    until_final: false, ..
                } => {
                    self.set_phase(&mut inner, d, Phase::Ready);
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
                    self.set_phase(inner, d, Phase::Ready);
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
        self.set_phase(&mut inner, tx, Phase::Ready);
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
        let ready_to_commit = inner.status[tx].phase == Phase::Executed
            && inner.status[tx].incarnation == incarnation;
        if ready_to_commit {
            self.set_phase(&mut inner, tx, Phase::Committed);
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
                self.cv.notify_all();
            }
            true
        } else {
            false
        }
    }

    /// Queue the lowest transaction that can make the commit prefix move.
    ///
    /// A task this call makes ready wakes parked workers. The caller still
    /// loops and runs it; the wake is for every other worker that is asleep.
    pub(crate) fn rescue(&self, worker: usize) -> bool {
        let publish = {
            let mut inner = self.inner.lock().unwrap();
            let start = self.committed.load(Ordering::Relaxed);
            let mut queued = false;
            for tx in start..self.n {
                match inner.status[tx].phase {
                    Phase::Committed => continue,
                    Phase::Executing | Phase::Executed => break,
                    Phase::Ready => {
                        if !inner.status[tx].queued {
                            queued = self.enqueue_locked(&mut inner, worker, tx);
                        }
                        break;
                    }
                    Phase::Sticky => break,
                    Phase::Parked { pred, until_final } => {
                        let pred_done = self.pred_satisfied(&inner, pred, until_final);
                        if pred_done {
                            self.set_phase(&mut inner, tx, Phase::Ready);
                            super::timeline::note_wake(tx);
                            queued = self.enqueue_locked(&mut inner, worker, tx);
                        } else if inner.status[pred].phase == Phase::Ready
                            && !inner.status[pred].queued
                        {
                            queued = self.enqueue_locked(&mut inner, worker, pred);
                        }
                        break;
                    }
                }
            }
            queued
        };
        if publish {
            self.poke_work();
        }
        publish
    }

    #[cfg(test)]
    fn parked_pred(&self, tx: TxIdx) -> Option<TxIdx> {
        let inner = self.inner.lock().unwrap();
        match inner.status.get(tx).map(|state| state.phase) {
            Some(Phase::Parked { pred, .. }) => Some(pred),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Claim, Runtime};

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

    #[test]
    fn claim_runs_a_predecessor_that_is_not_executing() {
        let rt = Runtime::new(4, 2);
        rt.seed();
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 0);
        match rt.claim_blocker(1, 2) {
            Claim::Run(got, inc) => {
                assert_eq!(got, 2);
                assert_eq!(inc, 0);
            }
            other => panic!("expected to run tx 2, {other:?}"),
        }
        assert!(rt.is_executing(2));
        while let Some((tx, _)) = rt.pop(0) {
            assert_ne!(tx, 2);
        }
        assert!(rt.pop(1).is_none() || rt.is_executing(2));
    }

    #[test]
    fn claim_follows_a_parked_predecessor_to_a_ready_root() {
        let rt = Runtime::new(3, 2);
        rt.park(0, 1, 0, false, false);
        assert_eq!(rt.parked_pred(1), Some(0));
        match rt.claim_blocker(1, 1) {
            Claim::Run(0, _) => {}
            other => panic!("expected to run tx 0, {other:?}"),
        }
        assert!(rt.is_executing(0));
        assert_eq!(rt.parked_pred(1), Some(0));
    }

    #[test]
    fn parking_retargets_a_reader_onto_the_running_root() {
        let rt = Runtime::new(3, 2);
        rt.seed();
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 0);
        let (tx, _) = rt.pop(1).unwrap();
        assert_eq!(tx, 1);
        rt.park(1, 1, 0, false, false);
        assert_eq!(rt.parked_pred(1), Some(0));
        match rt.claim_blocker(0, 2) {
            Claim::Run(2, _) => {}
            other => panic!("expected to run tx 2, {other:?}"),
        }
        rt.park(0, 0, 2, false, false);
        assert_eq!(
            rt.parked_pred(1),
            Some(2),
            "reader must not keep waiting on a parked predecessor"
        );
        assert!(rt.is_executing(2));
    }

    #[test]
    fn reader_parks_on_the_executing_root_of_a_parked_chain() {
        let rt = Runtime::new(4, 2);
        rt.seed();
        rt.park(0, 1, 0, false, false);
        rt.park(0, 2, 1, false, false);
        let root = match rt.claim_blocker(1, 2) {
            Claim::Run(root, _) => root,
            other => panic!("expected the ready root, {other:?}"),
        };
        assert_eq!(root, 0);
        assert!(rt.is_executing(0));
        let pred = 2;
        let on = if rt.is_executing(pred) { pred } else { root };
        rt.park(1, 3, on, false, false);
        let parked_on = rt.parked_pred(3).expect("reader stays parked");
        assert!(
            rt.is_executing(parked_on),
            "parked on {parked_on}, which is not executing"
        );
        assert_eq!(parked_on, 0);
    }

    #[test]
    fn executed_predecessor_is_sealed_instead_of_parked() {
        let rt = Runtime::new(2, 2);
        rt.seed();
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 0);
        rt.finish_ok(0, 0);
        assert!(
            matches!(rt.claim_blocker(1, 0), Claim::Seal(0)),
            "a finished predecessor is not a park target"
        );
    }

    #[test]
    fn claim_waits_while_the_predecessor_executes() {
        let rt = Runtime::new(2, 2);
        rt.seed();
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 0);
        assert!(rt.try_enter_interp(0, 0));
        assert!(matches!(rt.claim_blocker(1, 0), Claim::Wait(0)));
    }

    #[test]
    fn estimate_claim_runs_a_predecessor_that_is_not_in_the_interpreter() {
        // Tx 102 blocks on an estimate from tx 101. 101 was popped, so its
        // phase is Executing, but `handler.run` has not started. The waiter
        // runs 101. Parking would wait for a task nobody is executing.
        let rt = Runtime::new(4, 2);
        rt.seed();
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 0);
        assert!(rt.is_executing(0));
        // The popper left the task and waited. It is not about to enter.
        rt.set_idle(0, true);
        assert!(!rt.try_enter_interp(1, 0));
        match rt.claim_blocker(1, 0) {
            Claim::Run(0, inc) => assert_eq!(inc, 0),
            other => panic!("expected to run the unstarted predecessor, {other:?}"),
        }
        assert!(
            !rt.try_enter_interp(0, 0),
            "the worker that popped 101 lost it"
        );
        assert!(rt.try_enter_interp(1, 0), "the blocked worker enters 101");
    }

    #[test]
    fn commit_frontier_is_claimed_ahead_of_later_ready_work() {
        let rt = Runtime::new(8, 4);
        rt.seed();
        let (tx, inc) = rt.claim_commit_frontier(1).unwrap();
        assert_eq!((tx, inc), (0, 0));
        assert!(rt.try_enter_interp(1, 0));
        assert!(rt.claim_commit_frontier(2).is_none());
        rt.finish_ok(1, 0);
        assert!(rt.try_mark_committed(1, 0, 0));
        let (next, _) = rt.claim_commit_frontier(2).unwrap();
        assert_eq!(next, 1);
    }

    #[test]
    fn parking_on_a_finished_predecessor_wakes() {
        let rt = Runtime::new(2, 1);
        rt.seed();
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 0);
        assert!(rt.try_enter_interp(0, 0));
        rt.finish_ok(0, 0);
        let (tx, _) = rt.pop(0).unwrap();
        assert_eq!(tx, 1);
        assert!(rt.try_enter_interp(0, 1));
        let ticket = rt.work_ticket();
        assert!(rt.park(0, 1, 0, false, false));
        assert!(rt.work_ticket() > ticket);
        assert_eq!(rt.pop(0).unwrap().0, 1);
    }

    #[test]
    fn executing_count_keeps_the_active_set() {
        let rt = Runtime::new(8, 4);
        rt.seed();
        rt.maybe_grow();
        assert_eq!(rt.active_now(), 4);
        for w in 0..4 {
            while rt.pop(w).is_some() {}
        }
        assert_eq!(rt.ready_now(), 0);
        rt.executing_add(4);
        for _ in 0..32 {
            rt.maybe_grow();
        }
        assert_eq!(rt.active_now(), 4, "in-flight work must hold the set");
    }

    #[test]
    fn pop_returns_the_lowest_ready_index() {
        let rt = Runtime::new(8, 1);
        rt.seed();
        rt.boost(0, 7);
        assert_eq!(rt.pop(0).unwrap().0, 0);
        assert_eq!(rt.pop(0).unwrap().0, 1);
        assert_eq!(rt.pop(0).unwrap().0, 2);
    }

    #[test]
    fn pre_check_is_not_stolen() {
        let rt = Runtime::new(4, 2);
        rt.seed();
        assert_eq!(rt.pop(0).unwrap().0, 0);
        rt.set_idle(0, false);
        assert!(!rt.in_interpreter(0));
        assert!(matches!(rt.claim_blocker(1, 0), Claim::Wait(0)));
    }

    #[test]
    fn executing_owner_busy_on_a_later_tx_is_claimed() {
        let rt = Runtime::new(4, 2);
        rt.seed();
        assert_eq!(rt.pop(0).unwrap().0, 0);
        assert_eq!(rt.pop(0).unwrap().0, 2);
        rt.set_idle(0, false);
        match rt.claim_blocker(1, 0) {
            Claim::Run(0, inc) => assert_eq!(inc, 0),
            other => panic!("expected to run the unstarted predecessor, {other:?}"),
        }
        assert!(rt.try_enter_interp(1, 0));
    }

    #[test]
    fn frontier_is_taken_from_a_busy_workers_queue() {
        let rt = Runtime::new(8, 2);
        rt.seed();
        assert_eq!(rt.pop(0).unwrap().0, 0);
        rt.defer(0);
        assert_eq!(rt.pop(0).unwrap().0, 2);
        assert!(rt.try_enter_interp(0, 2));
        assert!(rt.rescue(0));
        let (front, _) = rt.claim_commit_frontier(1).unwrap();
        assert_eq!(front, 0);
        assert!(rt.try_enter_interp(1, 0));
    }
}
