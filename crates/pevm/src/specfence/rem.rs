//! Region Execution Machine (REM) — Lean repair + research plant (kept separate).
//!
//! Phase-1 still drives one interpreter session per incarnation (`RunTx`), but
//! must emit region events and expose per-location validate semantics.
//!
//! # Two APIs (V5-P3 — do not conflate)
//!
//! | API | Entry | Default Lean? | What it arms |
//! |-----|-------|---------------|--------------|
//! | **SuffixRepair** (Lean) | [`PartialRetryTable::apply_suffix_repair`] | **yes** | hang-free RewindTo + journal FF + force-bind (SoftWait-wake subset) |
//! | **Research plant** | [`PartialRetryTable::research_apply_abort_repair`] | **no** (`SPECFENCE_ENABLE_INSPECT`) | RewindTo + journal FF + force-bind (may pair with inspect resume) |
//! | SoftWait wake (P4) | [`PartialRetryTable::try_arm_park_resume_at_k`] | yes (hang-free) | journal FF + force-bind only; **no** absolute jump |
//!
//! SpecFence-native resolve: validation fail → **SuffixRepair** (resume at certified
//! checkpoint ≤ k), not OCC-style head FullRestart. Absolute PC jump / valued
//! CallOutcome stay research-only (`SPECFENCE_ENABLE_INSPECT`).
//!
//! V5-P3 A/B on block 14689597: full inspect **hangs** → plant stays research-only.
//! Never graduate by default: absolute PC jump, multi-SSTORE/LOG jump mythology,
//! valued CallOutcome SC, fanout→WaitHard.
//!
//! P3 EarlyAbort: [`PartialRetryTable::arm_early_abort`] (RewindTo/FullRetry + force-bind).
//! P2 semantic PartialRetry: Bind-when-Data / SpecRead-else on certified prefix;
//! selective suffix invalidate (no global aborted stamp).
//!
//! M1a–M1l research checkpoints / jump / CallOutcome SC remain behind inspect flag.
//! M2 `WaveParkTable` + P4 SoftWait `(t,k)` wake stay on Lean (journal FF only).

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
use super::boundary::{BoundarySnapshot, CachedCallOutcome, JournalBlob};

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

/// M1h: certified-prefix storage write to re-apply into revm journal on absolute jump
/// without dumping a full journal-blob (avoids present_values Db/MV poison).
///
/// Only the changed slot is mutated after `load_account`; residual MvMemory
/// republish remains the source of truth for pevm write-set publish.
#[derive(Debug, Clone)]
pub(crate) struct StorageWriteReplay {
    pub address: Address,
    pub slot: U256,
    pub original: U256,
    pub present: U256,
    /// Interpreter gas.remaining when this write was flushed (post-SSTORE).
    pub gas_remaining_after: u64,
}

/// M1j: a LOG* event with the interpreter PC *after* the LOG opcode (step_end).
/// Restored on absolute jump only when `jump_snap.pc >= pc` (jump past that LOG).
#[derive(Debug, Clone)]
pub(crate) struct LogReplay {
    pub pc: usize,
    pub log: alloy_primitives::Log,
}

/// Serialized continuation for M1b/M1c/M1e RewindTo: restore SpecFence journal to
/// `cp`, FF certified-prefix reads, optionally PC-resume at `boundary`, and
/// restore revm journal blob for write-prefix SSTORE when absolute-jumping.
#[derive(Debug, Clone)]
pub(crate) struct ResumeContinuation {
    pub cp: CheckpointId,
    pub k_fail: usize,
    pub certified: Vec<MemoryLocationHash>,
    pub suffix_writes: Vec<MemoryLocationHash>,
    /// Write locations certified in the prefix (kept in MvMemory; not ESTIMATEd).
    pub prefix_writes: Vec<MemoryLocationHash>,
    /// Effect journal prefix `[0..=cp.k]` (empty if cp.k==0).
    pub effects: Vec<RegionAccess>,
    /// Checkpoints with `k <= cp.k`.
    pub checkpoints: Vec<Checkpoint>,
    /// Bound values for locations touched at `k <= cp.k`.
    pub values: HashMap<MemoryLocationHash, FfValue, BuildIdentityHasher>,
    /// M1c: lite/synthetic boundary snap at `cp` for skip-credit accounting.
    pub boundary: Option<BoundarySnapshot>,
    /// M1e: live Inspector snap for absolute PC jump (separate from repair lite snap).
    pub jump_snap: Option<BoundarySnapshot>,
    /// M1e: revm journal blob (touched state + logs) at certified-prefix boundary.
    pub journal_blob: Option<JournalBlob>,
    /// M1g: nested CallOutcomes completed at `k_end <= cp.k` (resume short-circuit).
    pub call_outcomes: Vec<CachedCallOutcome>,
    /// M1h: storage writes in certified prefix for controlled journal slot replay.
    pub write_replays: Vec<StorageWriteReplay>,
    /// M1j: LOG* events from certified prefix (restore on absolute jump past LOG;
    /// not stored in live_boundaries JournalBlob — that path hung inspect_run).
    pub log_replays: Vec<LogReplay>,
    /// M1l: valued nested CALL completed at/before tip but is missing from
    /// `call_outcomes` (cache miss). Absolute jump would drop the transfer.
    /// When valued outcomes are present in `call_outcomes`, jump is allowed at
    /// CALL-boundary (arm FF-seeds + transfer_loaded).
    pub valued_blocks_jump: bool,
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

/// SpecFence-native Lean abort / validation resolve outcome.
///
/// Default verb is [`Self::SuffixRepair`] (hang-free RewindTo + journal FF +
/// force-bind) when a certified mid-tx checkpoint exists before fail `k`.
/// Absolute PC jump / valued CallOutcome remain research-only.
#[derive(Debug, Clone)]
pub(crate) enum LeanAbortRepair {
    /// Certified checkpoint before fail `k` — RewindTo + FF armed (`is_rewind_resume`).
    SuffixRepair {
        certified: Vec<MemoryLocationHash>,
        suffix_writes: Vec<MemoryLocationHash>,
        /// Suggested `LiveLearner::note_reexec_cost` sample (~0.6).
        reexec_cost: f64,
    },
    /// Certified prefix but no usable mid-tx checkpoint → force-bind + head reexec.
    ForceBind {
        certified: Vec<MemoryLocationHash>,
        /// Suggested `LiveLearner::note_reexec_cost` sample.
        reexec_cost: f64,
    },
    /// No usable certified prefix / control-flow broken → FullRestart from tx head.
    FullRestart {
        reexec_cost: f64,
    },
}

impl LeanAbortRepair {
    #[inline]
    pub(crate) fn reexec_cost(&self) -> f64 {
        match self {
            Self::SuffixRepair { reexec_cost, .. }
            | Self::ForceBind { reexec_cost, .. }
            | Self::FullRestart { reexec_cost } => *reexec_cost,
        }
    }

