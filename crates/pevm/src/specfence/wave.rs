//! Wave park / ready deque — product scheduling surface (file-SRP).
//!
//! SoftWait Soft + SuffixRepair research stay in [`super::rem`] (quarantined;
//! Soft=0). This file **owns** WaveParkTable.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v9.4-file-srp.md`.

use parking_lot::Mutex;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use dashmap::DashMap;

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx};

// --- Plant v2 M2: wave park / ready-queue (L2) --------------------------------

thread_local! {
    /// Set when this worker just parked a WaitHard; next successful ready steal counts.
    static STEAL_AFTER_PARK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Location + SoftWait `k` of the in-flight WaitHard Blocking about to be confirmed in pevm.
    static PENDING_PARK: std::cell::Cell<Option<PendingPark>> =
        const { std::cell::Cell::new(None) };
}

/// How SoftWait wake should resume the waiter incarnation.
///
/// Hang-free subset (P4): never live-park the Interpreter. Either arm existing
/// RewindTo/FF when a real checkpoint exists before armed `k`, or fall back to
/// tx-grain FullRetry (head reexec) — same as M2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParkResumeKind {
    /// Checkpoint `checkpoint_k` with `0 < checkpoint_k < armed_at_k` — arm RewindTo+FF.
    ResumeAtK { checkpoint_k: usize },
    /// No safe mid-tx continuation — reexec from tx head.
    FullRetry,
}

/// Intent restored on SoftWait wake: resume waiter at SoftWait `armed_at_k` if safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParkResumeIntent {
    pub waiter: TxIdx,
    pub armed_at_k: u64,
    pub location: MemoryLocationHash,
}

/// Which resolve path parked the worker (breaks `wait_park_ns` into subtypes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub(crate) enum ParkKind {
    /// FenceGraph SoftWait Soft arm (π WaitHard / Bind→Await).
    SoftWaitSoft = 0,
    /// P3 EarlyAbort Blocking (no SoftWait arm).
    EarlyAbort = 1,
    /// ESTIMATE / aborted-incarnation / nonce Blocking (no SoftWait arm). Cold
    /// account-hint WaitHard was converted to SpecRead (BlockingOther cut).
    #[default]
    BlockingOther = 2,
    /// v9.1 PinWithoutThrow — park the waiter; do **not** steal-convert.
    PinHold = 3,
}

/// One WaitHard park entry. Carries SoftWait `(t,k)` for P4 wake resume intent.
///
/// Still **not** a live mid-effect Interpreter continuation — PEVM tasks remain
/// whole-tx; `armed_at_k` records where SoftWait armed so wake can try RewindTo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParkedWait {
    pub waiter: TxIdx,
    pub writer: TxIdx,
    pub location: MemoryLocationHash,
    /// SoftWait observe ordinal (per-tx PartialRetry `k`, else 0).
    pub armed_at_k: u64,
    pub kind: ParkKind,
}

/// Pending WaitHard park location + SoftWait `k` (thread-local until pevm parks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PendingPark {
    pub location: MemoryLocationHash,
    pub armed_at_k: u64,
    pub kind: ParkKind,
}

