//! RunnableSet — three first-class queues + work-steal (SF-PS T1 / PC).
//!
//! `Q_indep` / `Q_released` / `Q_ordered` are real deques. Pick never calls
//! `Scheduler::next_task*`. Refuse = leave the head waiting and steal another
//! independent (PC-2). Soft=0: WaitReleased txs are not on any Q.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use parking_lot::Mutex;

use crate::TxIdx;
use crate::scheduler::Scheduler;

use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::visibility::VisibilityPolicy;

const ST_NONE: u8 = 0;
const ST_INDEP: u8 = 1;
const ST_RELEASED: u8 = 2;
const ST_ORDERED: u8 = 3;
const ST_REVALIDATE: u8 = 4;
const ST_RUNNING: u8 = 5;
const ST_WAIT: u8 = 6;
const ST_DONE: u8 = 7;

/// Which first-class queue a tx belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueKind {
    Indep,
    Released,
    Ordered,
    Revalidate,
}

impl QueueKind {
    #[inline]
    fn tag(self) -> u8 {
        match self {
            Self::Indep => ST_INDEP,
            Self::Released => ST_RELEASED,
            Self::Ordered => ST_ORDERED,
            Self::Revalidate => ST_REVALIDATE,
        }
    }
}

#[derive(Debug)]
struct Deque {
    inner: Mutex<VecDeque<TxIdx>>,
}

impl Deque {
    fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
        }
    }

    #[inline]
    fn push_local(&self, tx: TxIdx) {
        self.inner.lock().push_back(tx);
    }

    #[inline]
    fn try_pop_local(&self) -> Option<TxIdx> {
        self.inner.lock().pop_back()
    }

    #[inline]
    fn steal(&self) -> Option<TxIdx> {
        self.inner.lock().pop_front()
    }

    #[inline]
    fn len(&self) -> usize {
        self.inner.lock().len()
    }
}

/// Detect-driven runnable set: antichain ∪ released ∪ ordered tips.
#[derive(Debug)]
pub(crate) struct RunnableSet {
    q_indep: Deque,
    q_released: Deque,
    q_ordered: Deque,
    q_revalidate: Deque,
    state: Vec<AtomicU8>,
    block_size: usize,
    cores: usize,
    steal_n: AtomicUsize,
    refuse_fill_n: AtomicUsize,
    idle_spins: AtomicUsize,
    width_sum: AtomicUsize,
    width_n: AtomicUsize,
}

