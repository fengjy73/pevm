//! Index-seeded Chase-Lev scheduler and the commit prefix.
//!
//! Tasks are transaction indexes. Each worker owns one deque. The owner pops
//! LIFO, thieves pop FIFO. Status transitions sit behind one mutex so a park
//! and a publish cannot lose a wakeup. The mutex is not held across the EVM.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// `until_commit` waits for the predecessor's validated write. Execution
    /// alone is not enough: a published incarnation can still fail validation.
    Parked {
        pred: TxIdx,
        until_commit: bool,
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
}

pub(crate) struct Runtime {
    n: usize,
    inner: Mutex<Inner>,
    deques: Vec<LocalDeque>,
    committed: AtomicUsize,
    executing: AtomicUsize,
    waiting: AtomicUsize,
    mu: Mutex<()>,
    cv: Condvar,
    abort: AtomicBool,
    workers: usize,
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
            }),
            deques: (0..workers).map(|_| LocalDeque::with_capacity(n)).collect(),
            committed: AtomicUsize::new(0),
            executing: AtomicUsize::new(0),
            waiting: AtomicUsize::new(0),
            mu: Mutex::new(()),
            cv: Condvar::new(),
            abort: AtomicBool::new(false),
            workers,
        }
    }

    /// Strided index seeding. Worker `w` owns `w, w+C, w+2C, ...`, pushed
    /// high-to-low so the owner LIFO runs the lowest index first. The first
    /// wave is transactions `0..C`, which lets a class head publish before
    /// later strides start.
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
                self.deques[w].push_bottom(tx);
            }
        }
    }

    pub(crate) fn worker_count(&self) -> usize {
        self.workers
    }

    pub(crate) fn committed(&self) -> usize {
        self.committed.load(Ordering::Acquire)
    }

    pub(crate) fn request_abort(&self) {
        self.abort.store(true, Ordering::Release);
        self.notify();
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

    pub(crate) fn notify(&self) {
        self.cv.notify_all();
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
                Phase::Ready | Phase::Executing | Phase::Parked { .. }
            )
        })
    }

    pub(crate) fn incarnation(&self, tx: TxIdx) -> usize {
        self.inner.lock().unwrap().status[tx].incarnation
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
        for _ in 0..64 {
            let tx = self.deques[worker].pop_bottom().or_else(|| {
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
            })?;
            let mut inner = self.inner.lock().unwrap();
            if tx >= self.n {
                continue;
            }
            inner.status[tx].queued = false;
            if inner.status[tx].phase == Phase::Ready {
                inner.status[tx].phase = Phase::Executing;
                let inc = inner.status[tx].incarnation;
                return Some((tx, inc));
            }
        }
        None
    }

    fn enqueue_locked(&self, inner: &mut Inner, worker: usize, tx: TxIdx) -> bool {
        let st = &mut inner.status[tx];
        if st.phase != Phase::Ready || st.queued {
            return false;
        }
        st.queued = true;
        self.deques[worker].push_bottom(tx);
        true
    }

    /// Park `tx` until `pred` has executed, or until it has committed when
    /// `until_commit` is set. A read of an armed location uses the latter:
    /// the final write is the one validation kept.
    pub(crate) fn park(
        &self,
        worker: usize,
        tx: TxIdx,
        pred: TxIdx,
        bump_inc: bool,
        until_commit: bool,
    ) {
        let mut inner = self.inner.lock().unwrap();
        if tx >= self.n || pred >= self.n {
            return;
        }
        if bump_inc {
            inner.status[tx].incarnation = inner.status[tx].incarnation.saturating_add(1);
        }
        let pred_done = match inner.status[pred].phase {
            Phase::Committed => true,
            Phase::Executed => !until_commit,
            _ => false,
        };
        if pred_done {
            inner.status[tx].phase = Phase::Ready;
            super::timeline::note_wake(tx);
            self.enqueue_locked(&mut inner, worker, tx);
            return;
        }
        inner.status[tx].phase = Phase::Parked { pred, until_commit };
        inner.status[tx].queued = false;
        if !inner.dependents[pred].contains(&tx) {
            inner.dependents[pred].push(tx);
        }
        let pred_ready = inner.status[pred].phase == Phase::Ready && !inner.status[pred].queued;
        if pred_ready {
            self.enqueue_locked(&mut inner, worker, pred);
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
                    until_commit: false,
                    ..
                } => {
                    inner.status[d].phase = Phase::Ready;
                    super::timeline::note_wake(d);
                    self.enqueue_locked(&mut inner, worker, d);
                }
                Phase::Parked {
                    until_commit: true, ..
                } => inner.dependents[tx].push(d),
                _ => {}
            }
        }
        drop(inner);
        self.notify();
    }

    /// Validation failed. The next incarnation is queued on `worker`.
    pub(crate) fn requeue_abort(&self, worker: usize, tx: TxIdx) {
        let mut inner = self.inner.lock().unwrap();
        inner.status[tx].incarnation = inner.status[tx].incarnation.saturating_add(1);
        inner.status[tx].phase = Phase::Ready;
        inner.status[tx].queued = false;
        self.enqueue_locked(&mut inner, worker, tx);
        drop(inner);
        self.notify();
    }

    pub(crate) fn try_mark_committed(&self, worker: usize, tx: TxIdx, incarnation: usize) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if self.committed.load(Ordering::Relaxed) != tx {
            return false;
        }
        let st = &mut inner.status[tx];
        if st.phase == Phase::Executed && st.incarnation == incarnation {
            st.phase = Phase::Committed;
            self.committed.store(tx + 1, Ordering::Release);
            let deps = std::mem::take(&mut inner.dependents[tx]);
            for d in deps {
                match inner.status[d].phase {
                    Phase::Parked {
                        until_commit: true, ..
                    } => {
                        inner.status[d].phase = Phase::Ready;
                        super::timeline::note_wake(d);
                        self.enqueue_locked(&mut inner, worker, d);
                    }
                    Phase::Parked {
                        until_commit: false,
                        ..
                    } => inner.dependents[tx].push(d),
                    _ => {}
                }
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
                Phase::Parked { pred, until_commit } => {
                    let pred_done = match inner.status[pred].phase {
                        Phase::Committed => true,
                        Phase::Executed => !until_commit,
                        _ => false,
                    };
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