/// M2/P4 wave ready-queue + WaitHard park table.
///
/// **Grain (honest):** PEVM tasks are still whole-tx. Park = Block-STM
/// `Aborting` + dependency (`add_dependency`); wake = `ReadyToExecute` +
/// incarnation++. P4 stores SoftWait `armed_at_k` on the park entry and restores
/// a [`ParkResumeIntent`] so the next incarnation can arm RewindTo/FF when a
/// checkpoint exists; otherwise FullRetry from tx head (M2 behaviour).
/// Mid-effect live Interpreter park is **not** implemented (M1k/M1l hang lessons).
#[derive(Debug, Default)]
pub(crate) struct WaveParkTable {
    /// Min-heap: lower `TxIdx` first (frozen choice §8.3).
    ready: Mutex<BinaryHeap<Reverse<TxIdx>>>,
    /// Waiters parked on location ℓ (PublishWrite / writer-done wake).
    waiters_by_loc: DashMap<MemoryLocationHash, Vec<ParkedWait>, BuildIdentityHasher>,
    /// Waiters indexed by writer for `finish_execution` wake.
    waiters_by_writer: DashMap<TxIdx, Vec<ParkedWait>, BuildIdentityHasher>,
    /// Best-effort park start for `wait_park_ns`.
    park_started: DashMap<TxIdx, Instant, BuildIdentityHasher>,
    /// SoftWait wake resume intents consumed before the next `Vm::execute`.
    resume_intents: DashMap<TxIdx, ParkResumeIntent, BuildIdentityHasher>,
    wait_park_count: AtomicUsize,
    wait_park_ns: AtomicU64,
    /// Subtype split of `wait_park_ns` / counts (SoftWait Soft vs EarlyAbort vs other Blocking).
    park_ns_softwait: AtomicU64,
    park_ns_early_abort: AtomicU64,
    park_ns_blocking_other: AtomicU64,
    park_count_softwait: AtomicUsize,
    park_count_early_abort: AtomicUsize,
    park_count_blocking_other: AtomicUsize,
    /// Kind of the in-flight park for this waiter (for finish_park_ns split).
    park_kind_by_waiter: DashMap<TxIdx, ParkKind, BuildIdentityHasher>,
    ready_steal_on_wait: AtomicUsize,
    wave_width_sum: AtomicU64,
    wave_width_samples: AtomicUsize,
    /// P4: wakes that armed RewindTo at checkpoint before SoftWait `k`.
    park_resume_at_k: AtomicUsize,
    /// P4: wakes that fell back to tx-grain FullRetry.
    park_resume_full_retry: AtomicUsize,
}

