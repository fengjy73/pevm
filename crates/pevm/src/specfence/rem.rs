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
//! M1b: journal fast-forward + bound-value cache on RewindTo resume — SpecFence
//! effect journal restored to `cp`, certified-prefix DB reads served from FF
//! cache (skip MV lazy walks).
//! M1c: CALL/effect-boundary PC resume via stock Inspector — skip prefix opcodes
//! when a boundary snapshot is available; must NOT count as `evm_entries`.
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

use alloy_primitives::{Address, U256};
use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx, TxIncarnation};
use super::boundary::BoundarySnapshot;

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
#[derive(Debug, Clone)]
pub(crate) struct Checkpoint {
    pub id: CheckpointId,
    pub kind: CheckpointKind,
    /// SpecFence effect-journal length at capture (`== id.k`).
    pub journal_len: usize,
    /// M1c: interpreter boundary at capture (PC/stack/gas/steps), if available.
    pub boundary: Option<BoundarySnapshot>,
}

/// Bound value captured for journal fast-forward on RewindTo resume.
///
/// Used to skip MV lazy evaluation / storage I/O for certified-prefix reads
/// when the origin version is unchanged. Not a full revm journal blob.
#[derive(Debug, Clone)]
pub(crate) enum FfValue {
    Storage {
        address: Address,
        slot: U256,
        value: U256,
        /// `None` = storage origin; `Some` = MvMemory writer version.
        origin: Option<(TxIdx, TxIncarnation)>,
    },
    Basic {
        address: Address,
        basic: crate::AccountBasic,
        code_hash: Option<alloy_primitives::B256>,
        origin: Option<(TxIdx, TxIncarnation)>,
    },
}

/// Serialized continuation for M1b/M1c RewindTo: restore SpecFence journal to
/// `cp`, FF certified-prefix reads, and optionally PC-resume at `boundary`.
#[derive(Debug, Clone)]
pub(crate) struct ResumeContinuation {
    pub cp: CheckpointId,
    pub k_fail: usize,
    pub certified: Vec<MemoryLocationHash>,
    pub suffix_writes: Vec<MemoryLocationHash>,
    /// Effect journal prefix `[0..=cp.k]` (empty if cp.k==0).
    pub effects: Vec<RegionAccess>,
    /// Checkpoints with `k <= cp.k`.
    pub checkpoints: Vec<Checkpoint>,
    /// Bound values for locations touched at `k <= cp.k`.
    pub values: HashMap<MemoryLocationHash, FfValue, BuildIdentityHasher>,
    /// M1c: boundary snap at `cp` for PC resume (skip prefix opcodes).
    pub boundary: Option<BoundarySnapshot>,
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
    /// M1b/M1c: pairs with [`ResumeContinuation`] — SpecFence journal FF +
    /// bound-value cache + optional boundary PC resume (prefix opcodes skipped
    /// when snap present). Does **not** increment `evm_entries` / `tx_head_reexec`.
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
    /// Bound values observed this incarnation (for M1b journal FF).
    value_snap: HashMap<MemoryLocationHash, FfValue, BuildIdentityHasher>,
}

impl PartialRetryState {
    pub(crate) fn reset(&mut self, incarnation: TxIncarnation) {
        self.incarnation = incarnation;
        self.k = 0;
        self.first_k.clear();
        self.certified.clear();
        self.journal.clear();
        self.checkpoints.clear();
        self.value_snap.clear();
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
        self.push_checkpoint_with_boundary(tx_idx, kind, None)
    }

    pub(crate) fn push_checkpoint_with_boundary(
        &mut self,
        tx_idx: TxIdx,
        kind: CheckpointKind,
        boundary: Option<BoundarySnapshot>,
    ) -> CheckpointId {
        let id = CheckpointId {
            tx_idx,
            incarnation: self.incarnation,
            k: self.k,
        };
        self.checkpoints.push(Checkpoint {
            id,
            kind,
            journal_len: self.k,
            boundary,
        });
        id
    }

    pub(crate) fn note_value(&mut self, location: MemoryLocationHash, value: FfValue) {
        self.value_snap.insert(location, value);
    }

