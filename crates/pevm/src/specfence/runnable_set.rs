//! RunnableSet — three first-class queues + work-steal (SF-PS T1 / PC).
//!
//! `Q_indep` / `Q_released` / `Q_ordered` are real deques. Pick never calls
//! `Scheduler::next_task*`. Refuse = leave the head waiting and steal another
//! independent (PC-2). Soft=0: WaitReleased txs are not on any Q.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

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
/// Legacy tag. Chain hops stay `ST_INDEP` on [`RunnableSet::spine_q`].
/// Parking them here hung reuse: heal could not requeue the writer the
/// reader was waiting on.
const ST_CHAIN: u8 = 8;
/// Hop sitting in [`RunnableSet::spine_slot`]. Any free worker may claim it.
const ST_SPINE: u8 = 9;
/// `spine_owner_tx` while a worker is mid-pop of [`RunnableSet::spine_q`].
const SPINE_LOCK: usize = usize::MAX - 1;
/// Already-claimed Admit-Unfenced txs returned without another queue lock.
const BATCH_K: usize = 4;

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
    /// Per-worker Admit-Unfenced deques. Owner pops the back; thieves the front.
    locals: Vec<Deque>,
    /// Claimed (ST_RUNNING) width txs for this worker. Not the spine hop.
    batches: Vec<Mutex<VecDeque<TxIdx>>>,
    /// Locals + batches. Global deques are counted by their own locks.
    local_queued: AtomicUsize,
    /// Ordered hops only. Owner pops the back (chain head). Never stolen.
    spine_q: Deque,
    /// Set when a popped hop's predecessor has not published. Stops the
    /// pick loop from deferring the same hop sixteen times.
    spine_pause: AtomicBool,
    /// Next ordered hop. `usize::MAX` means empty. Never stolen.
    spine_slot: AtomicUsize,
    /// `worker_i + 1` while that worker holds a spine claim. 0 if none.
    spine_holder: AtomicUsize,
    spine_owner_tx: AtomicUsize,
    spine_cores_max: AtomicUsize,
    refuse_gated: AtomicUsize,
    gated_pick: AtomicUsize,
    local_hits: AtomicUsize,
    help_release_n: AtomicUsize,
    steal_cursor: AtomicUsize,
    /// 1 when `tx` is on the ordered writer chain. Written before workers start.
    chain_bit: Vec<AtomicU8>,
}

