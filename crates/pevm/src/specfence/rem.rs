//! Region Execution Machine (REM) plant scaffolding for SpecFence Spec v1 / plant v2.
//!
//! Phase-1 still drives one interpreter session per incarnation (`RunTx`), but
//! must emit region events and expose per-location validate semantics.
//!
//! P2: semantic PartialRetry — revm re-executes from start, but π forces
//! Bind/WaitHard on the previously certified-prefix locations, and only
//! failed-suffix writes are selectively invalidated (no global aborted stamp).
//!
//! M1 (plant v2): checkpoints at CALL + write/effect boundaries; RewindTo /
//! RebindOnly demote head PartialRetry when a certified prefix exists.
//! True PC resume is TODO — resume path still re-enters the handler with
//! force-bind / journal fast-forward prep, but must NOT count as `evm_entries`.
//!
//! M2 (plant v2): `WaveParkTable` parks WaitHard at **tx grain** (Block-STM
//! Blocking style) and steals from a lower-TxIdx-first ready deque. Live
//! mid-effect Interpreter park is out of scope.

#![allow(dead_code)]
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use hashbrown::{HashMap, HashSet};

use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx, TxIncarnation};

/// Spec v1 REM task kinds (plant vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemTask {
    RunTx(TxIdx),
    PublishWrite {
        location: MemoryLocationHash,
        tx_idx: TxIdx,
        incarnation: TxIncarnation,
    },
    ValidateLocation {
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    },
    Repair {
        location: MemoryLocationHash,
        tx_idx: TxIdx,
    },
    FinalizeTx(TxIdx),
}

/// Access mode of a region effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessMode {
    Read,
    Write,
}

/// One world-state journal effect during `RunTx(t)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegionAccess {
    pub tx_idx: TxIdx,
    /// Monotonic effect ordinal `k` inside the current incarnation.
    pub k: usize,
    pub location: MemoryLocationHash,
    pub mode: AccessMode,
}

/// Checkpoint identity `(t, inc, k)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CheckpointId {
    pub tx_idx: TxIdx,
    pub incarnation: TxIncarnation,
    pub k: usize,
}

/// Why a checkpoint was taken (plant v2 M1 grain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckpointKind {
    /// External CALL / create frame entry (incl. tx top-level).
    CallEntry,
    /// CALL / create frame exit.
    CallExit,
    /// Account basic / code write boundary.
    AccountWrite,
    /// Storage slot write boundary.
    StorageWrite,
    /// Generic effect boundary (certified Bind / EarlyVal).
    EffectBoundary,
}

/// Snapshot recorded on the SpecFence path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    pub id: CheckpointId,
    pub kind: CheckpointKind,
}

/// Decision after classifying a validation failure for PartialRetry / M1 repair.
#[derive(Debug, Clone)]
pub(crate) struct PartialRetryPlan {
    /// Locations whose origins still matched (certified prefix).
    pub certified: Vec<MemoryLocationHash>,
    /// First failed-read effect ordinal.
    pub k_fail: usize,
    /// Write locations to ESTIMATE (failed suffix).
    pub suffix_writes: Vec<MemoryLocationHash>,
    /// Write locations left intact (prefix; no global aborted stamp).
    pub prefix_writes: Vec<MemoryLocationHash>,
}

/// Next-incarnation (or Retry-loop) repair op for plant v2 L1.
#[derive(Debug, Clone)]
pub(crate) enum RepairPlan {
    /// Origins wrong but suffix empty — patched in place (no new incarnation).
    RebindOnly {
        locations: Vec<MemoryLocationHash>,
    },
    /// Certified prefix OK — resume from last good checkpoint (not tx head).
    ///
    /// TODO(M1+): true PC / journal fast-forward from `cp`; today the resume
    /// entry still runs the handler with force-bind, but does **not** increment
    /// `evm_entries` / `tx_head_reexec`.
    RewindTo {
        cp: CheckpointId,
        certified: Vec<MemoryLocationHash>,
        k_fail: usize,
        suffix_writes: Vec<MemoryLocationHash>,
    },
    /// Empty prefix / control-flow broken — FullRestart from tx head.
    FullRestart,
}

