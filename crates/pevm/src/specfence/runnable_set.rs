//! RunnableSet — AdmitShard on the three-primitive spine.
//!
//! `AdmitIndep` is seeded across per-worker Chase-Lev deques. The owner pops
//! LIFO at the bottom; a thief pops FIFO at the top only when its own deque
//! is empty. There is no single-owner mutex. Gated / waiting heads are not
//! work. Spine hops stay on [`super::access_spine::AccessSpine`]'s handoff
//! slot and are never seeded as `AdmitIndep`. Pick never calls
//! `Scheduler::next_task*`.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::thread::Thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::admit_deque::LocalAdmitDeque;

/// Product knife. Default on. `SPECFENCE_IDEAL_TIMED_ADMIT=0` restores
/// index-band seed and does not park later waves on a second deque.
fn ideal_timed_admit_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("SPECFENCE_IDEAL_TIMED_ADMIT").ok().as_deref(),
            Some("0") | Some("false") | Some("FALSE")
        )
    })
}

fn work_hint(work: &[u64], tx: TxIdx) -> u64 {
    work.get(tx).copied().filter(|&gas| gas > 0).unwrap_or(1)
}

fn hints_vary(txs: &[TxIdx], work: &[u64]) -> bool {
    if work.is_empty() || txs.is_empty() {
        return false;
    }
    let first = work_hint(work, txs[0]);
    txs.iter().any(|&tx| work_hint(work, tx) != first)
}

thread_local! {
    static ADMIT_OWNER: Cell<Option<usize>> = const { Cell::new(None) };
}

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

/// One-shot counts taken when the prior-chain tail records `first_start`.
pub(crate) struct SpanEndSnap {
    pub not_started: usize,
    pub unfinished: usize,
    pub owed: usize,
    pub running: usize,
    pub pending: usize,
    pub indep: usize,
    pub false_bits: u64,
}

/// Post-span join probe. Sums are worker-time. Origins are `exec_origin`.
#[derive(Clone, Copy)]
pub(crate) struct PostSpanProbe {
    pub span_end_origin_ns: u64,
    pub last_exec_origin_ns: u64,
    pub last_validate_origin_ns: u64,
    pub quiet_true_origin_ns: u64,
    pub last_exit_origin_ns: u64,
    pub post_span_exec_ns: u64,
    pub post_span_validate_ns: u64,
    pub post_span_heal_ns: u64,
    pub post_span_yield_ns: u64,
    pub post_span_park_ns: u64,
    pub post_span_steal_ns: u64,
    pub post_span_exec_n: usize,
    pub post_span_idle_n: usize,
    pub span_end_not_started: usize,
    pub span_end_unfinished: usize,
    pub span_end_owed: usize,
    pub span_end_running: usize,
    pub span_end_pending: usize,
    pub span_end_indep: usize,
    pub span_end_false_bits: u64,
    pub quiet_false_or: u64,
}

/// Detect-driven runnable set: antichain ∪ released ∪ ordered tips.
#[derive(Debug)]
pub(crate) struct RunnableSet {
    /// Per-worker `AdmitIndep`. Index = worker. Owner LIFO / thief FIFO.
    /// Ideal-ready (no Detect pred) lives here.
    local_admit: Vec<LocalAdmitDeque>,
    /// Later-wave AdmitIndep (a Detect pred exists). Popped only after
    /// `local_admit` when Ideal-timed admit is on, so a publish wake cannot
    /// jump the antichain. Spine hops never land here.
    local_late: Vec<LocalAdmitDeque>,
    /// Seeded with no Detect predecessor.
    ideal_ready: Vec<AtomicBool>,
    q_released: Deque,
    q_ordered: Deque,
    q_revalidate: Deque,
    state: Vec<AtomicU8>,
    block_size: usize,
    cores: usize,
    rr: AtomicUsize,
    steal_n: AtomicUsize,
    /// Owner `pop_bottom` of `AdmitIndep` (not a steal).
    seed_owner_local_pops: AtomicUsize,
    /// Sum across workers of time spent in thief `pop_top`. Not a wall clock.
    steal_top_ns: AtomicU64,
    /// `exact_wake_one` found no parked core.
    exact_wake_missed_nopark: AtomicUsize,
    refuse_fill_n: AtomicUsize,
    idle_spins: AtomicUsize,
    /// Idle-ring sum (includes [`Self::heal_ns`]). Not a wall clock.
    idle_ns: AtomicU64,
    /// Heal subset of [`Self::idle_ns`].
    heal_ns: AtomicU64,
    /// Post-exec validate/resolve/drain outside the prior-crit span.
    post_exec_validate_ns: AtomicU64,
    /// Post-exec validate/resolve/drain that started inside the open span.
    post_exec_in_span_ns: AtomicU64,
    /// Prior crit head for the span filter. `usize::MAX` if unset.
    span_head: AtomicUsize,
    /// Prior crit tail for the span filter. `usize::MAX` if unset.
    span_tail: AtomicUsize,
    /// `exec_origin` at the prior-chain tail's `first_start`. 0 if no tail.
    span_end_origin_ns: AtomicU64,
    /// Latest `exec_origin` at which an execution attempt returned.
    last_exec_origin_ns: AtomicU64,
    /// Latest `exec_origin` at which validate/resolve returned.
    last_validate_origin_ns: AtomicU64,
    /// First `exec_origin` at which a worker observed full BlockQuiet.
    quiet_true_origin_ns: AtomicU64,
    /// Latest `exec_origin` at which a worker left the loop.
    last_exit_origin_ns: AtomicU64,
    /// Sum of execute() time at or after [`Self::span_end_origin_ns`].
    post_span_exec_ns: AtomicU64,
    /// Sum of validate/resolve time at or after the span end.
    post_span_validate_ns: AtomicU64,
    /// Sum of idle-arm heal time at or after the span end.
    post_span_heal_ns: AtomicU64,
    /// Sum of `yield_now` time at or after the span end.
    post_span_yield_ns: AtomicU64,
    /// Sum of `park_idle` time at or after the span end.
    post_span_park_ns: AtomicU64,
    /// Sum of `pick → None` time at or after the span end.
    post_span_steal_ns: AtomicU64,
    /// Execution attempts whose start is at or after the span end.
    post_span_exec_n: AtomicUsize,
    /// Idle-arm entries at or after the span end.
    post_span_idle_n: AtomicUsize,
    /// Txs with `first_start` still 0 when the tail started.
    span_end_not_started: AtomicUsize,
    /// Txs not yet Executed/Validated when the tail started.
    span_end_unfinished: AtomicUsize,
    /// Executed-but-not-validated txs when the tail started.
    span_end_owed: AtomicUsize,
    /// `ST_RUNNING` count when the tail started.
    span_end_running: AtomicUsize,
    /// `pending_work` when the tail started.
    span_end_pending: AtomicUsize,
    /// AdmitShard occupancy when the tail started.
    span_end_indep: AtomicUsize,
    /// BlockQuiet clauses that were false when the tail started.
    span_end_false_bits: AtomicU64,
    /// OR of BlockQuiet clauses false on idle samples after the span end.
    quiet_false_or: AtomicU64,
    width_sum: AtomicUsize,
    width_n: AtomicUsize,
    threads: Vec<Mutex<Option<Thread>>>,
    parked: Vec<AtomicBool>,
    wake_rr: AtomicUsize,
    exact_wakes: AtomicUsize,
    parks: AtomicUsize,
    help_n: AtomicUsize,
}