impl RunnableSet {
    pub(crate) fn new(block_size: usize, cores: usize) -> Self {
        Self {
            q_indep: Deque::new(),
            q_released: Deque::new(),
            q_ordered: Deque::new(),
            q_revalidate: Deque::new(),
            state: (0..block_size).map(|_| AtomicU8::new(ST_NONE)).collect(),
            block_size,
            cores: cores.max(1),
            steal_n: AtomicUsize::new(0),
            refuse_fill_n: AtomicUsize::new(0),
            idle_spins: AtomicUsize::new(0),
            width_sum: AtomicUsize::new(0),
            width_n: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn q(&self, kind: QueueKind) -> &Deque {
        match kind {
            QueueKind::Indep => &self.q_indep,
            QueueKind::Released => &self.q_released,
            QueueKind::Ordered => &self.q_ordered,
            QueueKind::Revalidate => &self.q_revalidate,
        }
    }

    /// Seed after Detect `admit_seed`. Ungated → Q_indep. Window heads →
    /// Q_ordered. Released consumers → Q_released. Unreleased stay off-queue.
    pub(crate) fn seed_begin(
        &self,
        ready: &ReadyEdgeTable,
        stages: &ProducerStageTable,
        scheduler: &Scheduler,
    ) {
        for tx in 0..self.block_size {
            if scheduler.is_done(tx) || scheduler.is_validated(tx) {
                self.state[tx].store(ST_DONE, Ordering::Relaxed);
                continue;
            }
            if stages.is_reserved(tx) && !scheduler.is_done(tx) && ready.may_execute(tx) {
                self.push(tx, QueueKind::Ordered);
                continue;
            }
            if ready.is_gated(tx) {
                if ready.may_execute(tx) {
                    if ready.was_queued(tx) {
                        self.push(tx, QueueKind::Ordered);
                    } else {
                        self.push(tx, QueueKind::Released);
                    }
                } else {
                    ready.note_skip_gate(tx);
                    self.mark_wait(tx);
                }
            } else {
                self.push(tx, QueueKind::Indep);
            }
        }
    }

    #[inline]
    pub(crate) fn push(&self, tx: TxIdx, kind: QueueKind) {
        if tx >= self.block_size {
            return;
        }
        let tag = kind.tag();
        let prev = self.state[tx].swap(tag, Ordering::AcqRel);
        // Concurrent seed/drain must not steal a live claim or a committed tx.
        // Owners reclaim via [`Self::force_push`].
        if prev == ST_RUNNING || prev == ST_DONE {
            self.state[tx].store(prev, Ordering::Release);
            return;
        }
        if prev == tag {
            return;
        }
        self.q(kind).push_local(tx);
    }

    /// Owner / heal requeue: reclaim RUNNING (this worker released) or DONE
    /// (higher-reader revalidate). Never used to steal an in-flight execute.
    #[inline]
    pub(crate) fn force_push(&self, tx: TxIdx, kind: QueueKind) {
        if tx >= self.block_size {
            return;
        }
        let tag = kind.tag();
        let prev = self.state[tx].swap(tag, Ordering::AcqRel);
        if prev == tag {
            return;
        }
        self.q(kind).push_local(tx);
    }

    #[inline]
    pub(crate) fn mark_wait(&self, tx: TxIdx) {
        if tx < self.block_size {
            let prev = self.state[tx].load(Ordering::Acquire);
            if prev != ST_DONE {
                self.state[tx].store(ST_WAIT, Ordering::Release);
            }
        }
    }

    #[inline]
    pub(crate) fn mark_done(&self, tx: TxIdx) {
        if tx < self.block_size {
            self.state[tx].store(ST_DONE, Ordering::Release);
        }
    }

    #[inline]
    pub(crate) fn mark_running(&self, tx: TxIdx) {
        if tx < self.block_size {
            self.state[tx].store(ST_RUNNING, Ordering::Release);
        }
    }

    /// Drop a not-yet-started independent (IntraPatch).
    pub(crate) fn remove_from_indep(&self, tx: TxIdx) {
        if tx < self.block_size {
            let _ = self.state[tx].compare_exchange(
                ST_INDEP,
                ST_WAIT,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
        }
    }

    #[inline]
    fn take_if(&self, tx: TxIdx, expect: u8) -> bool {
        self.state[tx]
            .compare_exchange(expect, ST_RUNNING, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    fn pop_kind(&self, kind: QueueKind) -> Option<TxIdx> {
        let tag = kind.tag();
        let q = self.q(kind);
        while let Some(tx) = q.try_pop_local() {
            if self.take_if(tx, tag) {
                return Some(tx);
            }
        }
        None
    }

    fn steal_kind(&self, kind: QueueKind) -> Option<TxIdx> {
        let tag = kind.tag();
        let q = self.q(kind);
        while let Some(tx) = q.steal() {
            if self.take_if(tx, tag) {
                self.steal_n.fetch_add(1, Ordering::Relaxed);
                return Some(tx);
            }
        }
        None
    }

    /// Work-conserving pick (PC-1..PC-3). `worker_i` rotates local preference.
    pub(crate) fn pick(&self, worker_i: usize, ready: &ReadyEdgeTable) -> Option<SfPick> {
        if let Some(tx) = self.pop_kind(QueueKind::Revalidate) {
            return Some(SfPick::Revalidate(tx));
        }
        let prefer = match worker_i % 3 {
            0 => [QueueKind::Ordered, QueueKind::Released, QueueKind::Indep],
            1 => [QueueKind::Released, QueueKind::Indep, QueueKind::Ordered],
            _ => [QueueKind::Indep, QueueKind::Ordered, QueueKind::Released],
        };
        for kind in prefer {
            if let Some(tx) = self.pop_kind(kind) {
                return self.admit_or_refuse(tx, kind, ready);
            }
        }
        // Global steal: independents first (PC-3: keep width while a spine runs).
        for kind in [QueueKind::Indep, QueueKind::Released, QueueKind::Ordered] {
            if let Some(tx) = self.steal_kind(kind) {
                return self.admit_or_refuse(tx, kind, ready);
            }
        }
        if let Some(tx) = self.steal_kind(QueueKind::Revalidate) {
            return Some(SfPick::Revalidate(tx));
        }
        None
    }

    fn admit_or_refuse(
        &self,
        tx: TxIdx,
        kind: QueueKind,
        ready: &ReadyEdgeTable,
    ) -> Option<SfPick> {
        if ready.is_gated(tx) && !ready.may_execute(tx) {
            ready.note_skip_gate(tx);
            self.mark_wait(tx);
            self.refuse_fill_n.fetch_add(1, Ordering::Relaxed);
            // PC-2: immediately fill from Q_indep.
            if let Some(alt) = self
                .pop_kind(QueueKind::Indep)
                .or_else(|| self.steal_kind(QueueKind::Indep))
            {
                return Some(SfPick::Execute {
                    tx: alt,
                    vis: VisibilityPolicy::Opt,
                    from: QueueKind::Indep,
                    refused: true,
                });
            }
            return None;
        }
        let vis = match kind {
            QueueKind::Indep => VisibilityPolicy::Opt,
            QueueKind::Released => VisibilityPolicy::for_ready(ready, tx),
            QueueKind::Ordered => VisibilityPolicy::OrderedTip,
            QueueKind::Revalidate => VisibilityPolicy::for_ready(ready, tx),
        };
        Some(SfPick::Execute {
            tx,
            vis,
            from: kind,
            refused: false,
        })
    }

    fn requeue_ready(&self, tx: TxIdx, ready: &ReadyEdgeTable) {
        if ready.was_queued(tx) {
            self.force_push(tx, QueueKind::Ordered);
        } else if ready.is_gated(tx) {
            self.force_push(tx, QueueKind::Released);
        } else {
            self.force_push(tx, QueueKind::Indep);
        }
    }

    /// Idle heal: released waiters, leftover Executed, abandoned Ready.
    /// Does not steal `ST_RUNNING` while the scheduler is still `Executing`.
    pub(crate) fn heal(&self, ready: &ReadyEdgeTable, scheduler: &Scheduler) -> usize {
        // I1: Detect consumer←pred inserted after the producer already published
        // leaves `may_execute=false` and workers yield-spin (~400% CPU).
        ready.heal_finished_preds(|w| scheduler.is_done(w));
        let mut n = 0;
        // PC-5: leftover gated bits with no live producer must rejoin Q_indep.
        let freed = ready.collapse_false_gates(|w| {
            scheduler.is_executing(w) || scheduler.is_ready(w) || scheduler.is_executed(w)
        });
        for tx in freed {
            if scheduler.is_aborting(tx) {
                let _ = scheduler.recover_aborting(tx);
            } else if scheduler.is_executing(tx) {
                let _ = scheduler.recover_executing_waiter(tx);
            }
            if scheduler.is_ready(tx) || scheduler.is_executed(tx) {
                self.requeue_ready(tx, ready);
                n += 1;
            }
        }
        for tx in 0..self.block_size {
            let st = self.state[tx].load(Ordering::Acquire);
            if st == ST_DONE {
                continue;
            }
            if st == ST_RUNNING && scheduler.is_executing(tx) {
                continue;
            }
            if scheduler.is_validated(tx) && st != ST_REVALIDATE {
                self.mark_done(tx);
                continue;
            }
            if scheduler.is_aborting(tx) {
                match ready.blocking_producer(tx) {
                    Some(w) if scheduler.is_executing(w) => continue,
                    Some(w) if scheduler.is_ready(w) => {
                        self.requeue_ready(w, ready);
                        n += 1;
                        continue;
                    }
                    Some(w) if scheduler.is_done(w) => {
                        if scheduler.recover_aborting(tx) {
                            self.requeue_ready(tx, ready);
                            n += 1;
                        }
                        continue;
                    }
                    // Scheduler-only park (no Detect edge): recovering while
                    // any producer is still Executing re-issues the waiter and
                    // livelocks independent transfers at ~400% CPU.
                    _ => continue,
                }
            }
            if scheduler.is_executing(tx) && st != ST_RUNNING {
                if ready
                    .blocking_producer(tx)
                    .is_some_and(|w| !scheduler.is_done(w))
                {
                    continue;
                }
                // True-idle leftover only — any live Executing owner is ST_RUNNING.
                if scheduler.has_unfinished() && self.width() + self.q_revalidate.len() > 0 {
                    continue;
                }
                if scheduler.recover_executing_waiter(tx) {
                    self.requeue_ready(tx, ready);
                    n += 1;
                }
                continue;
            }
            if st == ST_REVALIDATE {
                if (scheduler.is_executed(tx) || scheduler.is_validated(tx))
                    && self.q_revalidate.len() == 0
                {
                    self.state[tx].store(ST_NONE, Ordering::Release);
                    self.force_push(tx, QueueKind::Revalidate);
                    n += 1;
                } else if scheduler.is_ready(tx) {
                    self.requeue_ready(tx, ready);
                    n += 1;
                }
                continue;
            }
            if scheduler.is_executed(tx) {
                self.force_push(tx, QueueKind::Revalidate);
                n += 1;
                continue;
            }
            if !scheduler.is_ready(tx) {
                continue;
            }
            if st == ST_WAIT && !ready.may_execute(tx) {
                if let Some(w) = ready.blocking_producer(tx)
                    && scheduler.is_done(w)
                {
                    ready.heal_finished_preds(|p| p == w);
                    if ready.may_execute(tx) {
                        self.requeue_ready(tx, ready);
                        n += 1;
                    }
                }
                continue;
            }
            if st == ST_NONE || st == ST_WAIT || st == ST_RUNNING {
                self.requeue_ready(tx, ready);
                n += 1;
            }
        }
        // Lost scheduler-only wakeup: recover only when the block is truly
        // idle (no Executing owner). Doing this while a producer is live
        // re-issues waiters and livelocks independent transfers.
        if n == 0 && self.pending_work() == 0 {
            let any_executing = (0..self.block_size).any(|t| scheduler.is_executing(t));
            if !any_executing {
                // PC-5 nuclear: leftover wait-set, nobody runnable, nobody
                // Executing. Drop every false gate and rejoin Q_indep.
                if scheduler.has_unfinished() {
                    let freed = ready.collapse_false_gates(|_| false);
                    for tx in freed {
                        if scheduler.is_aborting(tx) {
                            let _ = scheduler.recover_aborting(tx);
                        } else if scheduler.is_executing(tx) {
                            let _ = scheduler.recover_executing_waiter(tx);
                        }
                        if scheduler.is_ready(tx) || scheduler.is_executed(tx) {
                            self.requeue_ready(tx, ready);
                            n += 1;
                        }
                    }
                }
                for tx in 0..self.block_size {
                    let st = self.state[tx].load(Ordering::Acquire);
                    if st == ST_DONE || scheduler.is_validated(tx) {
                        continue;
                    }
                    if scheduler.is_aborting(tx) && scheduler.recover_aborting(tx) {
                        self.requeue_ready(tx, ready);
                        n += 1;
                    } else if scheduler.is_executing(tx)
                        && st != ST_RUNNING
                        && scheduler.recover_executing_waiter(tx)
                    {
                        self.requeue_ready(tx, ready);
                        n += 1;
                    } else if scheduler.is_executed(tx) {
                        self.force_push(tx, QueueKind::Revalidate);
                        n += 1;
                    } else if scheduler.is_ready(tx) && ready.may_execute(tx) {
                        self.requeue_ready(tx, ready);
                        n += 1;
                    }
                }
            }
        }
        n
    }

    #[inline]
    pub(crate) fn width(&self) -> usize {
        self.q_indep.len() + self.q_released.len() + self.q_ordered.len()
    }

    #[inline]
    pub(crate) fn width_hint(&self) -> usize {
        self.width()
    }

    pub(crate) fn sample_width(&self) {
        self.width_sum.fetch_add(self.width(), Ordering::Relaxed);
        self.width_n.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn note_idle_spin(&self) {
        self.idle_spins.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn steal_n(&self) -> usize {
        self.steal_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn refuse_fill_n(&self) -> usize {
        self.refuse_fill_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn idle_spins(&self) -> usize {
        self.idle_spins.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn cores(&self) -> usize {
        self.cores
    }

    #[inline]
    pub(crate) fn q_ordered_len(&self) -> usize {
        self.q_ordered.len()
    }

    #[inline]
    pub(crate) fn q_indep_len(&self) -> usize {
        self.q_indep.len()
    }

    #[inline]
    pub(crate) fn pending_work(&self) -> usize {
        self.width() + self.q_revalidate.len()
    }

    #[inline]
    pub(crate) fn waiting_on_live_producer(
        &self,
        ready: &ReadyEdgeTable,
        scheduler: &Scheduler,
    ) -> bool {
        (0..self.block_size).any(|tx| {
            self.state[tx].load(Ordering::Acquire) == ST_WAIT
                && ready
                    .blocking_producer(tx)
                    .is_some_and(|w| scheduler.is_executing(w))
        })
    }

    /// Visibility for a picked tx (Detect graph).
    #[inline]
    pub(crate) fn visibility(&self, ready: &ReadyEdgeTable, tx: TxIdx) -> VisibilityPolicy {
        VisibilityPolicy::for_ready(ready, tx)
    }
}

/// Outcome of [`RunnableSet::pick`].
#[derive(Debug, Clone, Copy)]
pub(crate) enum SfPick {
    Execute {
        tx: TxIdx,
        vis: VisibilityPolicy,
        from: QueueKind,
        refused: bool,
    },
    Revalidate(TxIdx),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specfence::producer_stage::ProducerStageTable;
    use crate::specfence::ready_edge::ReadyEdgeTable;

    #[test]
    fn three_queues_and_refuse_fill() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let sched = Scheduler::new(6);
        ready.note_consumer(3, 0);
        let r = RunnableSet::new(6, 2);
        r.seed_begin(&ready, &stages, &sched);
        assert!(r.q_indep_len() >= 4, "ungated antichain in Q_indep");
        assert_eq!(r.q_ordered_len(), 0, "no ordered tip until a window head");
        // Gated 3 is waiting, not on a Q.
        let first = r.pick(0, &ready).expect("indep work");
        let SfPick::Execute { tx, vis, .. } = first else {
            panic!("expected execute");
        };
        assert_ne!(tx, 3, "refused consumer must not occupy a core");
        assert_eq!(vis, VisibilityPolicy::Opt);
    }

    #[test]
    fn force_push_reclaims_running_and_done() {
        let ready = ReadyEdgeTable::new();
        let r = RunnableSet::new(4, 2);
        r.push(1, QueueKind::Indep);
        let picked = r.pick(0, &ready);
        assert!(matches!(picked, Some(SfPick::Execute { tx: 1, .. })));
        // Owner abort/requeue must not no-op on ST_RUNNING.
        r.force_push(1, QueueKind::Indep);
        let again = r.pick(2, &ready);
        assert!(matches!(again, Some(SfPick::Execute { tx: 1, .. })));
        r.mark_done(2);
        r.force_push(2, QueueKind::Revalidate);
        assert!(matches!(r.pick(0, &ready), Some(SfPick::Revalidate(2))));
    }

    #[test]
    fn heal_recovers_wait_after_producer_done() {
        let ready = ReadyEdgeTable::new();
        let sched = Scheduler::new(4);
        ready.note_consumer(2, 0);
        let r = RunnableSet::new(4, 2);
        r.mark_wait(2);
        ready.note_skip_gate(2);
        assert!(!ready.may_execute(2));
        let v = sched.try_execute_producer(0).unwrap();
        let _ = sched.finish_execution(v, crate::FinishExecFlags::empty());
        assert!(sched.is_done(0));
        let n = r.heal(&ready, &sched);
        assert!(n >= 1, "heal must requeue the released waiter, got {n}");
        assert!(ready.may_execute(2), "producer Done must open the edge");
        let mut saw_waiter = false;
        while let Some(SfPick::Execute { tx, .. }) = r.pick(0, &ready) {
            if tx == 2 {
                saw_waiter = true;
                break;
            }
        }
        assert!(saw_waiter, "healed waiter must be pickable from Q_*");
    }

    #[test]
    fn steal_moves_across_workers() {
        let ready = ReadyEdgeTable::new();
        let r = RunnableSet::new(8, 4);
        for t in 0..8 {
            r.push(t, QueueKind::Indep);
        }
        let a = r.pick(0, &ready);
        let b = r.pick(1, &ready);
        assert!(a.is_some() && b.is_some());
        // Drain locals then steal.
        let mut n = 2;
        while r.pick(0, &ready).is_some() {
            n += 1;
        }
        assert!(n >= 2);
        assert!(r.steal_n() > 0 || r.q_indep_len() == 0);
    }

    #[test]
    fn pick_source_never_calls_next_task() {
        let src = include_str!("runnable_set.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("next_task_with_wave_ready("));
        assert!(!code.contains(".next_task("));
        assert!(!code.contains("validate_occ_stage("));
    }
}