    #[inline]
    pub(crate) fn did_force_bind(&self) -> bool {
        matches!(self, Self::ForceBind { .. } | Self::SuffixRepair { .. })
    }

    #[inline]
    pub(crate) fn is_suffix_repair(&self) -> bool {
        matches!(self, Self::SuffixRepair { .. })
    }
}

/// V5-P3 research-plant abort outcome — **not** used by Lean default.
///
/// Absolute PC jump / valued CallOutcome SC are **not** decided here; resume
/// path gates those behind `SPECFENCE_ABSOLUTE_JUMP` / inspect + safety checks.
#[derive(Debug, Clone)]
pub(crate) enum ResearchAbortRepair {
    /// Armed RewindTo + journal FF continuation + force-bind.
    RewindTo {
        certified: Vec<MemoryLocationHash>,
        suffix_writes: Vec<MemoryLocationHash>,
        /// Suggested `LiveLearner::note_reexec_cost` sample (~0.6).
        reexec_cost: f64,
    },
    /// No usable checkpoint → FullRestart (caller selective/full invalidate).
    FullRestart {
        reexec_cost: f64,
    },
}

impl ResearchAbortRepair {
    #[inline]
    pub(crate) fn reexec_cost(&self) -> f64 {
        match self {
            Self::RewindTo { reexec_cost, .. } | Self::FullRestart { reexec_cost } => *reexec_cost,
        }
    }

    #[inline]
    pub(crate) fn is_rewind(&self) -> bool {
        matches!(self, Self::RewindTo { .. })
    }
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
    /// M1e: live Inspector snap + journal blob keyed by effect ordinal `k`.
    live_boundaries: HashMap<usize, (BoundarySnapshot, JournalBlob), BuildIdentityHasher>,
    /// M1g: nested CallOutcomes captured this incarnation (ordered by call_seq).
    call_outcomes: Vec<CachedCallOutcome>,
    /// M1h: storage writes observed at finalize (for jump replay).
    write_replays: Vec<(MemoryLocationHash, StorageWriteReplay)>,
    /// M1i: post-SSTORE gas.remaining values captured in Inspector step_end order.
    post_sstore_gases: Vec<u64>,
    /// M1j: LOG* events captured this incarnation for jump-past-LOG replay.
    log_replays: Vec<LogReplay>,
    /// G1: last completed incarnation's final effect ordinal (survives reset).
    last_final_k: usize,
    /// G1: last completed incarnation's tx_gas_used (survives reset; for docs / future).
    last_tx_gas_used: u64,
}

impl PartialRetryState {
    pub(crate) fn reset(&mut self, incarnation: TxIncarnation) {
        // Preserve last_final_k / last_tx_gas_used across incarnations (G1 depth proxy).
        self.incarnation = incarnation;
        self.k = 0;
        self.first_k.clear();
        self.certified.clear();
        self.journal.clear();
        self.checkpoints.clear();
        self.value_snap.clear();
        self.live_boundaries.clear();
        self.call_outcomes.clear();
        self.write_replays.clear();
        self.post_sstore_gases.clear();
        self.log_replays.clear();
    }

    /// Record finish of an incarnation for cheap depth proxy on the next try.
    pub(crate) fn note_incarnation_finish(&mut self, tx_gas_used: u64) {
        if self.k > 0 {
            self.last_final_k = self.k;
        }
        if tx_gas_used > 0 {
            self.last_tx_gas_used = tx_gas_used;
        }
    }