impl RunnableSet {
    /// Admit shards, released, ordered, or revalidate still hold a tx.
    pub(crate) const QF_PENDING: u64 = 1 << 0;
    /// Wave bag still holds a wake.
    pub(crate) const QF_WAVE: u64 = 1 << 1;
    /// Spine handoff slot is occupied.
    pub(crate) const QF_HANDOFF: u64 = 1 << 2;
    /// Some `ST_WAIT` is on a producer that is executing.
    pub(crate) const QF_WAITING_LIVE: u64 = 1 << 3;
    /// Some tx is `ST_RUNNING`.
    pub(crate) const QF_RUNNING: u64 = 1 << 4;
    /// Chain hop still owns `spine_owner`.
    pub(crate) const QF_SPINE: u64 = 1 << 5;
    /// A gated tx has not been marked done.
    pub(crate) const QF_GATED: u64 = 1 << 6;
    /// A sleeper is waiting for a true tip.
    pub(crate) const QF_SLEEPING: u64 = 1 << 7;
    /// Some tx is not yet Executed or Validated.
    pub(crate) const QF_UNFINISHED: u64 = 1 << 8;
    /// Some tx is Executed and still needs validate.
    pub(crate) const QF_OWED: u64 = 1 << 9;

    pub(crate) fn new(block_size: usize, cores: usize) -> Self {
        let cores = cores.max(1);
        Self {
            local_admit: (0..cores)
                .map(|_| LocalAdmitDeque::with_capacity(block_size))
                .collect(),
            local_late: (0..cores)
                .map(|_| LocalAdmitDeque::with_capacity(block_size))
                .collect(),
            ideal_ready: (0..block_size).map(|_| AtomicBool::new(false)).collect(),
            q_released: Deque::new(),
            q_ordered: Deque::new(),
            q_revalidate: Deque::new(),
            state: (0..block_size).map(|_| AtomicU8::new(ST_NONE)).collect(),
            block_size,
            cores,
            rr: AtomicUsize::new(0),
            steal_n: AtomicUsize::new(0),
            seed_owner_local_pops: AtomicUsize::new(0),
            steal_top_ns: AtomicU64::new(0),
            exact_wake_missed_nopark: AtomicUsize::new(0),
            refuse_fill_n: AtomicUsize::new(0),
            idle_spins: AtomicUsize::new(0),
            idle_ns: AtomicU64::new(0),
            heal_ns: AtomicU64::new(0),
            post_exec_validate_ns: AtomicU64::new(0),
            post_exec_in_span_ns: AtomicU64::new(0),
            span_head: AtomicUsize::new(usize::MAX),
            span_tail: AtomicUsize::new(usize::MAX),
            span_end_origin_ns: AtomicU64::new(0),
            last_exec_origin_ns: AtomicU64::new(0),
            last_validate_origin_ns: AtomicU64::new(0),
            quiet_true_origin_ns: AtomicU64::new(0),
            last_exit_origin_ns: AtomicU64::new(0),
            post_span_exec_ns: AtomicU64::new(0),
            post_span_validate_ns: AtomicU64::new(0),
            post_span_heal_ns: AtomicU64::new(0),
            post_span_yield_ns: AtomicU64::new(0),
            post_span_park_ns: AtomicU64::new(0),
            post_span_steal_ns: AtomicU64::new(0),
            post_span_exec_n: AtomicUsize::new(0),
            post_span_idle_n: AtomicUsize::new(0),
            span_end_not_started: AtomicUsize::new(0),
            span_end_unfinished: AtomicUsize::new(0),
            span_end_owed: AtomicUsize::new(0),
            span_end_running: AtomicUsize::new(0),
            span_end_pending: AtomicUsize::new(0),
            span_end_indep: AtomicUsize::new(0),
            span_end_false_bits: AtomicU64::new(0),
            quiet_false_or: AtomicU64::new(0),
            width_sum: AtomicUsize::new(0),
            width_n: AtomicUsize::new(0),
            threads: (0..cores).map(|_| Mutex::new(None)).collect(),
            parked: (0..cores).map(|_| AtomicBool::new(false)).collect(),
            wake_rr: AtomicUsize::new(0),
            exact_wakes: AtomicUsize::new(0),
            parks: AtomicUsize::new(0),
            help_n: AtomicUsize::new(0),
        }
    }

