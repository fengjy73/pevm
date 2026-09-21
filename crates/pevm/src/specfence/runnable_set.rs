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
                self.push(tx, QueueKind::Released);
                continue;
            }
            if ready.is_gated(tx) {
                if ready.may_execute(tx) {
                    self.push(tx, QueueKind::Released);
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
        // Independents first (PC-3). Worker 0 used to prefer Ordered/Released
        // and 1-core starved Q_indep=29 behind one Released park-requeue
        // (19469101 pending=30 / live_wait=false).
        let prefer = match worker_i % 3 {
            0 => [QueueKind::Indep, QueueKind::Released, QueueKind::Ordered],
            1 => [QueueKind::Indep, QueueKind::Ordered, QueueKind::Released],
            _ => [QueueKind::Released, QueueKind::Indep, QueueKind::Ordered],
        };
        // One gated !may_execute head must not hide a later runnable in the
        // same deque (19469101: pending=30 stuck, schedule broke on first None).
        for kind in prefer {
            while let Some(tx) = self.pop_kind(kind) {
                if let Some(p) = self.admit_or_refuse(tx, kind, ready) {
                    return Some(p);
                }
            }
        }
        // Global steal: independents first (PC-3: keep width while a spine runs).
        for kind in [QueueKind::Indep, QueueKind::Released, QueueKind::Ordered] {
            while let Some(tx) = self.steal_kind(kind) {
                if let Some(p) = self.admit_or_refuse(tx, kind, ready) {
                    return Some(p);
                }
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
        if ready.leftover_surplus(tx) || (ready.is_gated(tx) && !ready.may_execute(tx)) {
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
        // Gated !may_execute on a queue is the 19807137 refuse mill:
        // pick mark_waits, heal force_pushes, pending stays ~60, idle
        // ungate never runs.
        if ready.leftover_surplus(tx) || (ready.is_gated(tx) && !ready.may_execute(tx)) {
            self.mark_wait(tx);
            return;
        }
        // Never Q_ordered from heal: 19807137 mills ~40 OrderedTip heads
        // even when only location-admitted txs are pushed (8148ded).
        // Released/Indep + plant waits; FullReplay stays off Ordered.
        if ready.is_gated(tx) {
            self.force_push(tx, QueueKind::Released);
        } else {
            self.force_push(tx, QueueKind::Indep);
        }
    }

    /// Last-ditch when queues are empty, nothing is `ST_RUNNING`, and the
    /// scheduler still has unfinished txs. Recovers Aborting / Executing
    /// leftovers and requeues Ready/Executed. 8-core 19469101 flaked because
    /// Detect still named a ghost producer so ordinary heal no-op'd.
    pub(crate) fn force_idle_recover(
        &self,
        ready: &ReadyEdgeTable,
        scheduler: &Scheduler,
    ) -> usize {
        if self.pending_work() > 0 {
            return 0;
        }
        if (0..self.block_size).any(|t| self.state[t].load(Ordering::Acquire) == ST_RUNNING) {
            return 0;
        }
        let leftover_next =
            ready.take_finished_leftover_min(|t| scheduler.is_validated(t));
        // Keep unfinished producers. `is_executing && ST_RUNNING` is always
        // false here (we just proved no ST_RUNNING) and nuclear-ungated the
        // leftover chain into a 32-head Q_released mill (19807137). Ghost
        // Ready producers are requeued below via has_known_waiters.
        let freed = ready.collapse_false_gates(|w| !scheduler.is_done(w));
        let mut n = 0;
        for tx in leftover_next.into_iter().chain(ready.live_leftover_head()) {
            if scheduler.is_validated(tx) {
                continue;
            }
            if scheduler.is_aborting(tx) {
                let _ = scheduler.recover_aborting(tx);
            } else if scheduler.is_executing(tx) {
                let _ = scheduler.recover_executing_waiter(tx);
            }
            if scheduler.is_executed(tx) {
                self.force_push(tx, QueueKind::Revalidate);
                n += 1;
            } else if scheduler.is_ready(tx) {
                self.requeue_ready(tx, ready);
                n += 1;
            }
        }
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
            if scheduler.is_validated(tx) {
                self.mark_done(tx);
                continue;
            }
            if ready.leftover_surplus(tx) {
                continue;
            }
            if self.state[tx].load(Ordering::Acquire) == ST_RUNNING {
                continue;
            }
            if scheduler.is_executing(tx) && scheduler.recover_executing_waiter(tx) {
                self.requeue_ready(tx, ready);
                n += 1;
                continue;
            }
            if scheduler.is_aborting(tx) && scheduler.recover_aborting(tx) {
                self.requeue_ready(tx, ready);
                n += 1;
                continue;
            }
            if scheduler.is_executed(tx) {
                self.force_push(tx, QueueKind::Revalidate);
                n += 1;
                continue;
            }
            if scheduler.is_ready(tx) && (ready.may_execute(tx) || ready.has_known_waiters(tx)) {
                self.requeue_ready(tx, ready);
                n += 1;
            }
        }
        n
    }

    /// Idle heal: released waiters, leftover Executed, abandoned Ready.
    /// Does not steal `ST_RUNNING` while the scheduler is still `Executing`.
    pub(crate) fn heal(&self, ready: &ReadyEdgeTable, scheduler: &Scheduler) -> usize {
        // I1: Detect consumer←pred inserted after the producer already published
        // leaves `may_execute=false` and workers yield-spin (~400% CPU).
        ready.heal_finished_preds(|w| scheduler.is_done(w));
        let mut n = 0;
        // Ghost Executing *producer* (worker already left, not ST_RUNNING)
        // that still gates waiters. Recover even when queues are non-empty —
        // otherwise 30 Released heads sit behind a leftover root (19469101).
        // Recovering waiters into a live antichain is the 390% mill; this
        // path is producer-only (`has_known_waiters`).
        for tx in 0..self.block_size {
            if self.state[tx].load(Ordering::Acquire) == ST_RUNNING {
                continue;
            }
            if ready.leftover_surplus(tx) {
                continue;
            }
            if !scheduler.is_executing(tx) || !ready.has_known_waiters(tx) {
                continue;
            }
            if ready.blocking_producer(tx).is_some_and(|w| {
                !scheduler.is_done(w) && self.state[w].load(Ordering::Acquire) == ST_RUNNING
            }) {
                continue;
            }
            if scheduler.recover_executing_waiter(tx) {
                self.requeue_ready(tx, ready);
                n += 1;
            }
        }
        // WaitForDependency leftovers (Executing, worker gone). Only when
        // queues are empty — recovering into a live antichain mills at ~390%.
        if self.pending_work() == 0 {
            for tx in 0..self.block_size {
                let st = self.state[tx].load(Ordering::Acquire);
                if st == ST_RUNNING || st == ST_DONE || !scheduler.is_executing(tx) {
                    continue;
                }
                if ready.leftover_surplus(tx) {
                    continue;
                }
                if ready.blocking_producer(tx).is_some_and(|w| {
                    !scheduler.is_done(w) && self.state[w].load(Ordering::Acquire) == ST_RUNNING
                }) {
                    continue;
                }
                if scheduler.recover_executing_waiter(tx) {
                    self.requeue_ready(tx, ready);
                    n += 1;
                }
            }
        }
        // PC-5: leftover gated bits with no live producer must rejoin Q_indep.
        let freed = ready.collapse_false_gates(|w| {
            scheduler.is_executing(w)
                || scheduler.is_ready(w)
                || scheduler.is_executed(w)
                || scheduler.is_aborting(w)
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
            if ready.leftover_surplus(tx) {
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
                // Producer already published (or never gated): lost wake.
                // Recovering an still-gated Aborting into a live antichain
                // incarnation++ mills (6196166 reuse / 19807137 first SF).
                if ready.may_execute(tx) {
                    // Ungated Aborting + live antichain: add_dependency park
                    // recovered too early (incarnation++ mill). Wait for idle.
                    if !ready.is_gated(tx) && self.pending_work() > 0 {
                        continue;
                    }
                    if scheduler.recover_aborting(tx) {
                        self.requeue_ready(tx, ready);
                        n += 1;
                    }
                    continue;
                }
                match ready.blocking_producer(tx) {
                    Some(w)
                        if scheduler.is_executing(w)
                            && self.state[w].load(Ordering::Acquire) == ST_RUNNING =>
                    {
                        continue;
                    }
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
                    // True idle only. Detect naming another Aborting leftover
                    // (8-core 19469101 pending=0) still recovers here.
                    _ => {
                        if self.pending_work() == 0 && scheduler.recover_aborting(tx) {
                            self.requeue_ready(tx, ready);
                            n += 1;
                        }
                        continue;
                    }
                }
            }
            if scheduler.is_executing(tx) && st != ST_RUNNING {
                // WaitForDependency leftover: worker already left. Do not
                // recover into a live antichain (390% execute-park mill).
                // True-idle recover is the pending_work==0 pass below.
                if self.pending_work() > 0 {
                    continue;
                }
                if ready.blocking_producer(tx).is_some_and(|w| {
                    !scheduler.is_done(w) && self.state[w].load(Ordering::Acquire) == ST_RUNNING
                }) {
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
        // Lost scheduler-only wakeup. WaitForDependency parks leave status
        // `Executing` after `SfExec::Blocked` (worker has left). Those ghosts
        // made `any_executing=true` and skipped recover — 19469101 / 19807137
        // / complete_arch idle-spin. A live owner is `ST_RUNNING`, not a
        // leftover Executing bit.
        if n == 0 && self.pending_work() == 0 {
            if let Some(next) = ready
                .take_finished_leftover_min(|t| scheduler.is_validated(t))
            {
                if !scheduler.is_validated(next) {
                    if scheduler.is_aborting(next) {
                        let _ = scheduler.recover_aborting(next);
                    } else if scheduler.is_executing(next) {
                        let _ = scheduler.recover_executing_waiter(next);
                    }
                    if scheduler.is_executed(next) {
                        self.force_push(next, QueueKind::Revalidate);
                        n += 1;
                    } else if scheduler.is_ready(next) {
                        self.requeue_ready(next, ready);
                        n += 1;
                    }
                }
            }
            let any_running =
                (0..self.block_size).any(|t| self.state[t].load(Ordering::Acquire) == ST_RUNNING);
            for tx in 0..self.block_size {
                let st = self.state[tx].load(Ordering::Acquire);
                if st == ST_DONE || st == ST_RUNNING || scheduler.is_validated(tx) {
                    continue;
                }
                if ready.leftover_surplus(tx) {
                    continue;
                }
                if scheduler.is_executing(tx) {
                    if scheduler.recover_executing_waiter(tx) {
                        self.requeue_ready(tx, ready);
                        n += 1;
                    }
                } else if scheduler.is_aborting(tx) {
                    let live_owner = ready.blocking_producer(tx).is_some_and(|w| {
                        !scheduler.is_done(w) && self.state[w].load(Ordering::Acquire) == ST_RUNNING
                    });
                    if !live_owner && scheduler.recover_aborting(tx) {
                        self.requeue_ready(tx, ready);
                        n += 1;
                    }
                } else if scheduler.is_executed(tx) {
                    self.force_push(tx, QueueKind::Revalidate);
                    n += 1;
                } else if scheduler.is_ready(tx) && ready.may_execute(tx) {
                    self.requeue_ready(tx, ready);
                    n += 1;
                }
            }
            if n == 0 && !any_running && scheduler.has_unfinished() {
                // PC-5: leftover gate only when the producer is gone.
                // ` |_| false` ungated waiters of a still-Ready producer and
                // re-armed the 1-core Opt mill (19469101).
                let freed = ready.collapse_false_gates(|w| !scheduler.is_done(w));
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

    /// Hang-trace: leaked `ST_RUNNING` after a missed `try_execute`.
    #[inline]
    pub(crate) fn running_n(&self) -> usize {
        (0..self.block_size)
            .filter(|&t| self.state[t].load(Ordering::Acquire) == ST_RUNNING)
            .count()
    }

    #[inline]
    pub(crate) fn is_running(&self, tx: TxIdx) -> bool {
        tx < self.block_size && self.state[tx].load(Ordering::Acquire) == ST_RUNNING
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
    pub(crate) fn q_released_len(&self) -> usize {
        self.q_released.len()
    }

    #[inline]
    pub(crate) fn q_revalidate_len(&self) -> usize {
        self.q_revalidate.len()
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
                && ready.blocking_producer(tx).is_some_and(|w| {
                    scheduler.is_executing(w) && self.state[w].load(Ordering::Acquire) == ST_RUNNING
                })
        })
    }

    /// Visibility for a picked tx (Detect graph).
    #[inline]
    pub(crate) fn visibility(&self, ready: &ReadyEdgeTable, tx: TxIdx) -> VisibilityPolicy {
        VisibilityPolicy::for_ready(ready, tx)
    }

    #[cfg(test)]
    pub(crate) fn test_requeue_ready(&self, tx: TxIdx, ready: &ReadyEdgeTable) {
        self.requeue_ready(tx, ready);
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
    fn heal_recovers_wait_for_dependency_executing_leftover() {
        let ready = ReadyEdgeTable::new();
        let sched = Scheduler::new(4);
        let r = RunnableSet::new(4, 2);
        let _v = sched.try_execute_producer(1).unwrap();
        r.mark_wait(1);
        assert!(sched.is_executing(1), "WaitForDependency leaves Executing");
        assert_eq!(r.pending_work(), 0);
        let n = r.heal(&ready, &sched);
        assert!(
            n >= 1,
            "Executing leftover with no ST_RUNNING must recover, got {n}"
        );
        assert!(sched.is_ready(1));
        let mut saw = false;
        while let Some(SfPick::Execute { tx, .. }) = r.pick(0, &ready) {
            if tx == 1 {
                saw = true;
                break;
            }
        }
        assert!(saw, "recovered leftover must be pickable");
    }

    #[test]
    fn waiting_on_live_requires_st_running() {
        let ready = ReadyEdgeTable::new();
        let sched = Scheduler::new(4);
        let r = RunnableSet::new(4, 2);
        ready.note_consumer(2, 1);
        r.mark_wait(2);
        let _v = sched.try_execute_producer(1).unwrap();
        r.mark_wait(1);
        assert!(
            !r.waiting_on_live_producer(&ready, &sched),
            "ghost Executing producer is not a live owner"
        );
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
    fn pick_does_not_starve_indep_behind_released() {
        let ready = ReadyEdgeTable::new();
        let r = RunnableSet::new(6, 1);
        ready.note_consumer(2, 0);
        r.force_push(2, QueueKind::Released);
        r.force_push(5, QueueKind::Indep);
        let SfPick::Execute { tx, vis, .. } = r.pick(0, &ready).expect("indep") else {
            panic!("expected execute");
        };
        assert_eq!(tx, 5, "worker 0 must not mill Released ahead of Q_indep");
        assert_eq!(vis, VisibilityPolicy::Opt);
    }

    #[test]
    fn pick_skips_gated_head_and_takes_later_runnable() {
        let ready = ReadyEdgeTable::new();
        let r = RunnableSet::new(6, 1);
        ready.note_consumer(1, 0);
        // pop_back: last push is the head. Gated 1 must be skipped.
        r.force_push(4, QueueKind::Released);
        r.force_push(1, QueueKind::Released);
        let SfPick::Execute { tx, .. } = r.pick(0, &ready).expect("later runnable") else {
            panic!("expected execute");
        };
        assert_eq!(tx, 4, "must skip gated !may_execute head 1");
    }

    #[test]
    fn force_idle_recovers_waiter_of_ghost_ready_producer() {
        let ready = ReadyEdgeTable::new();
        let sched = Scheduler::new(4);
        let r = RunnableSet::new(4, 1);
        let _v0 = sched.try_execute_producer(0).unwrap();
        // 0 stays Ready after a fake abort-ready; Detect still names it.
        sched.recover_executing_waiter(0);
        ready.note_consumer_on(2, 0, None);
        r.mark_wait(2);
        assert_eq!(r.pending_work(), 0);
        assert!(!ready.may_execute(2));
        let n = r.force_idle_recover(&ready, &sched);
        assert!(
            n >= 1,
            "idle must free waiter of non-running producer, n={n}"
        );
        assert!(r.pending_work() > 0 || ready.may_execute(2) || sched.is_ready(0));
    }

    #[test]
    fn heal_orders_location_cohort_not_anonymous_fanin() {
        let ready = ReadyEdgeTable::new();
        let r = RunnableSet::new(6, 1);
        let wave = crate::specfence::wave::WaveParkTable::new();
        ready.note_consumer_on(2, 0, Some(0xabc));
        ready.note_consumer_on(3, 0, None);
        ready.note_producer_done(0, &wave);
        assert!(ready.admitted_on_location(2));
        assert!(!ready.admitted_on_location(3));
        assert!(ready.was_queued(3), "anonymous fan-in is still was_queued");
        assert!(ready.may_execute(2) && ready.may_execute(3));
        r.test_requeue_ready(2, &ready);
        r.test_requeue_ready(3, &ready);
        let mut from2 = None;
        let mut from3 = None;
        while let Some(p) = r.pick(0, &ready) {
            match p {
                SfPick::Execute { tx, from, .. } => {
                    if tx == 2 {
                        from2 = Some(from);
                    } else if tx == 3 {
                        from3 = Some(from);
                    }
                    r.mark_done(tx);
                }
                SfPick::Revalidate(tx) => r.mark_done(tx),
            }
        }
        assert_eq!(
            from2,
            Some(QueueKind::Released),
            "location cohort must not take Q_ordered (19807137 mill)"
        );
        assert_eq!(
            from3,
            Some(QueueKind::Released),
            "anonymous fan-in → Q_released, not Ordered"
        );
    }

    #[test]
    fn heal_does_not_requeue_gated_unready() {
        let ready = ReadyEdgeTable::new();
        let sched = Scheduler::new(4);
        let r = RunnableSet::new(4, 1);
        ready.note_consumer_on(2, 0, None);
        let _v0 = sched.try_execute_producer(0).unwrap();
        r.mark_wait(0);
        r.force_push(2, QueueKind::Released);
        r.force_push(3, QueueKind::Indep);
        assert!(!ready.may_execute(2));
        let _ = r.heal(&ready, &sched);
        while let Some(p) = r.pick(0, &ready) {
            match p {
                SfPick::Execute { tx, .. } => {
                    assert_ne!(tx, 2, "gated !may_execute must stay off queues");
                    r.mark_done(tx);
                }
                SfPick::Revalidate(tx) => r.mark_done(tx),
            }
        }
        assert!(
            !ready.may_execute(2) || r.pending_work() == 0,
            "waiter 2 must not refuse-mill on Released"
        );
    }

    #[test]
    fn heal_does_not_recover_aborting_into_live_antichain() {
        let ready = ReadyEdgeTable::new();
        let sched = Scheduler::new(4);
        let r = RunnableSet::new(4, 1);
        let _v0 = sched.try_execute_producer(0).unwrap();
        let _v1 = sched.try_execute_producer(1).unwrap();
        assert!(sched.add_dependency(1, 0));
        assert!(sched.is_aborting(1));
        ready.note_consumer_on(1, 0, None);
        r.mark_wait(1);
        r.force_push(3, QueueKind::Indep);
        let n = r.heal(&ready, &sched);
        assert!(
            sched.is_aborting(1),
            "gated Aborting must wait for the producer, heal n={n}"
        );
    }

    #[test]
    fn heal_recovers_aborting_chain_with_no_running_owner() {
        let ready = ReadyEdgeTable::new();
        let sched = Scheduler::new(4);
        let r = RunnableSet::new(4, 1);
        ready.note_consumer(2, 1);
        let _v1 = sched.try_execute_producer(1).unwrap();
        let _v2 = sched.try_execute_producer(2).unwrap();
        assert!(sched.add_dependency(2, 1));
        assert!(sched.is_aborting(2));
        r.mark_wait(1);
        r.mark_wait(2);
        assert_eq!(r.pending_work(), 0);
        let n = r.heal(&ready, &sched);
        assert!(
            n >= 1,
            "Aborting leftovers with no ST_RUNNING owner must recover, got {n}"
        );
        assert!(sched.is_ready(2) || sched.is_ready(1));
    }

    #[test]
    fn heal_recovers_ghost_producer_while_waiters_queued() {
        let ready = ReadyEdgeTable::new();
        let sched = Scheduler::new(4);
        let r = RunnableSet::new(4, 1);
        ready.note_consumer(2, 1);
        ready.note_consumer(3, 1);
        let _v = sched.try_execute_producer(1).unwrap();
        r.mark_wait(1);
        r.force_push(2, QueueKind::Released);
        r.force_push(3, QueueKind::Released);
        assert!(r.pending_work() >= 2, "waiters stay queued");
        assert!(sched.is_executing(1));
        let n = r.heal(&ready, &sched);
        assert!(n >= 1, "ghost producer with waiters must recover, got {n}");
        assert!(sched.is_ready(1));
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