    /// G1 cheap depth: `current_k / last_final_k` from a prior incarnation.
    /// Correlated with gross-work when prior finish completed; **not** gas/limit.
    /// Returns None on first incarnation or empty prior.
    pub(crate) fn estimate_effect_depth(&self) -> Option<f64> {
        let prior = self.last_final_k;
        if prior == 0 {
            return None;
        }
        let cur = self.k.max(1);
        Some(((cur as f64) / (prior as f64)).clamp(0.0, 1.0))
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

    /// M1i: record post-SSTORE gas.remaining from Inspector step_end.
    pub(crate) fn note_post_sstore_gas(&mut self, gas_remaining_after: u64) {
        if gas_remaining_after > 0 {
            self.post_sstore_gases.push(gas_remaining_after);
        }
    }

    /// Last post-SSTORE gas (sticky — finalize may note several slots after one SSTORE).
    pub(crate) fn last_post_sstore_gas(&self) -> u64 {
        self.post_sstore_gases.last().copied().unwrap_or(0)
    }

    /// M1h/M1i: record a storage write present/original for absolute-jump journal replay.
    pub(crate) fn note_write_replay(
        &mut self,
        location: MemoryLocationHash,
        mut replay: StorageWriteReplay,
    ) {
        if replay.gas_remaining_after == 0 {
            replay.gas_remaining_after = self.last_post_sstore_gas();
        }
        self.write_replays.retain(|(l, _)| *l != location);
        self.write_replays.push((location, replay));
    }

    /// M1j: replace captured LOG* events for this incarnation (jump-past-LOG replay).
    pub(crate) fn note_log_replays(&mut self, logs: Vec<LogReplay>) {
        self.log_replays = logs;
    }

    /// Build an M1b resume continuation for RewindTo(`cp`) with fail at `k_fail`.
    pub(crate) fn build_continuation(
        &self,
        cp: CheckpointId,
        k_fail: usize,
        certified: Vec<MemoryLocationHash>,
        suffix_writes: Vec<MemoryLocationHash>,
        prefix_writes: Vec<MemoryLocationHash>,
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
        // M1d: lite/synthetic boundary for skip-credit (never put live PC into repair snap).
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
                let steps = cp.k.max(effects.len()) as u64;
                if steps == 0 {
                    None
                } else {
                    Some(BoundarySnapshot {
                        pc: 0,
                        gas_remaining: 0,
                        gas_refunded: 0,
                        memory_words: 0,
                        memory_expansion_cost: 0,
                        call_depth: 1,
                        opcode_steps: steps,
                        stack: Vec::new(),
                        memory: Vec::new(),
                        code_hash: None,
                        bytecode_len: 0,
                        at_call_boundary: false,
                        post_sstore: false,
                    })
                }
            });
        // M1f side channel: prefer exact `cp.k` live snap; else nearest k ≤ cp.k.
        // Always-on step_end attach usually hits exact k; fallback covers CallEntry
        // floor when an EffectBoundary live exists at the certified prefix tip.
        let (jump_snap, journal_blob) = self
            .live_boundaries
            .get(&cp.k)
            .map(|(s, b)| (Some(s.clone()), Some(b.clone())))
            .or_else(|| {
                self.live_boundaries
                    .iter()
                    .filter(|(k, _)| **k <= cp.k)
                    .max_by_key(|(k, _)| *k)
                    .map(|(_, (s, b))| (Some(s.clone()), Some(b.clone())))
            })
            .unwrap_or((None, None));
        let journal_blob = journal_blob.filter(|b| !b.is_empty());
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
        let call_outcomes: Vec<CachedCallOutcome> = self
            .call_outcomes
            .iter()
            .filter(|c| c.k_end <= cp.k && c.depth > 1)
            .cloned()
            .collect();
        // Valued before tip with no cached outcome → refuse absolute jump (would
        // drop transfer). Valued present in call_outcomes → allowed at CALL-boundary.
        let valued_before_tip = self
            .call_outcomes
            .iter()
            .any(|c| c.depth > 1 && !c.value.is_zero() && c.k_end <= cp.k);
        let valued_blocks_jump = valued_before_tip
            && !call_outcomes.iter().any(|c| !c.value.is_zero());
        let prefix_set: HashSet<MemoryLocationHash, BuildIdentityHasher> =
            prefix_writes.iter().copied().collect();
        // Include writes whose first touch was before k_fail (effect order), even
        // when PartialRetry classified them as suffix due to missing certify.
        let write_replays: Vec<StorageWriteReplay> = self
            .write_replays
            .iter()
            .filter(|(loc, _)| {
                let fk = self.first_k.get(loc).copied().unwrap_or(usize::MAX);
                fk < k_fail
                    || prefix_set.contains(loc)
                    || (fk <= cp.k && certified_set.contains(loc))
            })
            .map(|(_, r)| r.clone())
            .collect();
        // M1k: only LOG* with pc ≤ jump tip — restore those skipped by absolute jump.
        let tip_pc = jump_snap.as_ref().map(|s| s.pc);
        let log_replays: Vec<LogReplay> = self
            .log_replays
            .iter()
            .filter(|lr| tip_pc.map(|pc| lr.pc <= pc).unwrap_or(false))
            .cloned()
            .collect();
        ResumeContinuation {
            cp,
            k_fail,
            certified,
            suffix_writes,
            prefix_writes,
            effects,
            checkpoints,
            values,
            boundary,
            jump_snap,
            journal_blob,
            call_outcomes,
            write_replays,
            log_replays,
            valued_blocks_jump,
        }
    }

    /// M1e: attach live Inspector snap + journal blob at current effect ordinal.
    ///
    /// Does **not** mutate checkpoint.boundary (lite snaps stay for repair/metrics);
    /// live data lives only in `live_boundaries` / `ResumeContinuation.jump_*`.
    pub(crate) fn attach_live_boundary(
        &mut self,
        snap: BoundarySnapshot,
        blob: JournalBlob,
    ) {
        let k = self.k;
        self.live_boundaries.insert(k, (snap, blob));
    }