    /// Worker-local `AdmitIndep` pushes land on this shard.
    pub(crate) fn bind_worker(&self, worker_i: usize) {
        let i = worker_i % self.cores;
        *self.threads[i].lock() = Some(std::thread::current());
        ADMIT_OWNER.with(|c| c.set(Some(i)));
    }

    #[inline]
    fn indep_shard(&self) -> usize {
        ADMIT_OWNER.with(|c| {
            c.get()
                .unwrap_or_else(|| self.rr.fetch_add(1, Ordering::Relaxed) % self.cores)
        })
    }

    #[inline]
    fn q(&self, kind: QueueKind) -> &Deque {
        match kind {
            QueueKind::Indep => unreachable!("AdmitIndep is a Chase-Lev shard"),
            QueueKind::Released => &self.q_released,
            QueueKind::Ordered => &self.q_ordered,
            QueueKind::Revalidate => &self.q_revalidate,
        }
    }

    #[inline]
    fn enqueue(&self, tx: TxIdx, kind: QueueKind) {
        if kind == QueueKind::Indep {
            let shard = self.indep_shard();
            let late = ideal_timed_admit_on()
                && tx < self.ideal_ready.len()
                && !self.ideal_ready[tx].load(Ordering::Relaxed);
            let q = if late {
                &self.local_late
            } else {
                &self.local_admit
            };
            q[shard].push_bottom(tx);
        } else {
            self.q(kind).push_local(tx);
        }
        // One sleeper, not the herd. No-op when nobody is parked.
        self.exact_wake_one();
    }

    /// Seed-time `AdmitIndep` push. Workers are not running, so this does not
    /// call `ExactWake` (there is nobody to unpark).
    ///
    /// `late` parks a tx that already has a Detect predecessor on
    /// [`Self::local_late`]. Ideal-ready txs (`late == false`) set the bit
    /// and land on [`Self::local_admit`].
    fn place_seed_on(&self, shard: usize, tx: TxIdx, late: bool) {
        if tx >= self.block_size {
            return;
        }
        let prev = self.state[tx].swap(ST_INDEP, Ordering::AcqRel);
        if prev == ST_RUNNING || prev == ST_DONE {
            self.state[tx].store(prev, Ordering::Release);
            return;
        }
        if prev == ST_INDEP {
            return;
        }
        if !late {
            self.ideal_ready[tx].store(true, Ordering::Relaxed);
        }
        let q = if late {
            &self.local_late
        } else {
            &self.local_admit
        };
        q[shard % self.cores].push_bottom(tx);
    }

    /// `pop_low_first`: push high-to-low so LIFO yields the lowest index.
    fn push_index_order(&self, shard: usize, txs: &[TxIdx], late: bool, pop_low_first: bool) {
        if pop_low_first {
            for &tx in txs.iter().rev() {
                self.place_seed_on(shard, tx, late);
            }
        } else {
            for &tx in txs {
                self.place_seed_on(shard, tx, late);
            }
        }
    }

    /// Split one index band. Ideal-ready keeps the historical direction.
    /// A recorded Detect pred goes to the late deque.
    fn push_band_ordered(
        &self,
        shard: usize,
        band: &[TxIdx],
        ready: &ReadyEdgeTable,
        low_first: bool,
    ) {
        let mut ideal = Vec::new();
        let mut late = Vec::new();
        for &tx in band {
            if ideal_timed_admit_on() && ready.recorded_pred(tx).is_some() {
                late.push(tx);
            } else {
                ideal.push(tx);
            }
        }
        self.push_index_order(shard, &late, true, low_first);
        self.push_index_order(shard, &ideal, false, low_first);
    }