impl WaveParkTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record location + SoftWait `k` + kind for the WaitHard about to return `Blocking`.
    pub(crate) fn set_pending_park(
        &self,
        location: MemoryLocationHash,
        armed_at_k: u64,
        kind: ParkKind,
    ) {
        PENDING_PARK.with(|c| {
            c.set(Some(PendingPark {
                location,
                armed_at_k,
                kind,
            }))
        });
    }

    /// SoftWait Soft arm pending park (FenceGraph arm present).
    pub(crate) fn set_pending_park_softwait(&self, location: MemoryLocationHash, armed_at_k: u64) {
        self.set_pending_park(location, armed_at_k, ParkKind::SoftWaitSoft);
    }

    /// Backward-compatible: pending park with `k=0` (tx-grain FullRetry on wake).
    pub(crate) fn set_pending_park_location(&self, location: MemoryLocationHash) {
        self.set_pending_park(location, 0, ParkKind::BlockingOther);
    }

    /// EarlyAbort Blocking pending park (no SoftWait SoT arm).
    pub(crate) fn set_pending_park_early_abort(&self, location: MemoryLocationHash) {
        self.set_pending_park(location, 0, ParkKind::EarlyAbort);
    }

    pub(crate) fn take_pending_park(&self) -> Option<PendingPark> {
        PENDING_PARK.with(|c| c.take())
    }

    pub(crate) fn take_pending_park_location(&self) -> Option<MemoryLocationHash> {
        self.take_pending_park().map(|p| p.location)
    }

    fn sample_wave_width_locked(&self, depth: usize) {
        self.wave_width_sum
            .fetch_add(depth as u64, Ordering::Relaxed);
        self.wave_width_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Park a WaitHard waiter at SoftWait `(t,k)`; worker must then steal (not spin).
    pub(crate) fn park(
        &self,
        waiter: TxIdx,
        writer: TxIdx,
        location: MemoryLocationHash,
        armed_at_k: u64,
    ) {
        self.park_with_kind(
            waiter,
            writer,
            location,
            armed_at_k,
            ParkKind::BlockingOther,
        );
    }

    /// Park with subtype so `wait_park_ns` can be split by SoftWait / EarlyAbort / other.
    pub(crate) fn park_with_kind(
        &self,
        waiter: TxIdx,
        writer: TxIdx,
        location: MemoryLocationHash,
        armed_at_k: u64,
        kind: ParkKind,
    ) {
        let entry = ParkedWait {
            waiter,
            writer,
            location,
            armed_at_k,
            kind,
        };
        self.waiters_by_loc.entry(location).or_default().push(entry);
        self.waiters_by_writer
            .entry(writer)
            .or_default()
            .push(entry);
        self.park_started.insert(waiter, Instant::now());
        self.park_kind_by_waiter.insert(waiter, kind);
        self.wait_park_count.fetch_add(1, Ordering::Relaxed);
        match kind {
            ParkKind::SoftWaitSoft => {
                self.park_count_softwait.fetch_add(1, Ordering::Relaxed);
            }
            ParkKind::EarlyAbort => {
                self.park_count_early_abort.fetch_add(1, Ordering::Relaxed);
            }
            ParkKind::BlockingOther | ParkKind::PinHold => {
                self.park_count_blocking_other
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        let depth = self.ready.lock().len();
        self.sample_wave_width_locked(depth);
        STEAL_AFTER_PARK.with(|c| c.set(true));
    }

    /// Undo park if `add_dependency` lost the race (writer already done).
    pub(crate) fn unpark(&self, waiter: TxIdx, writer: TxIdx, location: MemoryLocationHash) {
        if let Some(mut v) = self.waiters_by_loc.get_mut(&location) {
            v.retain(|p| p.waiter != waiter);
        }
        if let Some(mut v) = self.waiters_by_writer.get_mut(&writer) {
            v.retain(|p| p.waiter != waiter);
        }
        self.park_started.remove(&waiter);
        self.park_kind_by_waiter.remove(&waiter);
        self.resume_intents.remove(&waiter);
        STEAL_AFTER_PARK.with(|c| c.set(false));
    }

    fn record_resume_intent(&self, p: &ParkedWait) {
        self.resume_intents.insert(
            p.waiter,
            ParkResumeIntent {
                waiter: p.waiter,
                armed_at_k: p.armed_at_k,
                location: p.location,
            },
        );
    }

    /// Consume SoftWait wake resume intent (call before next `Vm::execute`).
    pub(crate) fn take_resume_intent(&self, waiter: TxIdx) -> Option<ParkResumeIntent> {
        self.resume_intents.remove(&waiter).map(|(_, v)| v)
    }

    pub(crate) fn peek_resume_intent(&self, waiter: TxIdx) -> Option<ParkResumeIntent> {
        self.resume_intents.get(&waiter).map(|v| *v)
    }

    pub(crate) fn note_park_resume_at_k(&self) {
        self.park_resume_at_k.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_park_resume_full_retry(&self) {
        self.park_resume_full_retry.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn park_resume_at_k(&self) -> usize {
        self.park_resume_at_k.load(Ordering::Relaxed)
    }

    pub(crate) fn park_resume_full_retry(&self) -> usize {
        self.park_resume_full_retry.load(Ordering::Relaxed)
    }

    /// Push a ready continuation; priority = lower TxIdx first.
    pub(crate) fn push_ready(&self, tx_idx: TxIdx) {
        let mut q = self.ready.lock();
        q.push(Reverse(tx_idx));
        self.sample_wave_width_locked(q.len());
    }

    /// Soft/Bayes edges: only reorder within the ready set (revocable).
    pub(crate) fn reorder_soft(&self, tx_idx: TxIdx) {
        self.push_ready(tx_idx);
    }

    /// Pop lowest TxIdx from the ready deque (stale entries skipped by caller).
    pub(crate) fn pop_ready(&self) -> Option<TxIdx> {
        self.ready.lock().pop().map(|Reverse(t)| t)
    }

    /// Arm park→steal convert without recording park idle (BlockingOther ESTIMATE path).
    /// Dependent stays Aborting in `transactions_dependents`; worker steals Ready writer.
    pub(crate) fn arm_steal_convert_without_park(&self) {
        STEAL_AFTER_PARK.with(|c| c.set(true));
    }

    /// Mark that a steal after park succeeded (wave ready **or** collaborative Ready).
    pub(crate) fn note_ready_steal_if_after_park(&self) {
        STEAL_AFTER_PARK.with(|c| {
            if c.get() {
                c.set(false);
                self.ready_steal_on_wait.fetch_add(1, Ordering::Relaxed);
            }
        });
    }

    /// True when this worker just parked and should prefer Ready steals.
    pub(crate) fn steal_after_park_pending(&self) -> bool {
        STEAL_AFTER_PARK.with(|c| c.get())
    }

    /// Clear steal-after-park flag without counting (e.g. idle yield).
    pub(crate) fn clear_steal_flag(&self) {
        STEAL_AFTER_PARK.with(|c| c.set(false));
    }

    /// Writer finished (`Executed`/`Validated`): wake location waiters → ready.
    ///
    /// Call after scheduler has set waiters to `ReadyToExecute` (dependents drain)
    /// or in addition when location publish is known. Accumulates `wait_park_ns`.
    pub(crate) fn wake_writer_done(&self, writer: TxIdx) -> Vec<TxIdx> {
        self.wake_writer_done_intents(writer)
            .into_iter()
            .map(|i| i.waiter)
            .collect()
    }

    /// Writer finished: wake parks, restore SoftWait `(t,k)` resume intents, push ready.
    pub(crate) fn wake_writer_done_intents(&self, writer: TxIdx) -> Vec<ParkResumeIntent> {
        let mut woken = Vec::new();
        if let Some((_, parked)) = self.waiters_by_writer.remove(&writer) {
            for p in parked {
                self.finish_park_ns(p.waiter);
                if let Some(mut v) = self.waiters_by_loc.get_mut(&p.location) {
                    v.retain(|x| x.waiter != p.waiter);
                }
                if !woken
                    .iter()
                    .any(|i: &ParkResumeIntent| i.waiter == p.waiter)
                {
                    self.record_resume_intent(&p);
                    let intent = ParkResumeIntent {
                        waiter: p.waiter,
                        armed_at_k: p.armed_at_k,
                        location: p.location,
                    };
                    woken.push(intent);
                    self.push_ready(p.waiter);
                }
            }
        }
        woken
    }

    /// PublishWrite wake for location ℓ (same as writer-done for that ℓ's waiters).
    pub(crate) fn wake_location(&self, location: MemoryLocationHash) -> Vec<TxIdx> {
        self.wake_location_intents(location)
            .into_iter()
            .map(|i| i.waiter)
            .collect()
    }

    /// Location publish wake with SoftWait `(t,k)` resume intents.
    pub(crate) fn wake_location_intents(
        &self,
        location: MemoryLocationHash,
    ) -> Vec<ParkResumeIntent> {
        let mut woken = Vec::new();
        if let Some((_, parked)) = self.waiters_by_loc.remove(&location) {
            for p in parked {
                self.finish_park_ns(p.waiter);
                if let Some(mut v) = self.waiters_by_writer.get_mut(&p.writer) {
                    v.retain(|x| x.waiter != p.waiter || x.location != location);
                }
                if !woken
                    .iter()
                    .any(|i: &ParkResumeIntent| i.waiter == p.waiter)
                {
                    self.record_resume_intent(&p);
                    woken.push(ParkResumeIntent {
                        waiter: p.waiter,
                        armed_at_k: p.armed_at_k,
                        location: p.location,
                    });
                    self.push_ready(p.waiter);
                }
            }
        }
        woken
    }

    fn finish_park_ns(&self, waiter: TxIdx) {
        if let Some((_, started)) = self.park_started.remove(&waiter) {
            let ns = started.elapsed().as_nanos() as u64;
            self.wait_park_ns.fetch_add(ns, Ordering::Relaxed);
            let kind = self
                .park_kind_by_waiter
                .remove(&waiter)
                .map(|(_, k)| k)
                .unwrap_or(ParkKind::BlockingOther);
            match kind {
                ParkKind::SoftWaitSoft => {
                    self.park_ns_softwait.fetch_add(ns, Ordering::Relaxed);
                }
                ParkKind::EarlyAbort => {
                    self.park_ns_early_abort.fetch_add(ns, Ordering::Relaxed);
                }
                ParkKind::BlockingOther | ParkKind::PinHold => {
                    self.park_ns_blocking_other.fetch_add(ns, Ordering::Relaxed);
                }
            }
        }
    }

    pub(crate) fn wait_park_count(&self) -> usize {
        self.wait_park_count.load(Ordering::Relaxed)
    }

    pub(crate) fn wait_park_ns(&self) -> u64 {
        self.wait_park_ns.load(Ordering::Relaxed)
    }

    pub(crate) fn ready_steal_on_wait(&self) -> usize {
        self.ready_steal_on_wait.load(Ordering::Relaxed)
    }

    pub(crate) fn park_ns_softwait(&self) -> u64 {
        self.park_ns_softwait.load(Ordering::Relaxed)
    }

    pub(crate) fn park_ns_early_abort(&self) -> u64 {
        self.park_ns_early_abort.load(Ordering::Relaxed)
    }

    pub(crate) fn park_ns_blocking_other(&self) -> u64 {
        self.park_ns_blocking_other.load(Ordering::Relaxed)
    }

    pub(crate) fn park_count_softwait(&self) -> usize {
        self.park_count_softwait.load(Ordering::Relaxed)
    }

    pub(crate) fn park_count_early_abort(&self) -> usize {
        self.park_count_early_abort.load(Ordering::Relaxed)
    }

    pub(crate) fn park_count_blocking_other(&self) -> usize {
        self.park_count_blocking_other.load(Ordering::Relaxed)
    }

    pub(crate) fn wave_width_mean(&self) -> f64 {
        let n = self.wave_width_samples.load(Ordering::Relaxed);
        if n == 0 {
            0.0
        } else {
            self.wave_width_sum.load(Ordering::Relaxed) as f64 / n as f64
        }
    }

    pub(crate) fn ready_depth(&self) -> usize {
        self.ready.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn park_stores_armed_at_k_and_wake_restores_intent() {
        let wave = WaveParkTable::new();
        wave.park_with_kind(5, 2, 99, 7, ParkKind::SoftWaitSoft);
        assert_eq!(wave.wait_park_count(), 1);
        assert_eq!(wave.park_count_softwait(), 1);
        let intents = wave.wake_writer_done_intents(2);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].waiter, 5);
        assert_eq!(intents[0].armed_at_k, 7);
        assert_eq!(intents[0].location, 99);
        let taken = wave.take_resume_intent(5).expect("intent");
        assert_eq!(taken.armed_at_k, 7);
        assert!(wave.take_resume_intent(5).is_none());
        assert_eq!(wave.pop_ready(), Some(5));
        assert!(wave.park_ns_softwait() > 0 || wave.wait_park_ns() > 0);
    }

    #[test]
    fn park_kind_splits_idle_ns() {
        let wave = WaveParkTable::new();
        wave.park_with_kind(1, 0, 10, 3, ParkKind::SoftWaitSoft);
        wave.park_with_kind(2, 0, 11, 0, ParkKind::EarlyAbort);
        wave.park_with_kind(3, 0, 12, 0, ParkKind::BlockingOther);
        assert_eq!(wave.park_count_softwait(), 1);
        assert_eq!(wave.park_count_early_abort(), 1);
        assert_eq!(wave.park_count_blocking_other(), 1);
        let _ = wave.wake_writer_done(0);
        assert_eq!(
            wave.wait_park_ns(),
            wave.park_ns_softwait() + wave.park_ns_early_abort() + wave.park_ns_blocking_other()
        );
    }

    #[test]
    fn pending_park_carries_k() {
        let wave = WaveParkTable::new();
        wave.set_pending_park(42, 9, ParkKind::SoftWaitSoft);
        let p = wave.take_pending_park().expect("pending");
        assert_eq!(p.location, 42);
        assert_eq!(p.armed_at_k, 9);
        assert!(wave.take_pending_park().is_none());
    }

    #[test]
    fn pinhold_does_not_use_softwait_counter() {
        let wave = WaveParkTable::new();
        wave.park_with_kind(4, 1, 7, 0, ParkKind::PinHold);
        assert_eq!(wave.park_count_softwait(), 0);
        assert_eq!(wave.park_count_blocking_other(), 1);
    }
}