/// Per-tx checkpoint / certified-prefix state for PartialRetry + M1 RewindTo.
#[derive(Debug, Default)]
pub(crate) struct PartialRetryState {
    /// Incarnation currently being journaled (for CheckpointId).
    incarnation: TxIncarnation,
    /// Monotonic effect ordinal for the current incarnation.
    k: usize,
    /// First-touch effect ordinal per location this incarnation.
    first_k: HashMap<MemoryLocationHash, usize, BuildIdentityHasher>,
    /// Locations that passed EarlyVal (or were otherwise certified) this incarnation.
    certified: HashSet<MemoryLocationHash, BuildIdentityHasher>,
    /// Full access journal (metrics / debugging).
    journal: Vec<RegionAccess>,
    /// Checkpoints captured this incarnation (CALL + write + effect).
    checkpoints: Vec<Checkpoint>,
}

impl PartialRetryState {
    pub(crate) fn reset(&mut self, incarnation: TxIncarnation) {
        self.incarnation = incarnation;
        self.k = 0;
        self.first_k.clear();
        self.certified.clear();
        self.journal.clear();
        self.checkpoints.clear();
    }

    pub(crate) fn note_access(
        &mut self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        mode: AccessMode,
    ) -> usize {
        self.k += 1;
        let k = self.k;
        self.first_k.entry(location).or_insert(k);
        self.journal.push(RegionAccess {
            tx_idx,
            k,
            location,
            mode,
        });
        k
    }

    pub(crate) fn note_certified(&mut self, location: MemoryLocationHash) {
        self.certified.insert(location);
    }

    pub(crate) fn push_checkpoint(&mut self, tx_idx: TxIdx, kind: CheckpointKind) -> CheckpointId {
        let id = CheckpointId {
            tx_idx,
            incarnation: self.incarnation,
            k: self.k,
        };
        self.checkpoints.push(Checkpoint { id, kind });
        id
    }

    /// Last checkpoint with `k < k_fail` (certified-prefix end).
    pub(crate) fn last_checkpoint_before(&self, k_fail: usize) -> Option<CheckpointId> {
        self.checkpoints
            .iter()
            .rev()
            .find(|cp| cp.id.k < k_fail)
            .map(|cp| cp.id)
            .or_else(|| {
                // Synthetic CallEntry at k=0 when we recorded no earlier cp.
                if k_fail > 0 {
                    Some(CheckpointId {
                        tx_idx: self
                            .journal
                            .first()
                            .map(|a| a.tx_idx)
                            .unwrap_or(0),
                        incarnation: self.incarnation,
                        k: 0,
                    })
                } else {
                    None
                }
            })
    }

    pub(crate) fn first_k(&self, location: MemoryLocationHash) -> Option<usize> {
        self.first_k.get(&location).copied()
    }

    pub(crate) fn certified_locations(&self) -> Vec<MemoryLocationHash> {
        self.certified.iter().copied().collect()
    }

    pub(crate) fn current_k(&self) -> usize {
        self.k
    }

    pub(crate) fn incarnation(&self) -> TxIncarnation {
        self.incarnation
    }

    pub(crate) fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }
}

/// Block-scoped PartialRetry / checkpoint plant.
#[derive(Debug)]
pub(crate) struct PartialRetryTable {
    states: Vec<Mutex<PartialRetryState>>,
    /// Locations π must Bind/WaitHard on the next incarnation of `t`.
    force_bind: DashMap<TxIdx, Vec<MemoryLocationHash>, BuildIdentityHasher>,
    /// Pending repair for next execute / Retry loop of `t`.
    repair: DashMap<TxIdx, RepairPlan, BuildIdentityHasher>,
}