    /// Longest-processing-time onto shards `shard0..`. Heavier gas is popped
    /// first. Equal gas breaks ties toward the higher index.
    fn assign_lpt(&self, shard0: usize, txs: &[TxIdx], work: &[u64], late: bool) {
        if txs.is_empty() {
            return;
        }
        let n_free = self.cores.saturating_sub(shard0);
        if n_free == 0 {
            return;
        }
        let mut order = txs.to_vec();
        order.sort_by(|&a, &b| work_hint(work, b).cmp(&work_hint(work, a)).then(b.cmp(&a)));
        let mut load = vec![0u64; n_free];
        let mut buckets: Vec<Vec<TxIdx>> = vec![Vec::new(); n_free];
        for tx in order {
            let mut best = 0usize;
            for i in 1..n_free {
                if load[i] < load[best] {
                    best = i;
                }
            }
            load[best] = load[best].saturating_add(work_hint(work, tx));
            buckets[best].push(tx);
        }
        for (i, mut bucket) in buckets.into_iter().enumerate() {
            bucket.sort_by(|&a, &b| work_hint(work, a).cmp(&work_hint(work, b)).then(a.cmp(&b)));
            for tx in bucket {
                self.place_seed_on(shard0 + i, tx, late);
            }
        }
    }

    /// Contiguous count-balanced bands of `AdmitIndep`, already sorted ascending.
    ///
    /// Band 0 is pushed high-to-low so worker 0's LIFO pops the low prefix
    /// and a thief takes that band's high end. Later bands are pushed
    /// low-to-high so those owners start at their high end, not in the
    /// prefix. Round-robin (`tx % C`) put every core in the prefix and
    /// committed a wrong receipt on 15274915 at 2 cores.
    ///
    /// When Ideal-timed admit is on and gas limits differ, band 0 stays that
    /// prefix. The other bands' Ideal-ready txs are reassigned by gas onto
    /// the free cores (longest first). Txs with a Detect pred wait on
    /// `local_late` until the Ideal-ready deque is empty.
    fn shard_indep(&self, indep_asc: &[TxIdx], ready: &ReadyEdgeTable, work: &[u64]) {
        let cores = self.cores;
        let n = indep_asc.len();
        if n == 0 || cores == 0 {
            return;
        }
        let base = n / cores;
        let rem = n % cores;
        let mut cursor = 0;
        let mut bands: Vec<&[TxIdx]> = Vec::with_capacity(cores);
        for shard in 0..cores {
            let len = base + usize::from(shard < rem);
            bands.push(&indep_asc[cursor..cursor + len]);
            cursor += len;
        }
        let lpt_free = ideal_timed_admit_on() && cores > 1 && hints_vary(indep_asc, work);
        if lpt_free {
            self.push_band_ordered(0, bands[0], ready, true);
            let mut free_ideal = Vec::new();
            let mut free_late = Vec::new();
            for band in bands.iter().skip(1) {
                for &tx in *band {
                    if ready.recorded_pred(tx).is_some() {
                        free_late.push(tx);
                    } else {
                        free_ideal.push(tx);
                    }
                }
            }
            self.assign_lpt(1, &free_ideal, work, false);
            self.assign_lpt(1, &free_late, work, true);
        } else {
            for (shard, band) in bands.iter().enumerate() {
                self.push_band_ordered(shard, band, ready, shard == 0);
            }
        }
    }

    /// Seed after Detect `admit_seed`. Ungated `AdmitIndep` is sharded onto
    /// per-core deques. Window heads and released consumers stay on their
    /// own queues. Spine successors stay off-queue (`mark_wait`).
    pub(crate) fn seed_begin(
        &self,
        ready: &ReadyEdgeTable,
        stages: &ProducerStageTable,
        scheduler: &Scheduler,
        crit_head: Option<TxIdx>,
        work_hint: &[u64],
    ) {
        let head = crit_head.filter(|&tx| tx < self.block_size);
        let mut indep = Vec::new();
        // High index first matches the previous Released LIFO (low index
        // popped first). AdmitIndep is collected and sharded afterwards.
        for tx in (0..self.block_size).rev() {
            if Some(tx) == head {
                continue;
            }
            self.seed_one(tx, ready, stages, scheduler, &mut indep);
        }
        indep.reverse();
        self.shard_indep(&indep, ready, work_hint);
        if let Some(tx) = head {
            let mut ignored = Vec::new();
            self.seed_one(tx, ready, stages, scheduler, &mut ignored);
            // Chain head last on worker 0 so it is the first LIFO pop,
            // ahead of the low prefix. A head that already has a Detect
            // pred does not jump the Ideal-ready deque.
            for tx in ignored {
                let late = ideal_timed_admit_on() && ready.recorded_pred(tx).is_some();
                self.place_seed_on(0, tx, late);
            }
        }
    }

    fn seed_one(
        &self,
        tx: TxIdx,
        ready: &ReadyEdgeTable,
        stages: &ProducerStageTable,
        scheduler: &Scheduler,
        indep: &mut Vec<TxIdx>,
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
        // ungated so the resume stays on the Opt path. Not stealable Indep.
        if let Some(pred) = ready.blocking_producer(tx)
            && !ready.is_writer_done(pred)
        {
            self.mark_wait(tx);
            return;
        }
        indep.push(tx);
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
        self.enqueue(tx, kind);
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
        self.enqueue(tx, kind);
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
        if prev == tag {
            return;
        }
        self.enqueue(tx, kind);
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
        self.enqueue(tx, kind);
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
        if kind == QueueKind::Indep {
            return None;
        }
        let tag = kind.tag();
        let q = self.q(kind);
        while let Some(tx) = q.try_pop_local() {
            if self.take_if(tx, tag) {
                return Some(tx);
            }
        }
        None
    }