    /// M1g: record nested CallOutcomes captured during inspect_run.
    pub(crate) fn note_call_outcomes(&mut self, calls: Vec<CachedCallOutcome>) {
        if calls.is_empty() {
            return;
        }
        // Replace with latest capture for this incarnation (inspect_run end).
        self.call_outcomes = calls;
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
        let tx_idx = self
            .journal
            .first()
            .map(|a| a.tx_idx)
            .unwrap_or(0);
        self.checkpoints
            .iter()
            .rev()
            .find(|cp| cp.id.k < k_fail)
            .map(|cp| cp.id)
            .or_else(|| {
                // M1f: if SpecRead skipped EffectBoundary but step_end attached a
                // live snap, rewind to that tip so jump_snap is available.
                self.live_boundaries
                    .keys()
                    .copied()
                    .filter(|k| *k > 0 && *k < k_fail)
                    .max()
                    .map(|k| CheckpointId {
                        tx_idx,
                        incarnation: self.incarnation,
                        k,
                    })
            })
            .or_else(|| {
                if k_fail > 0 {
                    Some(CheckpointId {
                        tx_idx,
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
    /// SoftWait wake consumed; next validation outcome → soft_wait_wake_{ok,reabort}.
    post_softwait_wake: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// Tx currently parked via FenceGraph SoftWait (not EarlyAbort-only park).
    softwait_parked: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// Pending repair for next execute / Retry loop of `t`.
    repair: DashMap<TxIdx, RepairPlan, BuildIdentityHasher>,
    /// M1b journal-FF continuation armed with RewindTo.
    ff_resume: DashMap<TxIdx, ResumeContinuation, BuildIdentityHasher>,
    /// M1e: last RewindTo resume applied an absolute jump (for abort→disable).
    last_jump_applied: DashMap<TxIdx, bool, BuildIdentityHasher>,
    /// M1e: absolute jump disabled after a jumped resume failed validation (anti-livelock).
    jump_disabled: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// After force_bind_reabort: next Lean execute should open narrow inspect to
    /// capture live jump_snap (CallEntry/EffectBoundary + Storage FF path).
    needs_live_capture: DashMap<TxIdx, (), BuildIdentityHasher>,
}

impl PartialRetryTable {
    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            states: (0..block_size)
                .map(|_| Mutex::new(PartialRetryState::default()))
                .collect(),
            force_bind: DashMap::default(),
            post_softwait_wake: DashMap::default(),
            softwait_parked: DashMap::default(),
            repair: DashMap::default(),
            ff_resume: DashMap::default(),
            last_jump_applied: DashMap::default(),
            jump_disabled: DashMap::default(),
            needs_live_capture: DashMap::default(),
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

    /// Live incarnation value snap (for value-stable RebindOnly at validation).
    pub(crate) fn snapped_value(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> Option<FfValue> {
        self.states.get(tx_idx).and_then(|slot| {
            slot.lock().unwrap().value_snap.get(&location).cloned()
        })
    }

    /// M1i: Inspector post-SSTORE gas capture for write-prefix jump gas-equality.
    pub(crate) fn note_post_sstore_gas(&self, tx_idx: TxIdx, gas_remaining_after: u64) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().note_post_sstore_gas(gas_remaining_after);
        }
    }

    /// M1h/M1i: record storage write present/original for absolute-jump journal replay.
    pub(crate) fn note_write_replay(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        replay: StorageWriteReplay,
    ) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().note_write_replay(location, replay);
        }
    }

    /// M1j: record LOG* events for absolute-jump past LOG (hang-free vs blob path).
    pub(crate) fn note_log_replays(&self, tx_idx: TxIdx, logs: Vec<LogReplay>) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().note_log_replays(logs);
        }
    }

    /// Locations with flushed write_replays in the live incarnation journal.
    pub(crate) fn write_replay_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        self.states
            .get(tx_idx)
            .map(|s| {
                s.lock()
                    .unwrap()
                    .write_replays
                    .iter()
                    .map(|(l, _)| *l)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Arm RewindTo + build M1b FF continuation from the failed incarnation's journal.
    pub(crate) fn arm_rewind_to(
        &self,
        tx_idx: TxIdx,
        cp: CheckpointId,
        k_fail: usize,
        certified: Vec<MemoryLocationHash>,
        suffix_writes: Vec<MemoryLocationHash>,
        prefix_writes: Vec<MemoryLocationHash>,
    ) {
        let cont = self.states[tx_idx].lock().unwrap().build_continuation(
            cp,
            k_fail,
            certified.clone(),
            suffix_writes.clone(),
            prefix_writes,
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

    /// P4 SoftWait wake: try hang-free resume at armed observe `k`.
    ///
    /// **Safe subset:** if a checkpoint with `0 < cp.k < armed_at_k` exists in the
    /// parked incarnation journal, arm existing RewindTo + journal FF (no new
    /// live-Interpreter park). Otherwise return [`ParkResumeKind::FullRetry`]
    /// (tx-grain head reexec — M2 behaviour). Absolute PC jump remains gated by
    /// M1e/M1l safety on the resume path — this only arms journal FF + force-bind.
    pub(crate) fn try_arm_park_resume_at_k(
        &self,
        tx_idx: TxIdx,
        armed_at_k: u64,
    ) -> ParkResumeKind {
        let k_fail = armed_at_k as usize;
        if k_fail == 0 {
            self.repair.insert(tx_idx, RepairPlan::FullRestart);
            return ParkResumeKind::FullRetry;
        }
        let Some(cp) = self.last_checkpoint_before(tx_idx, k_fail) else {
            self.repair.insert(tx_idx, RepairPlan::FullRestart);
            return ParkResumeKind::FullRetry;
        };
        // Require real mid-tx progress — synthetic CallEntry at k=0 alone is FullRetry.
        if cp.k == 0 || cp.k >= k_fail {
            self.repair.insert(tx_idx, RepairPlan::FullRestart);
            return ParkResumeKind::FullRetry;
        }

        let st = self.states[tx_idx].lock().unwrap();
        let mut certified: Vec<MemoryLocationHash> = st
            .certified
            .iter()
            .copied()
            .filter(|loc| st.first_k(*loc).unwrap_or(usize::MAX) <= cp.k)
            .collect();
        for access in &st.journal {
            if access.k <= cp.k
                && matches!(access.mode, AccessMode::Read)
                && !certified.contains(&access.location)
            {
                certified.push(access.location);
            }
        }
        if certified.is_empty() {
            drop(st);
            self.repair.insert(tx_idx, RepairPlan::FullRestart);
            return ParkResumeKind::FullRetry;
        }
        let mut suffix_writes = Vec::new();
        let mut prefix_writes = Vec::new();
        for access in &st.journal {
            if !matches!(access.mode, AccessMode::Write) {
                continue;
            }
            if access.k <= cp.k {
                if !prefix_writes.contains(&access.location) {
                    prefix_writes.push(access.location);
                }
            } else if !suffix_writes.contains(&access.location) {
                suffix_writes.push(access.location);
            }
        }
        drop(st);

        self.arm_rewind_to(
            tx_idx,
            cp,
            k_fail,
            certified.clone(),
            suffix_writes,
            prefix_writes,
        );
        self.set_force_bind(tx_idx, certified);
        ParkResumeKind::ResumeAtK {
            checkpoint_k: cp.k,
        }
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

    /// M1e: full FF continuation (for safety-gated absolute jump).
    pub(crate) fn ff_continuation(&self, tx_idx: TxIdx) -> Option<ResumeContinuation> {
        self.ff_resume.get(&tx_idx).map(|c| c.clone())
    }

    /// M1e: revm journal blob for write-prefix restore.
    pub(crate) fn ff_journal_blob(&self, tx_idx: TxIdx) -> Option<JournalBlob> {
        self.ff_resume
            .get(&tx_idx)
            .and_then(|c| c.journal_blob.clone())
    }

    /// M1e: store live Inspector snap + journal blob on the current incarnation state.
    pub(crate) fn attach_live_boundary(
        &self,
        tx_idx: TxIdx,
        snap: BoundarySnapshot,
        blob: JournalBlob,
    ) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().attach_live_boundary(snap, blob);
        }
    }

    /// M1g: persist nested CallOutcomes from Inspector capture into tx state.
    /// Also patch an already-armed RewindTo continuation (EarlyVal may arm mid-run
    /// before `with_plant_tls` ends and flushes captures).
    pub(crate) fn note_call_outcomes(&self, tx_idx: TxIdx, calls: Vec<CachedCallOutcome>) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().note_call_outcomes(calls.clone());
        }
        if let Some(mut cont) = self.ff_resume.get_mut(&tx_idx) {
            let cp_k = cont.cp.k;
            cont.call_outcomes = calls
                .into_iter()
                .filter(|c| c.k_end <= cp_k && c.depth > 1)
                .collect();
        }
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

    /// G1: cheap effect-progress depth proxy for π (None on first incarnation).
    pub(crate) fn estimate_effect_depth(&self, tx_idx: TxIdx) -> Option<f64> {
        self.states
            .get(tx_idx)
            .and_then(|s| s.lock().unwrap().estimate_effect_depth())
    }

    /// G1: record finished incarnation gas + k for next-try depth proxy.
    pub(crate) fn note_incarnation_finish(&self, tx_idx: TxIdx, tx_gas_used: u64) {
        if let Some(slot) = self.states.get(tx_idx) {
            slot.lock().unwrap().note_incarnation_finish(tx_gas_used);
        }
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

    /// True when a certified-prefix force_bind set is armed for this tx.
    pub(crate) fn has_force_bind(&self, tx_idx: TxIdx) -> bool {
        self.force_bind
            .get(&tx_idx)
            .is_some_and(|v| !v.is_empty())
    }

    /// Sticky resolve: union conflict locations into the armed force_bind set.
    pub(crate) fn extend_force_bind(
        &self,
        tx_idx: TxIdx,
        locations: &[MemoryLocationHash],
    ) {
        if locations.is_empty() {
            return;
        }
        let mut merged = self
            .force_bind
            .get(&tx_idx)
            .map(|v| v.clone())
            .unwrap_or_default();
        for &loc in locations {
            if !merged.contains(&loc) {
                merged.push(loc);
            }
        }
        self.set_force_bind(tx_idx, merged);
    }

    /// Mark SoftWait wake → next incarnation (for dig wake→validate counters).
    pub(crate) fn mark_post_softwait_wake(&self, tx_idx: TxIdx) {
        self.post_softwait_wake.insert(tx_idx, ());
    }

    /// Take SoftWait-wake-pending flag for this tx (if any).
    pub(crate) fn take_post_softwait_wake(&self, tx_idx: TxIdx) -> bool {
        self.post_softwait_wake.remove(&tx_idx).is_some()
    }

    pub(crate) fn clear_post_softwait_wake(&self, tx_idx: TxIdx) {
        self.post_softwait_wake.remove(&tx_idx);
    }

    pub(crate) fn mark_softwait_parked(&self, tx_idx: TxIdx) {
        self.softwait_parked.insert(tx_idx, ());
    }

    pub(crate) fn take_softwait_parked(&self, tx_idx: TxIdx) -> bool {
        self.softwait_parked.remove(&tx_idx).is_some()
    }

    /// SpecFence-native Lean resolve: **SuffixRepair-first** (default abort path).
    ///
    /// ```text
    /// if plan_partial_retry + checkpoint with 0 < cp.k < k_fail:
    ///   arm_rewind_to + journal FF + set_force_bind  → SuffixRepair
    ///   (same hang-free subset as SoftWait wake / try_arm_park_resume_at_k)
    /// else if certified prefix (no mid-tx cp):
    ///   set_force_bind; clear RewindTo → ForceBind (head reexec fallback)
    /// else:
    ///   clear_force_bind; clear_repair → FullRestart
    /// ```
    ///
    /// Absolute PC jump is **not** armed here — the execute path may hang-free
    /// narrow-arm via `jump_is_safe` on the SuffixRepair resume incarnation only
    /// (no whole-block `SPECFENCE_ENABLE_INSPECT`). After SuffixRepair,
    /// [`Self::is_rewind_resume`] is true so Lean takes the resume path
    /// (`record_resume`, FF seed, `try_ff_*`).
    pub(crate) fn apply_suffix_repair(
        &self,
        tx_idx: TxIdx,
        read_locations: &[MemoryLocationHash],
        invalid: &[MemoryLocationHash],
        write_locations: &[MemoryLocationHash],
    ) -> LeanAbortRepair {
        match self.plan_partial_retry(tx_idx, read_locations, invalid, write_locations) {
            Some(plan) if !plan.certified.is_empty() => {
                let k_fail = plan.k_fail;
                // Hang-free SoftWait-wake criteria: real mid-tx checkpoint before k.
                if k_fail > 0 {
                    if let Some(cp) = self.last_checkpoint_before(tx_idx, k_fail) {
                        if cp.k > 0 && cp.k < k_fail {
                            self.arm_rewind_to(
                                tx_idx,
                                cp,
                                k_fail,
                                plan.certified.clone(),
                                plan.suffix_writes.clone(),
                                plan.prefix_writes.clone(),
                            );
                            self.set_force_bind(tx_idx, plan.certified.clone());
                            return LeanAbortRepair::SuffixRepair {
                                certified: plan.certified,
                                suffix_writes: plan.suffix_writes,
                                reexec_cost: 0.6,
                            };
                        }
                    }
                }
                // Certified prefix but no usable mid-tx checkpoint → head ForceBind.
                self.set_force_bind(tx_idx, plan.certified.clone());
                self.clear_repair(tx_idx);
                LeanAbortRepair::ForceBind {
                    certified: plan.certified,
                    reexec_cost: 1.2,
                }
            }
            _ => {
                self.clear_force_bind(tx_idx);
                self.clear_repair(tx_idx);
                LeanAbortRepair::FullRestart { reexec_cost: 2.2 }
            }
        }
    }

    /// Alias for [`Self::apply_suffix_repair`] (legacy Lean abort name).
    #[inline]
    pub(crate) fn apply_lean_abort_repair(
        &self,
        tx_idx: TxIdx,
        read_locations: &[MemoryLocationHash],
        invalid: &[MemoryLocationHash],
        write_locations: &[MemoryLocationHash],
    ) -> LeanAbortRepair {
        self.apply_suffix_repair(tx_idx, read_locations, invalid, write_locations)
    }

    /// V5-P3 — **research plant** abort arming (thin wrapper over plan_repair + arm_rewind_to).
    ///
    /// Call only when `research_inspect_enabled()` / non-lean execute. Arms
    /// RewindTo + journal FF + force-bind when a checkpoint exists; otherwise
    /// clears to FullRestart. Does **not** enable absolute PC jump or valued
    /// CallOutcome SC (resume path). V5-P3 A/B: inspect hangs on 597 path →
    /// **not** graduated to Lean default.
    pub(crate) fn research_apply_abort_repair(
        &self,
        tx_idx: TxIdx,
        read_locations: &[MemoryLocationHash],
        invalid: &[MemoryLocationHash],
        write_locations: &[MemoryLocationHash],
    ) -> ResearchAbortRepair {
        match self.plan_partial_retry(tx_idx, read_locations, invalid, write_locations) {
            Some(plan) => match self.plan_repair(tx_idx, &plan) {
                RepairPlan::RewindTo {
                    certified,
                    suffix_writes,
                    cp,
                    k_fail,
                } => {
                    self.arm_rewind_to(
                        tx_idx,
                        cp,
                        k_fail,
                        certified.clone(),
                        suffix_writes.clone(),
                        plan.prefix_writes.clone(),
                    );
                    self.set_force_bind(tx_idx, certified.clone());
                    ResearchAbortRepair::RewindTo {
                        certified,
                        suffix_writes,
                        reexec_cost: 0.6,
                    }
                }
                RepairPlan::RebindOnly { .. } | RepairPlan::FullRestart => {
                    ResearchAbortRepair::FullRestart { reexec_cost: 2.2 }
                }
            },
            None => ResearchAbortRepair::FullRestart { reexec_cost: 2.2 },
        }
    }

    /// P3 EarlyAbort: arm rem repair for the next incarnation after cutting at `fail_location`.
    ///
    /// Mirrors EarlyVal-fail path: RewindTo + journal FF when a checkpoint exists,
    /// else FullRestart; always `set_force_bind` on the certified prefix so the
    /// reincarnation Bind/WaitHards instead of SpecReading the same early cross.
    /// Hang-freedom is the caller's Blocking(writer) — this only arms rem state.
    pub(crate) fn arm_early_abort(
        &self,
        tx_idx: TxIdx,
        fail_location: MemoryLocationHash,
        certified: Vec<MemoryLocationHash>,
    ) {
        let k_fail = self
            .first_k(tx_idx, fail_location)
            .unwrap_or_else(|| self.current_k(tx_idx));
        let cp = self.last_checkpoint_before(tx_idx, k_fail).unwrap_or(CheckpointId {
            tx_idx,
            incarnation: 0,
            k: 0,
        });
        if cp.k > 0 {
            self.arm_rewind_to(
                tx_idx,
                cp,
                k_fail,
                certified.clone(),
                Vec::new(),
                Vec::new(),
            );
        } else {
            // No certified checkpoint → FullRestart from tx head on next incarnation.
            self.set_repair(tx_idx, RepairPlan::FullRestart);
        }
        self.set_force_bind(tx_idx, certified);
    }

    pub(crate) fn set_repair(&self, tx_idx: TxIdx, plan: RepairPlan) {
        self.repair.insert(tx_idx, plan);
    }

    pub(crate) fn clear_repair(&self, tx_idx: TxIdx) {
        self.repair.remove(&tx_idx);
        self.ff_resume.remove(&tx_idx);
        self.last_jump_applied.remove(&tx_idx);
    }

    /// M1e: record whether this execute applied an absolute jump.
    pub(crate) fn note_jump_applied(&self, tx_idx: TxIdx, applied: bool) {
        self.last_jump_applied.insert(tx_idx, applied);
    }

    /// M1e: if last resume jumped and then failed validation, disable further jumps.
    pub(crate) fn disable_jump_after_failed_resume(&self, tx_idx: TxIdx) -> bool {
        let jumped = self
            .last_jump_applied
            .remove(&tx_idx)
            .map(|(_, v)| v)
            .unwrap_or(false);
        if jumped {
            self.jump_disabled.insert(tx_idx, ());
            true
        } else {
            false
        }
    }

    pub(crate) fn is_jump_disabled(&self, tx_idx: TxIdx) -> bool {
        self.jump_disabled.contains_key(&tx_idx)
    }

    pub(crate) fn clear_jump_disabled(&self, tx_idx: TxIdx) {
        self.jump_disabled.remove(&tx_idx);
    }

    /// Mark tx for one Lean inspect capture (live jump_snap) after force_bind_reabort.
    pub(crate) fn mark_needs_live_capture(&self, tx_idx: TxIdx) {
        self.needs_live_capture.insert(tx_idx, ());
    }

    /// Take live-capture prime flag (one-shot per force_bind_reabort).
    pub(crate) fn take_needs_live_capture(&self, tx_idx: TxIdx) -> bool {
        self.needs_live_capture.remove(&tx_idx).is_some()
    }

    pub(crate) fn needs_live_capture(&self, tx_idx: TxIdx) -> bool {
        self.needs_live_capture.contains_key(&tx_idx)
    }

    /// True when any write's first effect ordinal is at/after `k_fail` (true failed suffix).
    /// Writes before `k_fail` that PartialRetry classified as suffix (uncertified) are
    /// **not** true suffix — RebindOnly may still restore serializability.
    pub(crate) fn has_true_suffix_writes(
        &self,
        tx_idx: TxIdx,
        k_fail: usize,
        write_locations: &[MemoryLocationHash],
    ) -> bool {
        write_locations.iter().any(|&w| {
            self.first_k(tx_idx, w)
                .map(|k| k >= k_fail)
                .unwrap_or(false)
        })
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

    /// Best-effort global effect counter as SoftWait `k` when per-tx ordinal unavailable.
    pub(crate) fn effect_ordinal_hint(&self) -> u64 {
        self.effects.load(Ordering::Relaxed) as u64
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
    /// Cold/hint WaitHard, ESTIMATE Blocking, or validation Blocking without Soft arm.
    #[default]
    BlockingOther = 2,
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
    waiters_by_loc:
        DashMap<MemoryLocationHash, Vec<ParkedWait>, BuildIdentityHasher>,
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
    pub(crate) fn set_pending_park_softwait(
        &self,
        location: MemoryLocationHash,
        armed_at_k: u64,
    ) {
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
        self.waiters_by_loc
            .entry(location)
            .or_default()
            .push(entry);
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
            ParkKind::BlockingOther => {
                self.park_count_blocking_other.fetch_add(1, Ordering::Relaxed);
            }
        }
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
                if !woken.iter().any(|i: &ParkResumeIntent| i.waiter == p.waiter) {
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
                if !woken.iter().any(|i: &ParkResumeIntent| i.waiter == p.waiter) {
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
                ParkKind::BlockingOther => {
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
        let cont = st.build_continuation(cp, 4, vec![1, 2], vec![3, 4], vec![]);
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

#[cfg(test)]
mod p4_tk_park_tests {
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
    fn try_arm_park_resume_falls_back_when_k_zero_or_no_cp() {
        let table = PartialRetryTable::new(4);
        table.reset_incarnation(1, 0);
        // k=0 → FullRetry
        assert_eq!(
            table.try_arm_park_resume_at_k(1, 0),
            ParkResumeKind::FullRetry
        );
        // Journaled observe but only synthetic path with no mid-tx cp > 0
        table.note_access(1, 10, AccessMode::Read);
        // CallEntry-style cp at k after note would be at current k; push at k=1
        let _ = table.push_checkpoint(1, CheckpointKind::CallEntry);
        // armed_at_k == cp.k → need cp.k < armed_at_k; arm at same k → FullRetry
        assert_eq!(
            table.try_arm_park_resume_at_k(1, 1),
            ParkResumeKind::FullRetry
        );
    }

    #[test]
    fn try_arm_park_resume_at_k_when_checkpoint_before_k() {
        let table = PartialRetryTable::new(4);
        table.reset_incarnation(2, 0);
        // Build prefix: reads 1..=3 with cps at k=1 and k=2; SoftWait at k=3.
        for i in 1..=3 {
            let loc = i as MemoryLocationHash;
            table.note_access(2, loc, AccessMode::Read);
            table.note_certified(2, loc);
            table.states[2].lock().unwrap().note_value(
                loc,
                FfValue::Storage {
                    address: Address::ZERO,
                    slot: U256::from(i),
                    value: U256::from(i),
                    origin: None,
                },
            );
            if i <= 2 {
                let _ = table.push_checkpoint(2, CheckpointKind::EffectBoundary);
            }
        }
        assert_eq!(table.current_k(2), 3);
        let kind = table.try_arm_park_resume_at_k(2, 3);
        match kind {
            ParkResumeKind::ResumeAtK { checkpoint_k } => {
                assert_eq!(checkpoint_k, 2);
            }
            other => panic!("expected ResumeAtK, got {other:?}"),
        }
        assert!(table.is_rewind_resume(2));
        assert!(table.must_force_bind(2, 1));
        assert!(table.must_force_bind(2, 2));
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
}

#[cfg(test)]
mod p3_early_abort_tests {
    use super::*;

    #[test]
    fn arm_early_abort_full_restart_without_cp() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        table.note_access(0, 7, AccessMode::Read);
        table.arm_early_abort(0, 7, vec![1, 2]);
        assert!(table.must_force_bind(0, 1));
        assert!(table.must_force_bind(0, 2));
        // No mid-tx checkpoint → FullRestart repair, not RewindTo.
        assert!(!table.is_rewind_resume(0));
    }

    #[test]
    fn arm_early_abort_rewind_when_checkpoint_exists() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        table.note_access(0, 1, AccessMode::Read);
        table.note_certified(0, 1);
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 9, AccessMode::Read);
        table.arm_early_abort(0, 9, vec![1]);
        assert!(table.must_force_bind(0, 1));
        assert!(table.is_rewind_resume(0));
    }

    #[test]
    fn g1_effect_depth_proxy_survives_reset() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        assert!(table.estimate_effect_depth(0).is_none());
        for i in 0..10 {
            table.note_access(0, i as u64, AccessMode::Read);
        }
        table.note_incarnation_finish(0, 50_000);
        table.reset_incarnation(0, 1);
        // Mid next incarnation at k=3 → d≈0.3
        table.note_access(0, 100, AccessMode::Read);
        table.note_access(0, 101, AccessMode::Read);
        table.note_access(0, 102, AccessMode::Read);
        let d = table.estimate_effect_depth(0).unwrap();
        assert!((d - 0.3).abs() < 1e-9, "d={d}");
    }

    #[test]
    fn g2_plant_k_advances_without_hotset() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        assert_eq!(table.current_k(0), 0);
        table.note_access(0, 42, AccessMode::Read);
        table.note_access(0, 43, AccessMode::Write);
        assert_eq!(table.current_k(0), 2);
    }
}


#[cfg(test)]
mod abort_cheapening_tests {
    use super::*;

    /// Lean-style abort: CallEntry floor + certified prefix → RewindTo (not bare FullRestart).
    #[test]
    fn plan_repair_prefers_rewind_with_call_entry_floor() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        table.note_access(0, 10, AccessMode::Read);
        table.note_certified(0, 10);
        table.note_access(0, 11, AccessMode::Read);
        table.note_access(0, 20, AccessMode::Write);
        let _ = table.push_checkpoint(0, CheckpointKind::StorageWrite);

        let reads = vec![10u64, 11];
        let invalid = vec![11u64];
        let writes = vec![20u64];
        let plan = table
            .plan_partial_retry(0, &reads, &invalid, &writes)
            .expect("certified prefix");
        assert!(plan.certified.contains(&10));
        assert!(!plan.certified.contains(&11));
        match table.plan_repair(0, &plan) {
            RepairPlan::RewindTo {
                certified,
                k_fail,
                ..
            } => {
                assert!(certified.contains(&10));
                assert!(k_fail >= 1);
            }
            other => panic!("expected RewindTo, got {other:?}"),
        }
    }

    #[test]
    fn plan_repair_synthesizes_k0_rewind_when_no_explicit_cp() {
        // last_checkpoint_before synthesizes k=0 when k_fail>0 so abort can still
        // arm hang-free RewindTo+force-bind (cheaper than bare FullRestart).
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        table.note_access(0, 1, AccessMode::Read);
        table.note_access(0, 2, AccessMode::Read);
        let plan = table
            .plan_partial_retry(0, &[1, 2], &[2], &[])
            .expect("certified");
        match table.plan_repair(0, &plan) {
            RepairPlan::RewindTo { cp, certified, .. } => {
                assert_eq!(cp.k, 0);
                assert!(certified.contains(&1));
            }
            other => panic!("expected synthetic-k0 RewindTo, got {other:?}"),
        }
    }

    #[test]
    fn plan_partial_retry_none_when_no_certified_prefix() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        table.note_access(0, 1, AccessMode::Read);
        assert!(
            table
                .plan_partial_retry(0, &[1], &[1], &[])
                .is_none(),
            "all-invalid → no PartialRetry plan → FullRestart caller path"
        );
    }

    #[test]
    fn arm_rewind_sets_force_bind_for_next_incarnation() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        table.note_access(0, 5, AccessMode::Read);
        table.note_certified(0, 5);
        table.note_access(0, 6, AccessMode::Read);
        let plan = table
            .plan_partial_retry(0, &[5, 6], &[6], &[])
            .unwrap();
        let RepairPlan::RewindTo {
            cp,
            certified,
            k_fail,
            suffix_writes,
        } = table.plan_repair(0, &plan)
        else {
            panic!("expected RewindTo");
        };
        table.arm_rewind_to(
            0,
            cp,
            k_fail,
            certified.clone(),
            suffix_writes,
            plan.prefix_writes.clone(),
        );
        table.set_force_bind(0, certified);
        assert!(table.is_rewind_resume(0));
        assert!(table.must_force_bind(0, 5));
        assert!(!table.must_force_bind(0, 6));
    }

    /// SuffixRepair: mid-tx checkpoint before fail k → RewindTo + force-bind.
    #[test]
    fn apply_suffix_repair_arms_rewind_when_checkpoint_before_k() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        table.note_access(0, 10, AccessMode::Read);
        table.note_certified(0, 10);
        // Real mid-tx progress checkpoint (SoftWait hang-free subset requires cp.k > 0).
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 11, AccessMode::Read);

        match table.apply_suffix_repair(0, &[10, 11], &[11], &[]) {
            LeanAbortRepair::SuffixRepair {
                certified,
                suffix_writes: _,
                reexec_cost,
            } => {
                assert!(certified.contains(&10));
                assert!(!certified.contains(&11));
                assert!((reexec_cost - 0.6).abs() < 1e-9);
            }
            other => panic!("expected SuffixRepair, got {other:?}"),
        }
        assert!(table.must_force_bind(0, 10));
        assert!(
            table.is_rewind_resume(0),
            "SuffixRepair must leave RewindTo armed for journal FF"
        );
    }

    /// Certified prefix but only k=0 CallEntry → ForceBind head (no RewindTo).
    #[test]
    fn apply_suffix_repair_force_bind_without_mid_tx_checkpoint() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        table.note_access(0, 10, AccessMode::Read);
        table.note_certified(0, 10);
        table.note_access(0, 11, AccessMode::Read);

        match table.apply_suffix_repair(0, &[10, 11], &[11], &[]) {
            LeanAbortRepair::ForceBind { certified, reexec_cost } => {
                assert!(certified.contains(&10));
                assert!(!certified.contains(&11));
                assert!((reexec_cost - 1.2).abs() < 1e-9);
            }
            other => panic!("expected ForceBind fallback, got {other:?}"),
        }
        assert!(table.must_force_bind(0, 10));
        assert!(
            !table.is_rewind_resume(0),
            "k=0-only checkpoint must not arm RewindTo (hang-free SoftWait subset)"
        );
    }

