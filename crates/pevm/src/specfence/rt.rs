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
            validated: (0..n).map(|_| AtomicUsize::new(usize::MAX)).collect(),
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
        self.notify();
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
        self.notify();
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
            self.validated[tx].store(incarnation, Ordering::Release);
            self.committed.store(tx + 1, Ordering::Release);
            self.wake_final_locked(&mut inner, worker, tx);
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
}