    fn steal_revalidate(&self) -> Option<TxIdx> {
        while let Some(tx) = self.q_revalidate.steal() {
            if self.take_if(tx, ST_REVALIDATE) {
                self.steal_n.fetch_add(1, Ordering::Relaxed);
                return Some(tx);
            }
        }
        None
    }

    /// Take the spine slot when no hop owns a core. The tx is not pushed
    /// onto any AdmitIndep deque.
    fn claim_handoff(
        &self,
        ready: &ReadyEdgeTable,
        spine: &super::access_spine::AccessSpine,
    ) -> Option<SfPick> {
        if spine.spine_busy() {
            return None;
        }
        let tx = spine.take_handoff()?;
        if tx >= self.block_size {
            return None;
        }
        let st = self.state[tx].load(Ordering::Acquire);
        if st == ST_DONE {
            return None;
        }
        if st == ST_RUNNING {
            spine.restore_handoff(tx);
            return None;
        }
        if !self.claim_running(tx) {
            spine.restore_handoff(tx);
            return None;
        }
        if self.refused_gate(tx, ready) {
            self.mark_wait(tx);
            spine.restore_handoff(tx);
            return None;
        }
        Some(SfPick::Execute {
            tx,
            vis: self.visibility(ready, tx),
            from: QueueKind::Ordered,
            refused: false,
            slot: true,
        })
    }

    /// CAS into `ST_RUNNING` from a parked or queued state. Not from a live owner.
    fn claim_running(&self, tx: TxIdx) -> bool {
        for expect in [ST_WAIT, ST_NONE, ST_INDEP, ST_RELEASED, ST_ORDERED] {
            if self.take_if(tx, expect) {
                return true;
            }
        }
        false
    }

    fn refused_gate(&self, tx: TxIdx, ready: &ReadyEdgeTable) -> bool {
        !ready.leftover_min_on_skippable_gate(tx)
            && (ready.leftover_surplus(tx) || (ready.is_gated(tx) && !ready.may_execute(tx)))
    }

    fn note_refuse(&self, tx: TxIdx, ready: &ReadyEdgeTable) {
        ready.note_skip_gate(tx);
        self.mark_wait(tx);
        self.refuse_fill_n.fetch_add(1, Ordering::Relaxed);
    }

    fn execute_indep(&self, tx: TxIdx, refused: bool) -> SfPick {
        SfPick::Execute {
            tx,
            vis: VisibilityPolicy::Opt,
            from: QueueKind::Indep,
            refused,
            slot: false,
        }
    }

    /// Owner LIFO. Ideal-ready first, then the late deque. A gated head is
    /// not scanned further — caller steals.
    fn pop_local_admit(&self, worker_i: usize, ready: &ReadyEdgeTable) -> LocalAdmit {
        let primary = self.pop_deque(worker_i, &self.local_admit, ready);
        if matches!(primary, LocalAdmit::Empty) && ideal_timed_admit_on() {
            self.pop_deque(worker_i, &self.local_late, ready)
        } else {
            primary
        }
    }

    fn pop_deque(
        &self,
        worker_i: usize,
        deques: &[LocalAdmitDeque],
        ready: &ReadyEdgeTable,
    ) -> LocalAdmit {
        let shard = worker_i % self.cores;
        while let Some(tx) = deques[shard].pop_bottom() {
            if !self.take_if(tx, ST_INDEP) {
                continue;
            }
            self.seed_owner_local_pops.fetch_add(1, Ordering::Relaxed);
            if self.refused_gate(tx, ready) {
                self.note_refuse(tx, ready);
                return LocalAdmit::Refused;
            }
            return LocalAdmit::Ready(tx);
        }
        LocalAdmit::Empty
    }

    /// FIFO `pop_top` of another worker's `AdmitIndep` deque only.
    /// Ideal-ready deques first. Spine hops are not in either deque.
    fn steal_admit(&self, worker_i: usize, ready: &ReadyEdgeTable) -> Option<TxIdx> {
        if let Some(tx) = self.steal_deques(worker_i, &self.local_admit, ready) {
            return Some(tx);
        }
        if ideal_timed_admit_on() {
            self.steal_deques(worker_i, &self.local_late, ready)
        } else {
            None
        }
    }

    fn steal_deques(
        &self,
        worker_i: usize,
        deques: &[LocalAdmitDeque],
        ready: &ReadyEdgeTable,
    ) -> Option<TxIdx> {
        let n = self.cores;
        for k in 1..n {
            let victim = (worker_i + k) % n;
            let mut gated_budget = 0u8;
            loop {
                let t0 = Instant::now();
                let stolen = deques[victim].pop_top();
                self.steal_top_ns
                    .fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
                let Some(tx) = stolen else {
                    break;
                };
                if !self.take_if(tx, ST_INDEP) {
                    continue;
                }
                self.steal_n.fetch_add(1, Ordering::Relaxed);
                if self.refused_gate(tx, ready) {
                    self.note_refuse(tx, ready);
                    gated_budget += 1;
                    if gated_budget >= 4 {
                        break;
                    }
                    continue;
                }
                return Some(tx);
            }
        }
        None
    }