impl RunnableSet {
    pub(crate) fn new(block_size: usize, cores: usize) -> Self {
        let cores = cores.max(1);
        Self {
            q_indep: Deque::new(),
            q_released: Deque::new(),
            q_ordered: Deque::new(),
            q_revalidate: Deque::new(),
            state: (0..block_size).map(|_| AtomicU8::new(ST_NONE)).collect(),
            block_size,
            cores,
            steal_n: AtomicUsize::new(0),
            refuse_fill_n: AtomicUsize::new(0),
            idle_spins: AtomicUsize::new(0),
            width_sum: AtomicUsize::new(0),
            width_n: AtomicUsize::new(0),
            locals: (0..cores).map(|_| Deque::new()).collect(),
            batches: (0..cores).map(|_| Mutex::new(VecDeque::new())).collect(),
            local_queued: AtomicUsize::new(0),
            spine_q: Deque::new(),
            spine_pause: AtomicBool::new(false),
            spine_slot: AtomicUsize::new(usize::MAX),
            spine_holder: AtomicUsize::new(0),
            spine_owner_tx: AtomicUsize::new(usize::MAX),
            spine_cores_max: AtomicUsize::new(0),
            refuse_gated: AtomicUsize::new(0),
            gated_pick: AtomicUsize::new(0),
            local_hits: AtomicUsize::new(0),
            help_release_n: AtomicUsize::new(0),
            steal_cursor: AtomicUsize::new(0),
            chain_bit: (0..block_size).map(|_| AtomicU8::new(0)).collect(),
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
        crit_head: Option<TxIdx>,
    ) {
        // Index order is the suffix proxy. A learned crit head is pushed
        // last so the local LIFO pop starts that chain before lower indices.
        // Steal still pops the high-index end (antichain tail).
        let head = crit_head.filter(|&tx| tx < self.block_size);
        for tx in (0..self.block_size).rev() {
            if Some(tx) == head {
                continue;
            }
            self.seed_one(tx, ready, stages, scheduler);
        }
        if let Some(tx) = head {
            self.seed_one(tx, ready, stages, scheduler);
        }
    }

    fn seed_one(
        &self,
        tx: TxIdx,
        ready: &ReadyEdgeTable,
        stages: &ProducerStageTable,
        scheduler: &Scheduler,
    ) {
        if scheduler.is_done(tx) || scheduler.is_validated(tx) {
            self.state[tx].store(ST_DONE, Ordering::Relaxed);
            return;
        }
        if stages.is_reserved(tx) && !scheduler.is_done(tx) && ready.may_execute(tx) {
            self.push(tx, QueueKind::Released);
            return;
        }
        if ready.is_gated(tx) {
            if ready.may_execute(tx) {
                self.push(tx, QueueKind::Released);
            } else {
                ready.note_skip_gate(tx);
                self.mark_wait(tx);
            }
            return;
        }
        // Learned WAW successor: off-queue until the pred commits, still
        // ungated so the resume stays on the Opt path.
        if let Some(pred) = ready.blocking_producer(tx)
            && !ready.is_writer_done(pred)
        {
            self.mark_wait(tx);
            return;
        }
        self.push(tx, QueueKind::Indep);
    }

    /// Move the seeded antichain onto per-worker deques. Chain hops go to
    /// `spine_q` (head at the back) and are never stolen. Parking the tail
    /// off-queue deadlocked reuse. Call once, before workers.
    pub(crate) fn shard_width(&self, members: &[TxIdx]) {
        for &tx in members {
            if tx < self.block_size {
                self.chain_bit[tx].store(1, Ordering::Release);
            }
        }
        let mut drained = Vec::new();
        while let Some(tx) = self.q_indep.try_pop_local() {
            drained.push(tx);
        }
        let mut chain = members
            .iter()
            .copied()
            .filter(|&tx| tx < self.block_size)
            .collect::<Vec<_>>();
        chain.sort_unstable();
        chain.dedup();
        let mut rr = 0usize;
        for tx in drained {
            if tx >= self.block_size || chain.binary_search(&tx).is_ok() {
                continue;
            }
            let w = rr % self.cores;
            rr += 1;
            self.locals[w].push_local(tx);
            self.local_queued.fetch_add(1, Ordering::Relaxed);
        }
        // Highest index first, so pop_back is the chain head.
        for tx in chain.into_iter().rev() {
            if self.state[tx].load(Ordering::Acquire) == ST_INDEP {
                self.spine_q.push_local(tx);
            }
        }
    }

    #[inline]
    fn is_chain_member(&self, tx: TxIdx) -> bool {
        tx < self.block_size && self.chain_bit[tx].load(Ordering::Acquire) == 1
    }

    fn place(&self, tx: TxIdx, kind: QueueKind) {
        // Chain hops never join a width deque: steal would pop the tail.
        // Do not offer_spine here. A failed `try_execute` re-offered into the
        // slot pinned every pick on that hop.
        if kind == QueueKind::Indep && self.is_chain_member(tx) {
            self.spine_q.push_local(tx);
            return;
        }
        self.q(kind).push_local(tx);
    }

    /// Publish the next ordered hop. True only when this call filled an empty
    /// slot. Already-staged and in-flight hops return false so idle help does
    /// not spin.
    pub(crate) fn offer_spine(&self, tx: TxIdx) -> bool {
        if tx >= self.block_size || !self.is_chain_member(tx) {
            return false;
        }
        loop {
            let prev = self.state[tx].load(Ordering::Acquire);
            if prev == ST_RUNNING || prev == ST_DONE || prev == ST_REVALIDATE {
                return false;
            }
            if prev == ST_SPINE {
                return false;
            }
            if self.state[tx]
                .compare_exchange(prev, ST_SPINE, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            match self.spine_slot.compare_exchange(
                usize::MAX,
                tx,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.spine_pause.store(false, Ordering::Release);
                    return true;
                }
                Err(cur) if cur == tx => return false,
                Err(_) => {
                    let _ = self.state[tx].compare_exchange(
                        ST_SPINE,
                        prev,
                        Ordering::AcqRel,
                        Ordering::Relaxed,
                    );
                    return false;
                }
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
        self.place(tx, kind);
    }

    /// Owner finished a failed claim. Only the worker that holds
    /// `ST_RUNNING` may publish the tx back. A swap would steal another
    /// worker's running bit (double execute).
    #[inline]
    pub(crate) fn release_owner(&self, tx: TxIdx, kind: QueueKind) -> bool {
        if tx >= self.block_size {
            return false;
        }
        let tag = kind.tag();
        if self.state[tx]
            .compare_exchange(ST_RUNNING, tag, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        // The hop left `ST_RUNNING`. A stale owner blocks every later claim.
        self.clear_spine_claim(tx);
        self.place(tx, kind);
        true
    }

    /// Owner requeue: this worker is done with the claim (failed pick or
    /// validate requeue). Swaps `ST_RUNNING` onto the queue.
    #[inline]
    pub(crate) fn force_push(&self, tx: TxIdx, kind: QueueKind) {
        if tx >= self.block_size {
            return;
        }
        let tag = kind.tag();
        let prev = self.state[tx].swap(tag, Ordering::AcqRel);
        if prev == ST_RUNNING {
            self.clear_spine_claim(tx);
        }
        if prev == tag {
            return;
        }
        self.place(tx, kind);
    }

    /// Heal / drain wake. Does not take `ST_RUNNING`: that claim is an
    /// in-flight execute, and swapping it lets a second worker enter the
    /// same result slot (6196166 double free, 3356896 SEGV).
    #[inline]
    pub(crate) fn wake_idle(&self, tx: TxIdx, kind: QueueKind) -> bool {
        if tx >= self.block_size {
            return false;
        }
        let tag = kind.tag();
        let prev = self.state[tx].load(Ordering::Acquire);
        // `ST_DONE` is a finished claim, not an in-flight execute. Higher
        // readers and rewind requeues must still land (335 n_unf stuck,
        // has_unfinished false). Only `ST_RUNNING` is a live slot.
        if prev == ST_RUNNING {
            return false;
        }
        if prev == tag {
            return true;
        }
        if self.state[tx]
            .compare_exchange(prev, tag, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        self.place(tx, kind);
        true
    }

    /// Picker lost `try_execute`. Release only this claim — a plain
    /// `mark_wait` store clobbers a live owner if the bit was stolen.
    #[inline]
    pub(crate) fn release_running(&self, tx: TxIdx) {
        if tx >= self.block_size {
            return;
        }
        let _ = self.state[tx].compare_exchange(
            ST_RUNNING,
            ST_WAIT,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
        self.clear_spine_claim(tx);
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

    /// Wave refuse while the owner may still be inside `try_execute_sf`.
    /// `mark_wait` would clear `ST_RUNNING` and heal would treat a live
    /// execute as a ghost (`recover_executing` → second entry, heap abort).
    #[inline]
    pub(crate) fn note_wait_unless_running(&self, tx: TxIdx) {
        if tx >= self.block_size {
            return;
        }
        let prev = self.state[tx].load(Ordering::Acquire);
        if prev == ST_DONE || prev == ST_RUNNING {
            return;
        }
        let _ = self.state[tx].compare_exchange(prev, ST_WAIT, Ordering::AcqRel, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn mark_done(&self, tx: TxIdx) {
        if tx < self.block_size {
            self.state[tx].store(ST_DONE, Ordering::Release);
            self.clear_spine_claim(tx);
            if self.is_chain_member(tx) {
                self.spine_pause.store(false, Ordering::Release);
            }
        }
    }

    /// Predecessor has not published. Put the hop back on `spine_q` and stop
    /// this pick from popping it again. The tx stays `ST_INDEP` so heal can
    /// still find it; `ST_CHAIN` is not a parking state.
    pub(crate) fn defer_ordered(&self, tx: TxIdx) {
        if tx >= self.block_size {
            return;
        }
        // Pause before dropping `ST_RUNNING`, or another worker pops the tail
        // in the window between release and push.
        self.spine_pause.store(true, Ordering::Release);
        self.release_running(tx);
        let st = self.state[tx].load(Ordering::Acquire);
        if st == ST_DONE || st == ST_RUNNING {
            self.spine_pause.store(false, Ordering::Release);
            return;
        }
        if st != ST_INDEP {
            let _ =
                self.state[tx].compare_exchange(st, ST_INDEP, Ordering::AcqRel, Ordering::Relaxed);
        }
        self.spine_q.push_local(tx);
        self.spine_pause.store(true, Ordering::Release);
    }

    fn clear_spine_claim(&self, tx: TxIdx) {
        if self
            .spine_owner_tx
            .compare_exchange(tx, usize::MAX, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            self.spine_holder.store(0, Ordering::Release);
        }
    }

    fn note_spine_claim(&self, worker_i: usize, tx: TxIdx) {
        self.spine_owner_tx.store(tx, Ordering::Release);
        let tag = worker_i.saturating_add(1);
        match self
            .spine_holder
            .compare_exchange(0, tag, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                self.spine_cores_max.fetch_max(1, Ordering::Relaxed);
            }
            Err(prev) if prev == tag => {}
            Err(_) => {
                self.spine_cores_max.fetch_max(2, Ordering::Relaxed);
            }
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

    /// Work-conserving pick. One ordered hop, then this worker's Admit
    /// deque, then a steal of another worker's front. Chain members are not
    /// in those deques. Gated / waiting txs are never returned.
    pub(crate) fn pick(&self, worker_i: usize, ready: &ReadyEdgeTable) -> Option<SfPick> {
        let worker = worker_i % self.cores;

        // Handoff slot, any free worker. The reader inside WaitTrueVersion
        // must not be the only core that can start the writer.
        if let Some(tx) = self.claim_spine() {
            if let Some(p) = self.admit_or_refuse(tx, QueueKind::Indep, ready) {
                if matches!(p, SfPick::Execute { tx: got, .. } if got == tx) {
                    self.note_spine_claim(worker, tx);
                    return Some(p);
                }
                self.clear_spine_claim(tx);
                return Some(p);
            }
            self.clear_spine_claim(tx);
        }
        if let Some(tx) = self.pop_spine_hop() {
            if let Some(p) = self.admit_or_refuse(tx, QueueKind::Indep, ready) {
                if matches!(p, SfPick::Execute { tx: got, .. } if got == tx) {
                    self.note_spine_claim(worker, tx);
                    return Some(p);
                }
                self.clear_spine_claim(tx);
                return Some(p);
            }
            self.clear_spine_claim(tx);
        }

        if let Some(tx) = self.pop_batch(worker) {
            self.local_hits.fetch_add(1, Ordering::Relaxed);
            return Some(self.exec_width(tx));
        }

        // One revalidate. Soft=0 aborts are rare; draining the queue here
        // used to hide the antichain behind validation meta.
        if let Some(tx) = self.pop_kind(QueueKind::Revalidate) {
            return Some(SfPick::Revalidate(tx));
        }

        if self.claim_width_batch(worker, worker, false, ready) > 0
            && let Some(tx) = self.pop_batch(worker)
        {
            self.local_hits.fetch_add(1, Ordering::Relaxed);
            return Some(self.exec_width(tx));
        }

        if self.cores > 1 {
            let start = self.steal_cursor.fetch_add(1, Ordering::Relaxed);
            for k in 1..self.cores {
                let victim = (start + k) % self.cores;
                if self.claim_width_batch(worker, victim, true, ready) > 0
                    && let Some(tx) = self.pop_batch(worker)
                {
                    self.local_hits.fetch_add(1, Ordering::Relaxed);
                    return Some(self.exec_width(tx));
                }
            }
        }

        // Global backup. A chain hop that landed here goes back to spine_q.
        if worker == 0 {
            while let Some(tx) = self.pop_kind(QueueKind::Indep) {
                if self.is_chain_member(tx) {
                    self.defer_ordered(tx);
                    continue;
                }
                if let Some(p) = self.admit_or_refuse(tx, QueueKind::Indep, ready) {
                    return Some(p);
                }
            }
        } else {
            while let Some(tx) = self.steal_kind(QueueKind::Indep) {
                if self.is_chain_member(tx) {
                    self.defer_ordered(tx);
                    continue;
                }
                if let Some(p) = self.admit_or_refuse(tx, QueueKind::Indep, ready) {
                    return Some(p);
                }
            }
        }

        for kind in [QueueKind::Released, QueueKind::Ordered] {
            while let Some(tx) = self.pop_kind(kind) {
                if self.is_chain_member(tx) {
                    self.defer_ordered(tx);
                    continue;
                }
                if let Some(p) = self.admit_or_refuse(tx, kind, ready) {
                    return Some(p);
                }
            }
            while let Some(tx) = self.steal_kind(kind) {
                if self.is_chain_member(tx) {
                    self.defer_ordered(tx);
                    continue;
                }
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

    fn exec_width(&self, tx: TxIdx) -> SfPick {
        SfPick::Execute {
            tx,
            vis: VisibilityPolicy::Opt,
            from: QueueKind::Indep,
            refused: false,
        }
    }

    /// True only while the owned hop is still inside execute. A owner left
    /// on a requeued tx must not freeze the chain.
    fn spine_busy(&self) -> bool {
        let cur = self.spine_owner_tx.load(Ordering::Acquire);
        cur < self.block_size && self.state[cur].load(Ordering::Acquire) == ST_RUNNING
    }

    fn release_spine_lock(&self) {
        let _ = self.spine_owner_tx.compare_exchange(
            SPINE_LOCK,
            usize::MAX,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
    }

    /// Exclusive right to pull one ordered hop. Stale owners are cleared.
    fn lock_spine(&self) -> bool {
        loop {
            if self.spine_busy() {
                return false;
            }
            let cur = self.spine_owner_tx.load(Ordering::Acquire);
            if cur == SPINE_LOCK {
                return false;
            }
            if cur == usize::MAX {
                return self
                    .spine_owner_tx
                    .compare_exchange(usize::MAX, SPINE_LOCK, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok();
            }
            if cur < self.block_size && self.state[cur].load(Ordering::Acquire) == ST_RUNNING {
                return false;
            }
            if self
                .spine_owner_tx
                .compare_exchange(cur, usize::MAX, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
        }
    }

    /// One hop from `spine_q`. Does not skip a live head to start the tail.
    fn pop_spine_hop(&self) -> Option<TxIdx> {
        if self.spine_pause.load(Ordering::Acquire)
            || !self.spine_slot_empty()
            || !self.lock_spine()
        {
            return None;
        }
        let mut guard = 0usize;
        while guard < self.block_size {
            guard += 1;
            let Some(tx) = self.spine_q.try_pop_local() else {
                break;
            };
            if tx >= self.block_size {
                continue;
            }
            let st = self.state[tx].load(Ordering::Acquire);
            if st == ST_DONE {
                continue;
            }
            if self.spine_owner_tx.load(Ordering::Acquire) == SPINE_LOCK
                && st == ST_INDEP
                && self.take_if(tx, ST_INDEP)
            {
                self.spine_owner_tx.store(tx, Ordering::Release);
                return Some(tx);
            }
            if st == ST_RUNNING || st == ST_SPINE || st == ST_INDEP {
                self.spine_q.push_local(tx);
            }
            break;
        }
        self.release_spine_lock();
        None
    }

    #[inline]
    pub(crate) fn spine_slot_empty(&self) -> bool {
        self.spine_slot.load(Ordering::Acquire) == usize::MAX
    }

    fn claim_spine(&self) -> Option<TxIdx> {
        if self.spine_busy() {
            return None;
        }
        let tx = self.spine_slot.swap(usize::MAX, Ordering::AcqRel);
        if tx == usize::MAX || tx >= self.block_size {
            return None;
        }
        let took = self.take_if(tx, ST_SPINE)
            || self.take_if(tx, ST_CHAIN)
            || self.take_if(tx, ST_INDEP)
            || self.take_if(tx, ST_WAIT);
        if !took {
            let st = self.state[tx].load(Ordering::Acquire);
            if st != ST_RUNNING && st != ST_DONE {
                let _ = self.spine_slot.compare_exchange(
                    usize::MAX,
                    tx,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
            }
            return None;
        }
        if self.own_spine(tx) {
            Some(tx)
        } else {
            // A hop is actually running. Put this one back; do not drop it.
            self.state[tx].store(ST_SPINE, Ordering::Release);
            let _ = self.spine_slot.compare_exchange(
                usize::MAX,
                tx,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
            None
        }
    }

    /// Install `tx` as the spine owner. Fails only when another hop is
    /// `ST_RUNNING`. A stale owner (requeued, waiting, done) is replaced.
    fn own_spine(&self, tx: TxIdx) -> bool {
        loop {
            let cur = self.spine_owner_tx.load(Ordering::Acquire);
            if cur == tx {
                return true;
            }
            if cur < self.block_size && self.state[cur].load(Ordering::Acquire) == ST_RUNNING {
                return false;
            }
            if self
                .spine_owner_tx
                .compare_exchange(cur, tx, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn pop_batch(&self, worker: usize) -> Option<TxIdx> {
        // Claim order, not LIFO: the first pop is the chain head when it was
        // at the owner's back. A LIFO batch would run K width txs before it.
        let tx = self.batches.get(worker)?.lock().pop_front();
        if tx.is_some() {
            self.dec_queued();
        }
        tx
    }

    /// Claim up to [`BATCH_K`] Admit-Unfenced txs. `steal` pops the victim's
    /// front; the owner pops its own back. Gated txs are not claimed.
    fn claim_width_batch(
        &self,
        worker: usize,
        owner: usize,
        steal: bool,
        ready: &ReadyEdgeTable,
    ) -> usize {
        let Some(q) = self.locals.get(owner) else {
            return 0;
        };
        let mut n = 0;
        while n < BATCH_K {
            let Some(tx) = (if steal { q.steal() } else { q.try_pop_local() }) else {
                break;
            };
            if tx >= self.block_size {
                continue;
            }
            self.dec_queued();
            if self.is_chain_member(tx) {
                if self.state[tx].load(Ordering::Acquire) == ST_INDEP {
                    self.spine_q.push_local(tx);
                }
                continue;
            }
            if self.refused_gate(tx, ready) {
                ready.note_skip_gate(tx);
                self.note_wait_unless_running(tx);
                self.refuse_gated.fetch_add(1, Ordering::Relaxed);
                self.refuse_fill_n.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if !self.take_if(tx, ST_INDEP) {
                continue;
            }
            self.batches[worker].lock().push_back(tx);
            self.local_queued.fetch_add(1, Ordering::Relaxed);
            if steal {
                self.steal_n.fetch_add(1, Ordering::Relaxed);
            }
            n += 1;
        }
        n
    }

    fn dec_queued(&self) {
        let mut cur = self.local_queued.load(Ordering::Relaxed);
        while cur > 0 {
            match self.local_queued.compare_exchange_weak(
                cur,
                cur - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(v) => cur = v,
            }
        }
    }

    fn refused_gate(&self, tx: TxIdx, ready: &ReadyEdgeTable) -> bool {
        !ready.leftover_min_on_skippable_gate(tx)
            && (ready.leftover_surplus(tx) || (ready.is_gated(tx) && !ready.may_execute(tx)))
    }

    fn admit_or_refuse(
        &self,
        tx: TxIdx,
        kind: QueueKind,
        ready: &ReadyEdgeTable,
    ) -> Option<SfPick> {
        if !ready.leftover_min_on_skippable_gate(tx)
            && (ready.leftover_surplus(tx) || (ready.is_gated(tx) && !ready.may_execute(tx)))
        {
            ready.note_skip_gate(tx);
            self.mark_wait(tx);
            self.refuse_gated.fetch_add(1, Ordering::Relaxed);
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
        if ready.is_gated(tx) && !ready.may_execute(tx) {
            self.gated_pick.fetch_add(1, Ordering::Relaxed);
            self.mark_wait(tx);
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

    /// `Aborting` parked on a still-unpublished writer must not incarnation++.
    /// Nudge that writer instead (19807137 root_inc mill).
    fn recover_aborting_unless_parked(
        &self,
        tx: TxIdx,
        ready: &ReadyEdgeTable,
        scheduler: &Scheduler,
    ) -> bool {
        if let Some(b) = scheduler.live_block(tx) {
            let idle = b < self.block_size && self.state[b].load(Ordering::Acquire) != ST_RUNNING;
            // Claim already moved past this writer and its worker is gone.
            // Drop the park and recover the waiter once. Execute will not
            // `add_dependency` on `leftover_passed`, so this is not a mill.
            // Do not `recover_executing` the writer (335/619 SEGV). The
            // waiter must also have left: recovering `ST_RUNNING` lets
            // `force_push` start a second execute on the same slot.
            let waiter_idle = self.state[tx].load(Ordering::Acquire) != ST_RUNNING;
            if idle && waiter_idle && ready.leftover_passed(b) {
                scheduler.clear_stale_block(tx, b);
            } else {
                if idle && (scheduler.is_ready(b) || scheduler.is_executed(b)) {
                    self.requeue_ready(b, ready);
                }
                return false;
            }
        }
        scheduler.recover_aborting(tx)
    }

    fn requeue_ready(&self, tx: TxIdx, ready: &ReadyEdgeTable) {
        // Gated !may_execute on a queue is the 19807137 refuse mill:
        // pick mark_waits, heal force_pushes, pending stays ~60, idle
        // ungate never runs.
        let st = self.state[tx].load(Ordering::Acquire);
        if st == ST_RUNNING {
            return;
        }
        if ready.leftover_min_on_skippable_gate(tx) {
            let _ = self.wake_idle(tx, QueueKind::Indep);
            return;
        }
        if ready.leftover_surplus(tx) || (ready.is_gated(tx) && !ready.may_execute(tx)) {
            self.note_wait_unless_running(tx);
            return;
        }
        // Never Q_ordered from heal: 19807137 mills ~40 OrderedTip heads
        // even when only location-admitted txs are pushed (8148ded).
        // Released/Indep + plant waits; FullReplay stays off Ordered.
        if ready.is_gated(tx) {
            let _ = self.wake_idle(tx, QueueKind::Released);
        } else {
            let _ = self.wake_idle(tx, QueueKind::Indep);
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
        let leftover_next = ready.take_finished_leftover_min(|t| scheduler.is_validated(t));
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
                let _ = self.recover_aborting_unless_parked(tx, ready, scheduler);
            } else if scheduler.is_executing(tx) {
                let _ = scheduler.recover_executing_waiter(tx);
            }
            if scheduler.is_executed(tx) {
                if self.wake_idle(tx, QueueKind::Revalidate) {
                    n += 1;
                }
            } else if scheduler.is_ready(tx) {
                self.requeue_ready(tx, ready);
                n += 1;
            }
        }
        for tx in freed {
            if scheduler.is_aborting(tx) {
                let _ = self.recover_aborting_unless_parked(tx, ready, scheduler);
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
            if scheduler.is_aborting(tx)
                && self.recover_aborting_unless_parked(tx, ready, scheduler)
            {
                self.requeue_ready(tx, ready);
                n += 1;
                continue;
            }
            if scheduler.is_executed(tx) {
                if self.wake_idle(tx, QueueKind::Revalidate) {
                    n += 1;
                }
                continue;
            }
            if scheduler.is_ready(tx)
                && (ready.may_execute(tx)
                    || ready.has_known_waiters(tx)
                    || ready.leftover_min_on_skippable_gate(tx))
            {
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
                let _ = self.recover_aborting_unless_parked(tx, ready, scheduler);
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
                if scheduler.is_executed(tx) && self.wake_idle(tx, QueueKind::Revalidate) {
                    n += 1;
                } else if scheduler.is_ready(tx) {
                    self.requeue_ready(tx, ready);
                    n += 1;
                }
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
                    if self.recover_aborting_unless_parked(tx, ready, scheduler) {
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
                        if self.recover_aborting_unless_parked(tx, ready, scheduler) {
                            self.requeue_ready(tx, ready);
                            n += 1;
                        }
                        continue;
                    }
                    // True idle only. Detect naming another Aborting leftover
                    // (8-core 19469101 pending=0) still recovers here.
                    _ => {
                        if self.pending_work() == 0
                            && self.recover_aborting_unless_parked(tx, ready, scheduler)
                        {
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
                    if self.state[tx]
                        .compare_exchange(
                            ST_REVALIDATE,
                            ST_NONE,
                            Ordering::AcqRel,
                            Ordering::Relaxed,
                        )
                        .is_ok()
                        && self.wake_idle(tx, QueueKind::Revalidate)
                    {
                        n += 1;
                    }
                } else if scheduler.is_ready(tx) {
                    self.requeue_ready(tx, ready);
                    n += 1;
                }
                continue;
            }
            if scheduler.is_executed(tx) {
                if self.wake_idle(tx, QueueKind::Revalidate) {
                    n += 1;
                }
                continue;
            }
            if !scheduler.is_ready(tx) {
                continue;
            }
            if st == ST_WAIT && !ready.may_execute(tx) {
                if ready.leftover_min_on_skippable_gate(tx) {
                    self.requeue_ready(tx, ready);
                    n += 1;
                    continue;
                }
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
            if st == ST_NONE || st == ST_WAIT {
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
            if let Some(next) = ready.take_finished_leftover_min(|t| scheduler.is_validated(t)) {
                if !scheduler.is_validated(next) {
                    if scheduler.is_aborting(next) {
                        let _ = self.recover_aborting_unless_parked(next, ready, scheduler);
                    } else if scheduler.is_executing(next) {
                        let _ = scheduler.recover_executing_waiter(next);
                    }
                    if scheduler.is_executed(next) {
                        if self.wake_idle(next, QueueKind::Revalidate) {
                            n += 1;
                        }
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
                    if !live_owner && self.recover_aborting_unless_parked(tx, ready, scheduler) {
                        self.requeue_ready(tx, ready);
                        n += 1;
                    }
                } else if scheduler.is_executed(tx) {
                    if self.wake_idle(tx, QueueKind::Revalidate) {
                        n += 1;
                    }
                } else if scheduler.is_ready(tx)
                    && (ready.may_execute(tx) || ready.leftover_min_on_skippable_gate(tx))
                {
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
                        let _ = self.recover_aborting_unless_parked(tx, ready, scheduler);
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
        let spine = usize::from(self.spine_slot.load(Ordering::Relaxed) != usize::MAX);
        self.q_indep.len()
            + self.q_released.len()
            + self.q_ordered.len()
            + self.spine_q.len()
            + self.local_queued.load(Ordering::Relaxed)
            + spine
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
    pub(crate) fn refuse_gated(&self) -> usize {
        self.refuse_gated.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn gated_pick(&self) -> usize {
        self.gated_pick.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn spine_cores(&self) -> usize {
        self.spine_cores_max.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn local_hits(&self) -> usize {
        self.local_hits.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn help_release_n(&self) -> usize {
        self.help_release_n.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn note_help_release(&self) {
        self.help_release_n.fetch_add(1, Ordering::Relaxed);
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

    /// Hang-trace: runnable state byte (`ST_*`).
    #[inline]
    pub(crate) fn hang_state(&self, tx: TxIdx) -> u8 {
        if tx >= self.block_size {
            return 0xff;
        }
        self.state[tx].load(Ordering::Acquire)
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
        r.seed_begin(&ready, &stages, &sched, None);
        assert!(r.q_indep_len() >= 4, "ungated antichain in Q_indep");
        assert_eq!(r.q_ordered_len(), 0, "no ordered tip until a window head");
        // Gated 3 is waiting, not on a Q.
        let first = r.pick(0, &ready).expect("indep work");
        let SfPick::Execute { tx, vis, .. } = first else {
            panic!("expected execute");
        };
        assert_ne!(tx, 3, "refused consumer must not occupy a core");
        assert_eq!(vis, VisibilityPolicy::Opt);
        assert_eq!(tx, 0, "longest remaining suffix is the lowest index");
    }

    #[test]
    fn crit_head_is_popped_before_lower_indices() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let sched = Scheduler::new(8);
        ready.plant_nearest_preds(0xabc, &[4, 6]);
        let r = RunnableSet::new(8, 2);
        r.seed_begin(&ready, &stages, &sched, Some(4));
        let first = r.pick(0, &ready).expect("head");
        let SfPick::Execute { tx, .. } = first else {
            panic!("expected execute");
        };
        assert_eq!(tx, 4, "learned chain head starts before index 0");
        assert!(!ready.is_gated(6), "successor stays on the Opt path");
        assert_eq!(ready.blocking_producer(6), Some(4));
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
    fn shard_spine_is_owner_only_and_width_steals() {
        let ready = ReadyEdgeTable::new();
        let r = RunnableSet::new(8, 4);
        for t in 0..8 {
            r.push(t, QueueKind::Indep);
        }
        r.shard_width(&[1, 4, 6]);
        let SfPick::Execute { tx, .. } = r.pick(0, &ready).expect("chain head") else {
            panic!("expected execute");
        };
        assert_eq!(tx, 1, "worker 0 LIFO starts at the chain head");
        let mut width = Vec::new();
        while let Some(p) = r.pick(1, &ready) {
            match p {
                SfPick::Execute { tx, .. } => {
                    assert!(
                        tx != 1 && tx != 4 && tx != 6,
                        "width core stole a chain hop {tx}"
                    );
                    width.push(tx);
                    r.mark_done(tx);
                }
                SfPick::Revalidate(tx) => r.mark_done(tx),
            }
        }
        assert!(
            !width.is_empty(),
            "worker 1 must take Admit work from a local or a steal"
        );
        assert_eq!(r.gated_pick(), 0);
        assert!(r.spine_cores() <= 1);
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