impl PartialRetryTable {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            states: (0..block_size)
                .map(|_| Mutex::new(PartialRetryState::default()))
                .collect(),
            force_bind: DashMap::default(),
            repair: DashMap::default(),
        }
    }

    pub(crate) fn reset_incarnation(&self, tx_idx: TxIdx, incarnation: TxIncarnation) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().reset(incarnation);
        }
    }

    pub(crate) fn note_access(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        mode: AccessMode,
    ) -> usize {
        let mut st = self.states[tx_idx].lock().unwrap();
        st.note_access(tx_idx, location, mode)
    }

    pub(crate) fn note_certified(&self, tx_idx: TxIdx, location: MemoryLocationHash) {
        self.states[tx_idx]
            .lock()
            .unwrap()
            .note_certified(location);
    }

    pub(crate) fn push_checkpoint(
        &self,
        tx_idx: TxIdx,
        kind: CheckpointKind,
    ) -> Option<CheckpointId> {
        self.states.get(tx_idx).map(|slot| {
            slot.lock().unwrap().push_checkpoint(tx_idx, kind)
        })
    }

    pub(crate) fn last_checkpoint_before(
        &self,
        tx_idx: TxIdx,
        k_fail: usize,
    ) -> Option<CheckpointId> {
        self.states
            .get(tx_idx)?
            .lock()
            .unwrap()
            .last_checkpoint_before(k_fail)
    }

    pub(crate) fn current_k(&self, tx_idx: TxIdx) -> usize {
        self.states
            .get(tx_idx)
            .map(|s| s.lock().unwrap().current_k())
            .unwrap_or(0)
    }

    pub(crate) fn first_k(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> Option<usize> {
        self.states[tx_idx].lock().unwrap().first_k(location)
    }

    /// Locations π should force Bind/WaitHard for this incarnation (from prior repair).
    pub(crate) fn force_bind_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        self.force_bind
            .get(&tx_idx)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub(crate) fn must_force_bind(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> bool {
        self.force_bind
            .get(&tx_idx)
            .is_some_and(|v| v.iter().any(|l| *l == location))
    }

    pub(crate) fn set_force_bind(&self, tx_idx: TxIdx, locations: Vec<MemoryLocationHash>) {
        if locations.is_empty() {
            self.force_bind.remove(&tx_idx);
        } else {
            self.force_bind.insert(tx_idx, locations);
        }
    }

    pub(crate) fn clear_force_bind(&self, tx_idx: TxIdx) {
        self.force_bind.remove(&tx_idx);
    }

    pub(crate) fn set_repair(&self, tx_idx: TxIdx, plan: RepairPlan) {
        self.repair.insert(tx_idx, plan);
    }

    pub(crate) fn clear_repair(&self, tx_idx: TxIdx) {
        self.repair.remove(&tx_idx);
    }

    pub(crate) fn peek_repair(&self, tx_idx: TxIdx) -> Option<RepairPlan> {
        self.repair.get(&tx_idx).map(|v| v.clone())
    }

    pub(crate) fn is_rewind_resume(&self, tx_idx: TxIdx) -> bool {
        self.repair
            .get(&tx_idx)
            .is_some_and(|p| matches!(*p, RepairPlan::RewindTo { .. }))
    }

    /// Classify validation failure into PartialRetry plan, or `None` if unsafe → FullRetry.
    ///
    /// Safe when there is a non-empty certified-prefix of still-valid reads and we can
    /// split writes: prefix writes ⊆ certified (same location was a certified read before
    /// `k_fail`); everything else is failed-suffix.
    pub(crate) fn plan_partial_retry(
        &self,
        tx_idx: TxIdx,
        read_locations: &[MemoryLocationHash],
        invalid: &[MemoryLocationHash],
        write_locations: &[MemoryLocationHash],
    ) -> Option<PartialRetryPlan> {
        if invalid.is_empty() || read_locations.is_empty() {
            return None;
        }
        let st = self.states[tx_idx].lock().unwrap();
        let invalid_set: HashSet<MemoryLocationHash, BuildIdentityHasher> =
            invalid.iter().copied().collect();
        let mut certified: Vec<MemoryLocationHash> = read_locations
            .iter()
            .copied()
            .filter(|l| !invalid_set.contains(l))
            .collect();
        // Merge EarlyVal certifications that still validate.
        for loc in st.certified.iter() {
            if !invalid_set.contains(loc) && !certified.contains(loc) {
                certified.push(*loc);
            }
        }
        if certified.is_empty() {
            return None;
        }

        let mut k_fail = usize::MAX;
        for &loc in invalid {
            if let Some(k) = st.first_k(loc) {
                k_fail = k_fail.min(k);
            }
        }
        if k_fail == usize::MAX {
            // No journal (shouldn't happen under SpecFence) → unsafe.
            return None;
        }

        let certified_set: HashSet<MemoryLocationHash, BuildIdentityHasher> =
            certified.iter().copied().collect();

        let mut suffix_writes = Vec::new();
        let mut prefix_writes = Vec::new();
        for &w in write_locations {
            let wk = st.first_k(w).unwrap_or(usize::MAX);
            // Prefix-safe only if touched before k_fail AND location was certified.
            if wk < k_fail && certified_set.contains(&w) {
                prefix_writes.push(w);
            } else {
                suffix_writes.push(w);
            }
        }

        Some(PartialRetryPlan {
            certified,
            k_fail,
            suffix_writes,
            prefix_writes,
        })
    }

    /// Build an M1 repair plan from a PartialRetry classification.
    ///
    /// Caller may attempt `RebindOnly` first when `suffix_writes` is empty
    /// (patch origins in place without abort). Otherwise prefer `RewindTo`
    /// when a checkpoint exists; `FullRestart` only if prefix/control-flow
    /// cannot be recovered.
    pub(crate) fn plan_repair(
        &self,
        tx_idx: TxIdx,
        plan: &PartialRetryPlan,
    ) -> RepairPlan {
        if plan.certified.is_empty() {
            return RepairPlan::FullRestart;
        }
        match self.last_checkpoint_before(tx_idx, plan.k_fail) {
            Some(cp) => RepairPlan::RewindTo {
                cp,
                certified: plan.certified.clone(),
                k_fail: plan.k_fail,
                suffix_writes: plan.suffix_writes.clone(),
            },
            None => RepairPlan::FullRestart,
        }
    }
}

/// Per-block REM counters (Phase-2 checkpoint prep).
#[derive(Debug, Default)]
pub(crate) struct RemCounters {
    /// Effects observed this block (sum of `k` advances).
    pub effects: AtomicUsize,
    /// Times a checkpoint opportunity was recorded (every successful
    /// per-location validate or every `K` effects).
    pub checkpoint_opportunities: AtomicUsize,
}

impl RemCounters {
    pub(crate) fn note_effect(&self) -> usize {
        self.effects.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub(crate) fn note_checkpoint_opportunity(&self) {
        self.checkpoint_opportunities
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn checkpoint_opportunities(&self) -> usize {
        self.checkpoint_opportunities.load(Ordering::Relaxed)
    }
}

/// Per-worker effect ordinal for the current incarnation.
#[derive(Debug, Default)]
pub(crate) struct EffectOrdinal {
    k: usize,
}

impl EffectOrdinal {
    pub(crate) fn reset(&mut self) {
        self.k = 0;
    }

    pub(crate) fn next(&mut self) -> usize {
        self.k += 1;
        self.k
    }

    pub(crate) fn current(&self) -> usize {
        self.k
    }
}

// --- Plant v2 M2: wave park / ready-queue (L2) --------------------------------

thread_local! {
    /// Set when this worker just parked a WaitHard; next successful ready steal counts.
    static STEAL_AFTER_PARK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Location of the in-flight WaitHard Blocking about to be confirmed in pevm.
    static PENDING_PARK_LOC: std::cell::Cell<Option<MemoryLocationHash>> =
        const { std::cell::Cell::new(None) };
}

/// One WaitHard park entry (tx-level continuation — not mid-effect Interpreter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParkedWait {
    pub waiter: TxIdx,
    pub writer: TxIdx,
    pub location: MemoryLocationHash,
}

/// M2 wave ready-queue + WaitHard park table.
///
/// **Grain (honest):** PEVM tasks are still whole-tx. Park = Block-STM
/// `Aborting` + dependency (`add_dependency`); wake = `ReadyToExecute` +
/// incarnation++ (resume from tx head or M1 RewindTo force-bind). The rayon
/// worker never spins inside WaitHard — it returns to `next_task` / steals.
/// Mid-effect live Interpreter park is not implemented.
#[derive(Debug, Default)]
pub(crate) struct WaveParkTable {
    /// Min-heap: lower `TxIdx` first (frozen choice §8.3).
    ready: Mutex<BinaryHeap<Reverse<TxIdx>>>,
    /// Waiters parked on location ℓ (PublishWrite / writer-done wake).
    waiters_by_loc:
        DashMap<MemoryLocationHash, Vec<ParkedWait>, BuildIdentityHasher>,
    /// Waiters indexed by writer for `finish_execution` wake.
    waiters_by_writer: DashMap<TxIdx, Vec<ParkedWait>, BuildIdentityHasher>,
    /// Best-effort park start for `wait_park_ns`.
    park_started: DashMap<TxIdx, Instant, BuildIdentityHasher>,
    wait_park_count: AtomicUsize,
    wait_park_ns: AtomicU64,
    ready_steal_on_wait: AtomicUsize,
    wave_width_sum: AtomicU64,
    wave_width_samples: AtomicUsize,
}

impl WaveParkTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record location for the WaitHard that is about to return `Blocking`.
    pub(crate) fn set_pending_park_location(&self, location: MemoryLocationHash) {
        PENDING_PARK_LOC.with(|c| c.set(Some(location)));
    }

    pub(crate) fn take_pending_park_location(&self) -> Option<MemoryLocationHash> {
        PENDING_PARK_LOC.with(|c| c.take())
    }

    fn sample_wave_width_locked(&self, depth: usize) {
        self.wave_width_sum
            .fetch_add(depth as u64, Ordering::Relaxed);
        self.wave_width_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Park a WaitHard waiter; worker must then steal (not spin).
    pub(crate) fn park(
        &self,
        waiter: TxIdx,
        writer: TxIdx,
        location: MemoryLocationHash,
    ) {
        let entry = ParkedWait {
            waiter,
            writer,
            location,
        };
        self.waiters_by_loc
            .entry(location)
            .or_default()
            .push(entry);
        self.waiters_by_writer
            .entry(writer)
            .or_default()
            .push(entry);
        self.park_started.insert(waiter, Instant::now());
        self.wait_park_count.fetch_add(1, Ordering::Relaxed);
        let depth = self.ready.lock().unwrap().len();
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
        STEAL_AFTER_PARK.with(|c| c.set(false));
    }

    /// Push a ready continuation; priority = lower TxIdx first.
    pub(crate) fn push_ready(&self, tx_idx: TxIdx) {
        let mut q = self.ready.lock().unwrap();
        q.push(Reverse(tx_idx));
        self.sample_wave_width_locked(q.len());
    }

    /// Soft/Bayes edges: only reorder within the ready set (revocable).
    pub(crate) fn reorder_soft(&self, tx_idx: TxIdx) {
        self.push_ready(tx_idx);
    }

    /// Pop lowest TxIdx from the ready deque (stale entries skipped by caller).
    pub(crate) fn pop_ready(&self) -> Option<TxIdx> {
        self.ready.lock().unwrap().pop().map(|Reverse(t)| t)
    }

    /// Mark that a steal after park succeeded.
    pub(crate) fn note_ready_steal_if_after_park(&self) {
        STEAL_AFTER_PARK.with(|c| {
            if c.get() {
                c.set(false);
                self.ready_steal_on_wait.fetch_add(1, Ordering::Relaxed);
            }
        });
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
        let mut woken = Vec::new();
        if let Some((_, parked)) = self.waiters_by_writer.remove(&writer) {
            for p in parked {
                self.finish_park_ns(p.waiter);
                if let Some(mut v) = self.waiters_by_loc.get_mut(&p.location) {
                    v.retain(|x| x.waiter != p.waiter);
                }
                if !woken.contains(&p.waiter) {
                    woken.push(p.waiter);
                    self.push_ready(p.waiter);
                }
            }
        }
        woken
    }

    /// PublishWrite wake for location ℓ (same as writer-done for that ℓ's waiters).
    pub(crate) fn wake_location(&self, location: MemoryLocationHash) -> Vec<TxIdx> {
        let mut woken = Vec::new();
        if let Some((_, parked)) = self.waiters_by_loc.remove(&location) {
            for p in parked {
                self.finish_park_ns(p.waiter);
                if let Some(mut v) = self.waiters_by_writer.get_mut(&p.writer) {
                    v.retain(|x| x.waiter != p.waiter || x.location != location);
                }
                if !woken.contains(&p.waiter) {
                    woken.push(p.waiter);
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

    pub(crate) fn wave_width_mean(&self) -> f64 {
        let n = self.wave_width_samples.load(Ordering::Relaxed);
        if n == 0 {
            0.0
        } else {
            self.wave_width_sum.load(Ordering::Relaxed) as f64 / n as f64
        }
    }

    pub(crate) fn ready_depth(&self) -> usize {
        self.ready.lock().unwrap().len()
    }
}