    /// HelpRelease: one runnable Released/Ordered after AdmitIndep is empty.
    /// Gated heads are skipped, not returned.
    fn help_release(&self, ready: &ReadyEdgeTable) -> Option<SfPick> {
        for kind in [QueueKind::Released, QueueKind::Ordered] {
            for _ in 0..8 {
                let Some(tx) = self.pop_kind(kind) else {
                    break;
                };
                if self.refused_gate(tx, ready) {
                    self.note_refuse(tx, ready);
                    continue;
                }
                self.help_n.fetch_add(1, Ordering::Relaxed);
                let vis = if kind == QueueKind::Ordered {
                    VisibilityPolicy::OrderedTip
                } else {
                    VisibilityPolicy::for_ready(ready, tx)
                };
                return Some(SfPick::Execute {
                    tx,
                    vis,
                    from: kind,
                    refused: false,
                    slot: false,
                });
            }
        }
        None
    }

    /// IdleStealWake pick. `spine` is `None` in unit tests (no handoff slot).
    pub(crate) fn pick(&self, worker_i: usize, ready: &ReadyEdgeTable) -> Option<SfPick> {
        self.pick_in(worker_i, ready, None)
    }

    pub(crate) fn pick_in(
        &self,
        worker_i: usize,
        ready: &ReadyEdgeTable,
        spine: Option<&super::access_spine::AccessSpine>,
    ) -> Option<SfPick> {
        if let Some(tx) = self.pop_kind(QueueKind::Revalidate) {
            return Some(SfPick::Revalidate(tx));
        }
        // Handoff before local Indep so the single spine hop is not stuck
        // behind a wide antichain. Only one core wins (`spine≤1`); the rest
        // fall through and fill Ideal-ready AdmitIndep. No Estimate gate.
        if let Some(spine) = spine
            && let Some(p) = self.claim_handoff(ready, spine)
        {
            return Some(p);
        }
        let refused = match self.pop_local_admit(worker_i, ready) {
            LocalAdmit::Ready(tx) => return Some(self.execute_indep(tx, false)),
            LocalAdmit::Refused => true,
            LocalAdmit::Empty => false,
        };
        if let Some(tx) = self.steal_admit(worker_i, ready) {
            return Some(self.execute_indep(tx, refused));
        }
        if let Some(p) = self.help_release(ready) {
            return Some(p);
        }
        if let Some(tx) = self.steal_revalidate() {
            return Some(SfPick::Revalidate(tx));
        }
        None
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
        self.q_indep_len() + self.q_released.len() + self.q_ordered.len()
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
    pub(crate) fn seed_owner_local_pops(&self) -> usize {
        self.seed_owner_local_pops.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn steal_top_ns(&self) -> u64 {
        self.steal_top_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn exact_wake_missed_nopark(&self) -> usize {
        self.exact_wake_missed_nopark.load(Ordering::Relaxed)
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

    /// Prior crit ends for the post-exec span filter. `head == tail` is a
    /// zero-width span (all post-exec time counts as outside).
    pub(crate) fn note_span_ends(&self, head: TxIdx, tail: TxIdx) {
        self.span_head.store(head, Ordering::Relaxed);
        self.span_tail.store(tail, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn span_head(&self) -> usize {
        self.span_head.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn span_tail(&self) -> usize {
        self.span_tail.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn span_end_ns(&self) -> u64 {
        self.span_end_origin_ns.load(Ordering::Relaxed)
    }

    /// First writer wins. Later incarnations of the tail do not refresh it.
    pub(crate) fn publish_span_end(&self, origin_ns: u64, snap: SpanEndSnap) {
        if self
            .span_end_origin_ns
            .compare_exchange(0, origin_ns.max(1), Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return;
        }
        self.span_end_not_started
            .store(snap.not_started, Ordering::Relaxed);
        self.span_end_unfinished
            .store(snap.unfinished, Ordering::Relaxed);
        self.span_end_owed.store(snap.owed, Ordering::Relaxed);
        self.span_end_running.store(snap.running, Ordering::Relaxed);
        self.span_end_pending.store(snap.pending, Ordering::Relaxed);
        self.span_end_indep.store(snap.indep, Ordering::Relaxed);
        self.span_end_false_bits
            .store(snap.false_bits, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn note_max_origin(slot: &AtomicU64, origin_ns: u64) {
        let mut cur = slot.load(Ordering::Relaxed);
        while origin_ns > cur {
            match slot.compare_exchange_weak(cur, origin_ns, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(seen) => cur = seen,
            }
        }
    }

    #[inline]
    pub(crate) fn note_last_exec(&self, origin_ns: u64) {
        Self::note_max_origin(&self.last_exec_origin_ns, origin_ns);
    }

    #[inline]
    pub(crate) fn note_last_validate(&self, origin_ns: u64) {
        Self::note_max_origin(&self.last_validate_origin_ns, origin_ns);
    }

    #[inline]
    pub(crate) fn note_worker_exit(&self, origin_ns: u64) {
        Self::note_max_origin(&self.last_exit_origin_ns, origin_ns);
    }

    /// First full BlockQuiet observation. Later samples keep the earlier time.
    #[inline]
    pub(crate) fn note_quiet_true(&self, origin_ns: u64) {
        let _ = self.quiet_true_origin_ns.compare_exchange(
            0,
            origin_ns.max(1),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    #[inline]
    pub(crate) fn note_quiet_false(&self, bits: u64) {
        if bits != 0 {
            self.quiet_false_or.fetch_or(bits, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_post_span_exec_ns(&self, ns: u64) {
        if ns != 0 {
            self.post_span_exec_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_post_span_validate_ns(&self, ns: u64) {
        if ns != 0 {
            self.post_span_validate_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_post_span_heal_ns(&self, ns: u64) {
        if ns != 0 {
            self.post_span_heal_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_post_span_yield_ns(&self, ns: u64) {
        if ns != 0 {
            self.post_span_yield_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_post_span_park_ns(&self, ns: u64) {
        if ns != 0 {
            self.post_span_park_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_post_span_steal_ns(&self, ns: u64) {
        if ns != 0 {
            self.post_span_steal_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn inc_post_span_exec_n(&self) {
        self.post_span_exec_n.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn inc_post_span_idle_n(&self) {
        self.post_span_idle_n.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn post_span_probe(&self) -> PostSpanProbe {
        PostSpanProbe {
            span_end_origin_ns: self.span_end_origin_ns.load(Ordering::Relaxed),
            last_exec_origin_ns: self.last_exec_origin_ns.load(Ordering::Relaxed),
            last_validate_origin_ns: self.last_validate_origin_ns.load(Ordering::Relaxed),
            quiet_true_origin_ns: self.quiet_true_origin_ns.load(Ordering::Relaxed),
            last_exit_origin_ns: self.last_exit_origin_ns.load(Ordering::Relaxed),
            post_span_exec_ns: self.post_span_exec_ns.load(Ordering::Relaxed),
            post_span_validate_ns: self.post_span_validate_ns.load(Ordering::Relaxed),
            post_span_heal_ns: self.post_span_heal_ns.load(Ordering::Relaxed),
            post_span_yield_ns: self.post_span_yield_ns.load(Ordering::Relaxed),
            post_span_park_ns: self.post_span_park_ns.load(Ordering::Relaxed),
            post_span_steal_ns: self.post_span_steal_ns.load(Ordering::Relaxed),
            post_span_exec_n: self.post_span_exec_n.load(Ordering::Relaxed),
            post_span_idle_n: self.post_span_idle_n.load(Ordering::Relaxed),
            span_end_not_started: self.span_end_not_started.load(Ordering::Relaxed),
            span_end_unfinished: self.span_end_unfinished.load(Ordering::Relaxed),
            span_end_owed: self.span_end_owed.load(Ordering::Relaxed),
            span_end_running: self.span_end_running.load(Ordering::Relaxed),
            span_end_pending: self.span_end_pending.load(Ordering::Relaxed),
            span_end_indep: self.span_end_indep.load(Ordering::Relaxed),
            span_end_false_bits: self.span_end_false_bits.load(Ordering::Relaxed),
            quiet_false_or: self.quiet_false_or.load(Ordering::Relaxed),
        }
    }

    #[inline]
    pub(crate) fn add_idle_ns(&self, ns: u64) {
        self.idle_ns.fetch_add(ns, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn idle_ns(&self) -> u64 {
        self.idle_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn add_heal_ns(&self, ns: u64) {
        self.heal_ns.fetch_add(ns, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn heal_ns(&self) -> u64 {
        self.heal_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn add_post_exec_validate_ns(&self, ns: u64) {
        self.post_exec_validate_ns.fetch_add(ns, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn post_exec_validate_ns(&self) -> u64 {
        self.post_exec_validate_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn add_post_exec_in_span_ns(&self, ns: u64) {
        self.post_exec_in_span_ns.fetch_add(ns, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn post_exec_in_span_ns(&self) -> u64 {
        self.post_exec_in_span_ns.load(Ordering::Relaxed)
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
        self.local_admit
            .iter()
            .chain(self.local_late.iter())
            .map(|d| d.len())
            .sum()
    }

    /// Unpark one worker waiting on an ExactWakeToken.
    pub(crate) fn exact_wake_one(&self) {
        let start = self.wake_rr.fetch_add(1, Ordering::Relaxed);
        for k in 0..self.cores {
            let i = (start + k) % self.cores;
            if self.parked[i]
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                if let Some(t) = self.threads[i].lock().clone() {
                    t.unpark();
                    self.exact_wakes.fetch_add(1, Ordering::Relaxed);
                }
                return;
            }
        }
        self.exact_wake_missed_nopark
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Teardown only. Work distribution never broadcasts.
    pub(crate) fn wake_all_teardown(&self) {
        for i in 0..self.cores {
            self.parked[i].store(false, Ordering::Release);
            if let Some(t) = self.threads[i].lock().clone() {
                t.unpark();
            }
        }
    }

    /// Park this worker until one ExactWake or a short safety timeout.
    pub(crate) fn park_idle(&self, worker_i: usize) {
        let i = worker_i % self.cores;
        self.parks.fetch_add(1, Ordering::Relaxed);
        self.parked[i].store(true, Ordering::Release);
        if self.pending_work() > 0 {
            self.parked[i].store(false, Ordering::Release);
            return;
        }
        // Safety net so a missed unpark cannot pin `thread::scope`. Long
        // enough that it is not the scheduling heartbeat.
        std::thread::park_timeout(Duration::from_millis(20));
        self.parked[i].store(false, Ordering::Release);
    }

    #[inline]
    pub(crate) fn any_running(&self) -> bool {
        (0..self.block_size).any(|t| self.state[t].load(Ordering::Acquire) == ST_RUNNING)
    }

    #[inline]
    pub(crate) fn exact_wakes(&self) -> usize {
        self.exact_wakes.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn idle_parks(&self) -> usize {
        self.parks.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn help_releases(&self) -> usize {
        self.help_n.load(Ordering::Relaxed)
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
        /// Taken from [`super::access_spine::AccessSpine`]'s handoff slot.
        slot: bool,
    },
    Revalidate(TxIdx),
}

enum LocalAdmit {
    Ready(TxIdx),
    /// Gated head. Do not keep popping this deque.
    Refused,
    Empty,
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
        r.seed_begin(&ready, &stages, &sched, None, &[]);
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
        r.seed_begin(&ready, &stages, &sched, Some(4), &[]);
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
    fn local_lifo_and_cross_core_fifo_steal() {
        let ready = ReadyEdgeTable::new();
        let r = RunnableSet::new(6, 2);
        r.bind_worker(0);
        r.push(1, QueueKind::Indep);
        r.push(2, QueueKind::Indep);
        let SfPick::Execute { tx, slot, .. } = r.pick(0, &ready).expect("owner") else {
            panic!("expected execute");
        };
        assert_eq!(tx, 2, "owner pops LIFO");
        assert!(!slot);
        let SfPick::Execute { tx, .. } = r.pick(1, &ready).expect("thief") else {
            panic!("expected execute");
        };
        assert_eq!(tx, 1, "thief pops FIFO bottom");
        assert!(r.steal_n() >= 1, "cross-core steal counts");
        assert!(r.seed_owner_local_pops() >= 1, "owner pop is not a steal");
        ADMIT_OWNER.with(|c| c.set(None));
    }

    #[test]
    fn admit_shard_prefix_stays_on_worker0() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let sched = Scheduler::new(8);
        ready.note_consumer(1, 0);
        ready.plant_nearest_preds(0xabc, &[4, 6]);
        let r = RunnableSet::new(8, 2);
        r.seed_begin(&ready, &stages, &sched, Some(4), &[]);
        assert_eq!(
            ready.blocking_producer(6),
            Some(4),
            "spine successor is not seeded as Indep"
        );
        let SfPick::Execute { tx, slot, .. } = r.pick(0, &ready).expect("worker 0") else {
            panic!("expected execute");
        };
        assert_eq!(tx, 4, "chain head is worker 0's first pop");
        assert!(!slot);
        assert_eq!(r.steal_n(), 0);
        let SfPick::Execute { tx, .. } = r.pick(1, &ready).expect("worker 1") else {
            panic!("expected execute");
        };
        assert!(
            tx >= 5,
            "other cores start at the high end of their band, got {tx}"
        );
        assert_eq!(r.steal_n(), 0, "balanced local pop is not a steal");
        assert!(r.seed_owner_local_pops() >= 2);
        let mut saw_waiter = false;
        for w in 0..2 {
            for _ in 0..8 {
                let Some(SfPick::Execute { tx, .. }) = r.pick(w, &ready) else {
                    break;
                };
                if tx == 1 || tx == 6 {
                    saw_waiter = true;
                }
            }
        }
        assert!(!saw_waiter, "gated and spine successor stay off AdmitIndep");
    }

    #[test]
    fn ideal_timed_free_core_pops_heavier_ready_first() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let sched = Scheduler::new(8);
        let r = RunnableSet::new(8, 2);
        let mut work = vec![1u64; 8];
        work[5] = 5_000_000;
        r.seed_begin(&ready, &stages, &sched, None, &work);
        let SfPick::Execute { tx, .. } = r.pick(0, &ready).expect("worker 0") else {
            panic!("expected execute");
        };
        assert!(tx <= 3, "worker 0 still owns the low prefix, got {tx}");
        let SfPick::Execute { tx, .. } = r.pick(1, &ready).expect("worker 1") else {
            panic!("expected execute");
        };
        assert_eq!(tx, 5, "free core pops the heavy Ideal-ready tx first");
    }

    #[test]
    fn ideal_timed_late_wave_waits_behind_ready() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let sched = Scheduler::new(4);
        let wave = crate::specfence::wave::WaveParkTable::new();
        ready.plant_nearest_preds(0xabc, &[0, 3]);
        let r = RunnableSet::new(4, 1);
        r.seed_begin(&ready, &stages, &sched, None, &[]);
        assert_eq!(ready.blocking_producer(3), Some(0));
        ready.note_producer_done(0, &wave);
        assert!(
            r.wake_idle(3, QueueKind::Indep),
            "published successor must rejoin"
        );
        let SfPick::Execute { tx, .. } = r.pick(0, &ready).expect("ideal") else {
            panic!("expected execute");
        };
        assert_ne!(
            tx, 3,
            "a woken later wave must not preempt Ideal-ready still queued"
        );
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