    /// Build an M1b resume continuation for RewindTo(`cp`) with fail at `k_fail`.
    pub(crate) fn build_continuation(
        &self,
        cp: CheckpointId,
        k_fail: usize,
        certified: Vec<MemoryLocationHash>,
        suffix_writes: Vec<MemoryLocationHash>,
    ) -> ResumeContinuation {
        let effects: Vec<RegionAccess> = self
            .journal
            .iter()
            .filter(|a| a.k <= cp.k)
            .copied()
            .collect();
        let checkpoints: Vec<Checkpoint> = self
            .checkpoints
            .iter()
            .filter(|c| c.id.k <= cp.k)
            .cloned()
            .collect();
        let boundary = self
            .checkpoints
            .iter()
            .rev()
            .find(|c| c.id.k == cp.k)
            .and_then(|c| c.boundary.clone())
            .or_else(|| {
                self.checkpoints
                    .iter()
                    .rev()
                    .find(|c| c.id.k <= cp.k && c.boundary.is_some())
                    .and_then(|c| c.boundary.clone())
            })
            .or_else(|| {
                // Synthetic CALL/effect-boundary credit when only CallEntry@0 exists:
                // use certified-prefix effect count as estimated opcode/work units.
                let steps = cp.k.max(effects.len()) as u64;
                if steps == 0 {
                    None
                } else {
                    Some(BoundarySnapshot {
                        pc: 0,
                        gas_remaining: 0,
                        call_depth: 1,
                        opcode_steps: steps,
                        stack: Vec::new(),
                        memory: Vec::new(),
                    })
                }
            });
        // Bound values for the whole certified prefix (k < k_fail), not only
        // up to cp — resume still force-binds those reads; FF cache skips MV walks.
        let certified_set: HashSet<MemoryLocationHash, BuildIdentityHasher> =
            certified.iter().copied().collect();
        let mut values = HashMap::with_hasher(BuildIdentityHasher::default());
        for (loc, val) in &self.value_snap {
            let fk = self.first_k.get(loc).copied().unwrap_or(usize::MAX);
            if fk < k_fail && (fk <= cp.k || certified_set.contains(loc)) {
                values.insert(*loc, val.clone());
            }
        }
        ResumeContinuation {
            cp,
            k_fail,
            certified,
            suffix_writes,
            effects,
            checkpoints,
            values,
            boundary,
        }
    }

    /// Restore SpecFence journal/checkpoints/values from an FF continuation
    /// (after `reset` for the new incarnation). Returns number of effects replayed.
    pub(crate) fn replay_continuation(&mut self, cont: &ResumeContinuation) -> usize {
        self.k = 0;
        self.first_k.clear();
        self.certified.clear();
        self.journal.clear();
        self.checkpoints.clear();
        self.value_snap.clear();
        for access in &cont.effects {
            self.k = access.k;
            self.first_k.entry(access.location).or_insert(access.k);
            self.journal.push(*access);
        }
        // Ensure k reflects cp even if effects empty (synthetic CallEntry at 0).
        self.k = self.k.max(cont.cp.k);
        for loc in &cont.certified {
            let fk = self.first_k.get(loc).copied().unwrap_or(0);
            if fk <= cont.cp.k {
                self.certified.insert(*loc);
            }
        }
        for cp in &cont.checkpoints {
            self.checkpoints.push(cp.clone());
        }
        for (loc, val) in &cont.values {
            self.value_snap.insert(*loc, val.clone());
        }
        cont.effects.len()
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
    /// M1b journal-FF continuation armed with RewindTo.
    ff_resume: DashMap<TxIdx, ResumeContinuation, BuildIdentityHasher>,
}

impl PartialRetryTable {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            states: (0..block_size)
                .map(|_| Mutex::new(PartialRetryState::default()))
                .collect(),
            force_bind: DashMap::default(),
            repair: DashMap::default(),
            ff_resume: DashMap::default(),
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