    /// No certified prefix → FullRestart + cleared force-bind.
    #[test]
    fn apply_suffix_repair_full_restart_without_prefix() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        table.note_access(0, 1, AccessMode::Read);
        table.set_force_bind(0, vec![99]);
        match table.apply_suffix_repair(0, &[1], &[1], &[]) {
            LeanAbortRepair::FullRestart { reexec_cost } => {
                assert!((reexec_cost - 2.2).abs() < 1e-9);
            }
            other => panic!("expected FullRestart, got {other:?}"),
        }
        assert!(!table.must_force_bind(0, 99));
        assert!(!table.is_rewind_resume(0));
    }

    /// Research + Lean SuffixRepair both arm RewindTo when mid-tx cp exists.
    #[test]
    fn research_and_lean_suffix_repair_both_arm_rewind() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        table.note_access(0, 10, AccessMode::Read);
        table.note_certified(0, 10);
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 11, AccessMode::Read);

        match table.research_apply_abort_repair(0, &[10, 11], &[11], &[]) {
            ResearchAbortRepair::RewindTo {
                certified,
                reexec_cost,
                ..
            } => {
                assert!(certified.contains(&10));
                assert!((reexec_cost - 0.6).abs() < 1e-9);
            }
            other => panic!("expected research RewindTo, got {other:?}"),
        }
        assert!(table.is_rewind_resume(0));
        assert!(table.must_force_bind(0, 10));

        // Same inputs on Lean SuffixRepair must keep/re-arm RewindTo.
        match table.apply_suffix_repair(0, &[10, 11], &[11], &[]) {
            LeanAbortRepair::SuffixRepair { .. } => {}
            other => panic!("expected Lean SuffixRepair, got {other:?}"),
        }
        assert!(
            table.is_rewind_resume(0),
            "Lean SuffixRepair must arm RewindTo when mid-tx checkpoint exists"
        );
    }
    #[test]
    fn extend_force_bind_merges_conflict_locs_after_suffix_repair() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        let _ = table.note_access(0, 1, AccessMode::Read);
        table.note_certified(0, 1);
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        let _ = table.note_access(0, 2, AccessMode::Read);
        let repair = table.apply_suffix_repair(0, &[1, 2], &[2], &[]);
        assert!(matches!(repair, LeanAbortRepair::SuffixRepair { .. }) || matches!(repair, LeanAbortRepair::ForceBind { .. }));
        assert!(table.has_force_bind(0));
        table.extend_force_bind(0, &[2, 3]);
        assert!(table.must_force_bind(0, 2));
        assert!(table.must_force_bind(0, 3));
    }

    #[test]
    fn has_true_suffix_writes_ignores_uncertified_prefix() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.note_access(0, 10, AccessMode::Write); // k=1
        let _ = table.note_access(0, 20, AccessMode::Read); // k=2 fail
        assert!(!table.has_true_suffix_writes(0, 2, &[10]), "write before k_fail is not true suffix");
        let _ = table.note_access(0, 30, AccessMode::Write); // k=3 after fail
        assert!(table.has_true_suffix_writes(0, 2, &[10, 30]));
    }


}