    pub(crate) fn note_value(&self, tx_idx: TxIdx, location: MemoryLocationHash, value: FfValue) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().note_value(location, value);
        }
    }

    /// Arm RewindTo + build M1b FF continuation from the failed incarnation's journal.
    pub(crate) fn arm_rewind_to(
        &self,
        tx_idx: TxIdx,
        cp: CheckpointId,
        k_fail: usize,
        certified: Vec<MemoryLocationHash>,
        suffix_writes: Vec<MemoryLocationHash>,
    ) {
        let cont = self.states[tx_idx].lock().unwrap().build_continuation(
            cp,
            k_fail,
            certified.clone(),
            suffix_writes.clone(),
        );
        self.ff_resume.insert(tx_idx, cont);
        self.repair.insert(
            tx_idx,
            RepairPlan::RewindTo {
                cp,
                certified,
                k_fail,
                suffix_writes,
            },
        );
    }

    /// After `reset_incarnation`, replay FF continuation into the fresh journal.
    /// Returns effects replayed (0 if none).
    pub(crate) fn replay_ff_if_armed(&self, tx_idx: TxIdx) -> usize {
        let Some(cont) = self.ff_resume.get(&tx_idx).map(|c| c.clone()) else {
            return 0;
        };
        self.states[tx_idx]
            .lock()
            .unwrap()
            .replay_continuation(&cont)
    }

    pub(crate) fn ff_value(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> Option<FfValue> {
        self.ff_resume
            .get(&tx_idx)
            .and_then(|c| c.values.get(&location).cloned())
    }

    pub(crate) fn ff_entries(&self, tx_idx: TxIdx) -> usize {
        self.ff_resume
            .get(&tx_idx)
            .map(|c| c.effects.len().max(c.values.len()))
            .unwrap_or(0)
    }

    pub(crate) fn clear_ff(&self, tx_idx: TxIdx) {
        self.ff_resume.remove(&tx_idx);
    }

    pub(crate) fn push_checkpoint(
        &self,
        tx_idx: TxIdx,
        kind: CheckpointKind,
    ) -> Option<CheckpointId> {
        self.push_checkpoint_with_boundary(tx_idx, kind, None)
    }

    pub(crate) fn push_checkpoint_with_boundary(
        &self,
        tx_idx: TxIdx,
        kind: CheckpointKind,
        boundary: Option<BoundarySnapshot>,
    ) -> Option<CheckpointId> {
        self.states.get(tx_idx).map(|slot| {
            slot.lock()
                .unwrap()
                .push_checkpoint_with_boundary(tx_idx, kind, boundary)
        })
    }

    /// Boundary snap attached to the rewind target checkpoint, if any.
    pub(crate) fn ff_boundary(&self, tx_idx: TxIdx) -> Option<BoundarySnapshot> {
        self.ff_resume
            .get(&tx_idx)
            .and_then(|c| c.boundary.clone())
    }

    /// Suffix write locations from the armed RewindTo continuation (empty if none).
    pub(crate) fn ff_suffix_writes(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        self.ff_resume
            .get(&tx_idx)
            .map(|c| c.suffix_writes.clone())
            .unwrap_or_default()
    }

    /// True when certified-prefix effects contain no writes (safe for live PC jump
    /// without revm journal-blob FF).
    pub(crate) fn ff_prefix_is_read_only(&self, tx_idx: TxIdx) -> bool {
        self.ff_resume
            .get(&tx_idx)
            .map(|c| c.effects.iter().all(|e| e.mode == AccessMode::Read))
            .unwrap_or(true)
    }

    pub(crate) fn ff_values(
        &self,
        tx_idx: TxIdx,
    ) -> Vec<(MemoryLocationHash, FfValue)> {
        self.ff_resume
            .get(&tx_idx)
            .map(|c| c.values.iter().map(|(k, v)| (*k, v.clone())).collect())
            .unwrap_or_default()
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
        self.ff_resume.remove(&tx_idx);
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

#[cfg(test)]
mod m1b_tests {
    use super::*;

    #[test]
    fn journal_ff_continuation_truncates_to_cp_and_replays() {
        let mut st = PartialRetryState::default();
        st.reset(1);
        // Simulate 4 reads then fail at k=4; cp at k=2.
        for i in 1..=4 {
            st.note_access(0, i as MemoryLocationHash, AccessMode::Read);
            st.note_value(
                i as MemoryLocationHash,
                FfValue::Storage {
                    address: Address::ZERO,
                    slot: U256::from(i),
                    value: U256::from(i),
                    origin: None,
                },
            );
            if i <= 2 {
                st.note_certified(i as MemoryLocationHash);
                st.push_checkpoint(0, CheckpointKind::EffectBoundary);
            }
        }
        let cp = st.last_checkpoint_before(4).expect("cp");
        assert_eq!(cp.k, 2);
        let cont = st.build_continuation(cp, 4, vec![1, 2], vec![3, 4]);
        assert_eq!(cont.effects.len(), 2);
        assert!(cont.values.contains_key(&1));
        assert!(cont.values.contains_key(&2));
        assert!(!cont.values.contains_key(&3));

        st.reset(2);
        let n = st.replay_continuation(&cont);
        assert_eq!(n, 2);
        assert_eq!(st.current_k(), 2);
        assert_eq!(st.checkpoint_count(), cont.checkpoints.len());
        assert!(st.certified_locations().contains(&1));
        assert!(st.certified_locations().contains(&2));
    }
}

