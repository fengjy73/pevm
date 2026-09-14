//! Region Execution Machine (REM) — Lean SuffixRepair + SoftWait Soft research.
//!
//! WavePark / WaitForDependency live in [`super::wave`] (file-SRP). SoftWait Soft stays
//! here as a **quarantined** museum (Soft=0 on the product path).
//!
//! Phase-1 still drives one interpreter session per incarnation (`RunTx`), but
//! must emit region events and expose per-location validate semantics.
//!
//! # Two APIs (V5-P3 — do not conflate)
//!
//! | API | Entry | Default Lean? | What it arms |
//! |-----|-------|---------------|--------------|
//! | **SuffixRepair** (Lean) | [`PartialRetryTable::apply_suffix_repair`] | **yes** | hang-free RewindTo + journal FF + force-ordered_admit (SoftWait-wake subset) |
//! | **Research plant** | [`PartialRetryTable::research_apply_abort_repair`] | **no** (`SPECFENCE_ENABLE_INSPECT`) | RewindTo + journal FF + force-ordered_admit (may pair with inspect resume) |
//! | SoftWait wake (P4) | [`PartialRetryTable::try_arm_park_resume_at_k`] | yes (hang-free) | journal FF + force-ordered_admit only; **no** absolute jump |
//!
//! SpecFence-native resolve: validation fail → **SuffixRepair** (resume at certified
//! checkpoint ≤ k), not OCC-style head FullAbortReexecute. Absolute PC jump / valued
//! CallOutcome stay research-only (`SPECFENCE_ENABLE_INSPECT`).
//!
//! V5-P3 A/B on block 14689597: full inspect **hangs** → plant stays research-only.
//! Never graduate by default: absolute PC jump, multi-SSTORE/LOG jump mythology,
//! valued CallOutcome SC, fanout→WaitHard.
//!
//! P3 EarlyAbort: [`PartialRetryTable::arm_early_abort`] (RewindTo/FullAbortReexecute + force-ordered_admit).
//! P2 semantic PartialRetry: OrderedAdmit-when-Data / OptimisticRead-else on certified prefix;
//! selective suffix invalidate (no global aborted stamp).
//!
//! M1a–M1l research checkpoints / jump / CallOutcome SC remain behind inspect flag.
//! WavePark lives in [`super::wave`]. This file is SoftWait/SuffixRepair research (Soft=0).

#![allow(dead_code)]
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use dashmap::DashMap;
use hashbrown::{HashMap, HashSet};

use super::boundary::{BoundarySnapshot, CachedCallOutcome, JournalBlob};
use super::wave::ParkResumeKind;
use crate::{BuildIdentityHasher, MemoryLocationHash, TxIdx, TxIncarnation};
use alloy_primitives::{Address, U256};

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
    /// Generic effect boundary (certified OrderedAdmit / EarlyVal).
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
    RebindOnly { locations: Vec<MemoryLocationHash> },
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
    /// Empty prefix / control-flow broken — FullAbortReexecute from tx head.
    FullAbortReexecute,
}

/// SpecFence-native Lean abort / validation resolve outcome.
///
/// Default verb is [`Self::SuffixRepair`] (hang-free RewindTo + journal FF +
/// force-ordered_admit) when a certified mid-tx checkpoint exists before fail `k`.
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
    /// Certified prefix but no usable mid-tx checkpoint → force-ordered_admit + head reexec.
    /// `suffix_writes` alone are ESTIMATEd; certified/`prefix` Data stays (no selective abort stamp).
    ForceOrderedAdmit {
        certified: Vec<MemoryLocationHash>,
        /// Failed-suffix write locations to ESTIMATE (certified prefix kept as Data).
        suffix_writes: Vec<MemoryLocationHash>,
        /// Suggested `LiveLearner::note_reexec_cost` sample.
        reexec_cost: f64,
    },
    /// No usable certified prefix / control-flow broken → FullAbortReexecute from tx head.
    FullAbortReexecute { reexec_cost: f64 },
}

impl LeanAbortRepair {
    #[inline]
    pub(crate) fn reexec_cost(&self) -> f64 {
        match self {
            Self::SuffixRepair { reexec_cost, .. }
            | Self::ForceOrderedAdmit { reexec_cost, .. }
            | Self::FullAbortReexecute { reexec_cost } => *reexec_cost,
        }
    }

    #[inline]
    pub(crate) fn did_force_ordered_admit(&self) -> bool {
        matches!(
            self,
            Self::ForceOrderedAdmit { .. } | Self::SuffixRepair { .. }
        )
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
    /// Armed RewindTo + journal FF continuation + force-ordered_admit.
    RewindTo {
        certified: Vec<MemoryLocationHash>,
        suffix_writes: Vec<MemoryLocationHash>,
        /// Suggested `LiveLearner::note_reexec_cost` sample (~0.6).
        reexec_cost: f64,
    },
    /// No usable checkpoint → FullAbortReexecute (caller selective/full invalidate).
    FullAbortReexecute { reexec_cost: f64 },
}

impl ResearchAbortRepair {
    #[inline]
    pub(crate) fn reexec_cost(&self) -> f64 {
        match self {
            Self::RewindTo { reexec_cost, .. } | Self::FullAbortReexecute { reexec_cost } => {
                *reexec_cost
            }
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
    /// Incarnation-stable seen ℓ (tx72-class). Survives `reset`.
    inc_carry_seen: HashSet<MemoryLocationHash, BuildIdentityHasher>,
    /// First-seen value snaps carried across repair incarnations.
    inc_carry_snap: HashMap<MemoryLocationHash, FfValue, BuildIdentityHasher>,
}

impl PartialRetryState {
    pub(crate) fn reset(&mut self, incarnation: TxIncarnation) {
        // Carry seen ℓ + snaps across repair incarnations (tx72-class).
        // Do not re-cold-miss locations already Bound / accessed on a prior inc.
        for &loc in self.first_k.keys() {
            self.inc_carry_seen.insert(loc);
        }
        for &loc in &self.certified {
            self.inc_carry_seen.insert(loc);
        }
        for (loc, val) in &self.value_snap {
            self.inc_carry_snap
                .entry(*loc)
                .or_insert_with(|| val.clone());
        }
        // Preserve last_final_k / last_tx_gas_used / inc_carry_* (G1 + v2 carry).
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

    /// OptimisticRead≡OCC: increment \(k\) + first_k for Detect/abort grain, **no**
    /// journal / checkpoint (those are Fenced-path PrefixSkip tax).
    pub(crate) fn note_access_k_only(&mut self, location: MemoryLocationHash) -> usize {
        self.k += 1;
        let k = self.k;
        self.first_k.entry(location).or_insert(k);
        k
    }

    /// Cheap per-tx \(k\) bump without `first_k`. Prefer `note_access_k_only`
    /// on the live SpecFence path (AccessOrdinalLog / true-\(k\) PE train).
    #[inline]
    pub(crate) fn bump_k_only(&mut self) -> usize {
        self.k += 1;
        self.k
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
        // Iter10: finalize re-notes with gas=0 must NOT touch first_k (that shifted
        // true_suffix/RebindOnly). Iter11: *plant* notes (gas>0) wait_for_dependency first_k at the
        // current effect ordinal so build_continuation keeps tip write_replays when
        // abort hits before finalize Write note_access (otherwise fk=MAX drops them
        // → cont.write_replays empty → Handler jump never arms).
        let plant_note = replay.gas_remaining_after > 0;
        let mut prior_gas = 0u64;
        self.write_replays.retain(|(l, r)| {
            if *l == location {
                prior_gas = r.gas_remaining_after;
                false
            } else {
                true
            }
        });
        if replay.gas_remaining_after == 0 {
            replay.gas_remaining_after = if prior_gas > 0 {
                prior_gas
            } else {
                self.last_post_sstore_gas()
            };
        }
        if plant_note {
            self.first_k.entry(location).or_insert(self.k);
        }
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
                        sstore_index: 0,
                        write_replays_at_tip: Vec::new(),
                        tip_sloads: Vec::new(),
                    })
                }
            });
        // Bound values for the whole certified prefix (k < k_fail), not only
        // up to cp — resume still force-binds those reads; FF cache skips MV walks.
        // Built before jump_snap select so Iter26 can prefer tip_sloads≡FF tips.
        let certified_set: HashSet<MemoryLocationHash, BuildIdentityHasher> =
            certified.iter().copied().collect();
        let mut values = HashMap::with_hasher(BuildIdentityHasher::default());
        for (loc, val) in &self.value_snap {
            let fk = self.first_k.get(loc).copied().unwrap_or(usize::MAX);
            if fk < k_fail && (fk <= cp.k || certified_set.contains(loc)) {
                values.insert(*loc, val.clone());
            }
        }
        // Iter19: among live snaps with k < k_fail, prefer OrderedAdmit/read-boundary
        // (sstore_index=0, !post_sstore) certified-prefix end — post-SSTORE plant
        // tips are usually k≥k_fail on RAW-read fails (Iter18 aj=0).
        // Iter26: among OrderedAdmit tips prefer tip_sloads≡FF (Validated-fresh / FF-path
        // captures) so refuse-if-stale can arm jump.
        // Iter27: tip≡FF = ≥1 tip_sload matches FF Storage and none conflict
        // (cumulative OrderedAdmit SLOAD log often has extras absent from certified
        // values — old all-match refused every 597 arm). Prefer steps within
        // jump_is_safe cap (not max steps — that selected steps_over tips).
        let tip_ff_status = |s: &BoundarySnapshot| -> (bool, bool) {
            if s.tip_sloads.is_empty() {
                return (false, false);
            }
            let mut any_match = false;
            let mut conflict = false;
            for (addr, slot, snap_val) in &s.tip_sloads {
                let ff = values.values().find_map(|v| match v {
                    FfValue::Storage {
                        address,
                        slot: ss,
                        value,
                        ..
                    } if address == addr && ss == slot => Some(*value),
                    _ => None,
                });
                match ff {
                    Some(v) if v == *snap_val => any_match = true,
                    Some(_) => conflict = true,
                    None => {}
                }
            }
            (any_match && !conflict, any_match)
        };
        let tip_matches_ff = |s: &BoundarySnapshot| -> bool { tip_ff_status(s).0 };
        let steps_cap = |s: &BoundarySnapshot| -> u64 {
            // Three-pillar resolve: tip≡FF/storage/write/call → 8192 (597 OrderedAdmit tips
            // at PC 2–6k were refused steps_over under 2048); Basic-only stays 128.
            let has_storage = values
                .values()
                .any(|v| matches!(v, FfValue::Storage { .. }));
            if has_storage || tip_matches_ff(s) || !s.tip_sloads.is_empty() {
                8192
            } else {
                128
            }
        };
        let (jump_snap, journal_blob) = self
            .live_boundaries
            .iter()
            .filter(|(k, (s, _))| **k < k_fail && s.is_live_capture())
            .max_by_key(|(k, (s, _))| {
                let steps = s.opcode_steps;
                let cap = steps_cap(s);
                let steps_ok = steps > 0 && steps <= cap;
                // Iter27: fewer tip_sloads → likelier first-frame apply (Lean
                // CALL_DEPTH stuck at 0; nested tips fail code_hash on frame0).
                // Iter29: keep prefer-fewer (richer tip select raised 597 wall);
                // hang-free nested consume still applies nested tips when armed.
                let tip_n = s.tip_sloads.len() as u64;
                let tip_compact = if tip_n == 0 {
                    0
                } else {
                    64u64.saturating_sub(tip_n.min(64))
                };
                (
                    u64::from(s.sstore_index == 0 && !s.post_sstore),
                    u64::from(tip_matches_ff(s)),
                    u64::from(steps_ok),
                    tip_compact,
                    if steps_ok { **k as u64 } else { 0 },
                    if steps_ok { steps } else { 0 },
                    s.sstore_index,
                    u64::from(s.post_sstore),
                )
            })
            .map(|(_, (s, b))| (Some(s.clone()), Some(b.clone())))
            .unwrap_or((None, None));
        let journal_blob = journal_blob.filter(|b| !b.is_empty());
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
        let valued_blocks_jump =
            valued_before_tip && !call_outcomes.iter().any(|c| !c.value.is_zero());
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
    pub(crate) fn attach_live_boundary(&mut self, snap: BoundarySnapshot, blob: JournalBlob) {
        self.attach_live_boundary_at(self.k, snap, blob);
    }

    /// Iter26: attach at capture-time k (TLS-deferred OrderedAdmit-snap).
    pub(crate) fn attach_live_boundary_at(
        &mut self,
        k: usize,
        snap: BoundarySnapshot,
        blob: JournalBlob,
    ) {
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
        let tx_idx = self.journal.first().map(|a| a.tx_idx).unwrap_or(0);
        self.checkpoints
            .iter()
            .rev()
            .find(|cp| cp.id.k < k_fail)
            .map(|cp| cp.id)
            .or_else(|| {
                // M1f: if OptimisticRead skipped EffectBoundary but step_end attached a
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

    /// True when every grain \(1..=cp_k\) was journaled (Fenced OrderedAdmit).
    /// OptimisticRead `note_access_k_only` leaves holes — PrefixSkip FF would be stale.
    pub(crate) fn journal_covers_prefix(&self, cp_k: usize) -> bool {
        if cp_k == 0 || self.journal.len() < cp_k {
            return false;
        }
        let mut seen = vec![false; cp_k];
        for e in &self.journal {
            if e.k >= 1 && e.k <= cp_k {
                seen[e.k - 1] = true;
            }
        }
        seen.iter().all(|&b| b)
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
    /// Per-tx rem journal. Single-executor invariant (see Sync impl).
    states: Vec<UnsafeCell<PartialRetryState>>,
    /// Locations π must OrderedAdmit/WaitHard on the next incarnation of `t`.
    force_ordered_admit: DashMap<TxIdx, Vec<MemoryLocationHash>, BuildIdentityHasher>,
    /// U4: ℓ→writer identity preserved across R2/R4 (not predicted-writer sticky).
    force_writers: DashMap<
        TxIdx,
        HashMap<MemoryLocationHash, TxIdx, BuildIdentityHasher>,
        BuildIdentityHasher,
    >,
    /// SoftWait wake consumed; next validation outcome → soft_wait_wake_{ok,reabort}.
    post_softwait_wake: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// Tx currently parked via FenceGraph SoftWait (not EarlyAbort-only park).
    softwait_parked: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// Await@a BO park armed; next validation → await_at_a_wake_{ok,reabort}.
    post_await_at_a_wake: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// Tx currently parked via access-grain Await@a (BO-until-Validated on hot ℓ).
    await_at_a_parked: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// Pending repair for next execute / Retry loop of `t`.
    repair: DashMap<TxIdx, RepairPlan, BuildIdentityHasher>,
    /// M1b journal-FF continuation armed with RewindTo.
    ff_resume: DashMap<TxIdx, ResumeContinuation, BuildIdentityHasher>,
    /// M1e: last RewindTo resume applied an absolute jump (for abort→disable).
    last_jump_applied: DashMap<TxIdx, bool, BuildIdentityHasher>,
    /// M1e: absolute jump disabled after a jumped resume failed validation (anti-livelock).
    jump_disabled: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// After force_ordered_admit_reabort: next Lean execute should open narrow inspect to
    /// capture live jump_snap (CallEntry/EffectBoundary + Storage FF path).
    needs_live_capture: DashMap<TxIdx, (), BuildIdentityHasher>,
    /// Per-tx SuffixRepair/ForceOrderedAdmit resolve depth this block. Cap → FullAbortReexecute.
    suffix_repair_depth: Vec<AtomicUsize>,
    /// Iter3: serial-barrier park count this block (cap in try_claim).
    serial_barrier_count: Vec<AtomicUsize>,
    /// Iter5: delay fb escalate once when jump_is_safe after serial capture window.
    jump_defer_count: Vec<AtomicUsize>,
    /// Iter16: validate-defer / RebindOnly-after-spine claims (cap 1/tx/block).
    validation_defer_count: Vec<AtomicUsize>,
    /// Iter5: certified-prefix FF values retained across escalate FullAbortReexecute (DB skip).
    ff_head: DashMap<
        TxIdx,
        HashMap<MemoryLocationHash, FfValue, BuildIdentityHasher>,
        BuildIdentityHasher,
    >,
}

// SAFETY: scheduler runs ≤1 executor per tx; validate after execute returns.
unsafe impl Sync for PartialRetryTable {}

impl PartialRetryTable {
    #[inline]
    unsafe fn state_mut(&self, tx_idx: TxIdx) -> &mut PartialRetryState {
        unsafe { &mut *self.states.get_unchecked(tx_idx).get() }
    }

    #[inline]
    unsafe fn state_ref(&self, tx_idx: TxIdx) -> &PartialRetryState {
        unsafe { &*self.states.get_unchecked(tx_idx).get() }
    }

    pub(crate) fn new(block_size: usize) -> Self {
        Self {
            states: (0..block_size)
                .map(|_| UnsafeCell::new(PartialRetryState::default()))
                .collect(),
            force_ordered_admit: DashMap::default(),
            force_writers: DashMap::default(),
            post_softwait_wake: DashMap::default(),
            softwait_parked: DashMap::default(),
            post_await_at_a_wake: DashMap::default(),
            await_at_a_parked: DashMap::default(),
            repair: DashMap::default(),
            ff_resume: DashMap::default(),
            last_jump_applied: DashMap::default(),
            jump_disabled: DashMap::default(),
            needs_live_capture: DashMap::default(),
            suffix_repair_depth: (0..block_size).map(|_| AtomicUsize::new(0)).collect(),
            serial_barrier_count: (0..block_size).map(|_| AtomicUsize::new(0)).collect(),
            jump_defer_count: (0..block_size).map(|_| AtomicUsize::new(0)).collect(),
            validation_defer_count: (0..block_size).map(|_| AtomicUsize::new(0)).collect(),
            ff_head: DashMap::default(),
        }
    }

    pub(crate) fn reset_incarnation(&self, tx_idx: TxIdx, incarnation: TxIncarnation) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.reset(incarnation);
        }
    }

    pub(crate) fn note_access(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        mode: AccessMode,
    ) -> usize {
        let mut st = unsafe { self.state_mut(tx_idx) };
        st.note_access(tx_idx, location, mode)
    }

    /// OptimisticRead≡OCC grain: \(k\) + first_k only (no journal / checkpoint).
    pub(crate) fn note_access_k_only(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> usize {
        let mut st = unsafe { self.state_mut(tx_idx) };
        st.note_access_k_only(location)
    }

    /// PE-probe ordinal only (no `first_k`). Safe on OptimisticRead.
    #[inline]
    pub(crate) fn bump_k_only(&self, tx_idx: TxIdx) -> usize {
        if tx_idx >= self.states.len() {
            return 0;
        }
        unsafe { self.state_mut(tx_idx) }.bump_k_only()
    }

    pub(crate) fn note_certified(&self, tx_idx: TxIdx, location: MemoryLocationHash) {
        unsafe { self.state_mut(tx_idx) }.note_certified(location);
    }

    /// OrderedAdmit-on-Data lite: journal Read access + certify + lightweight EffectBoundary
    /// checkpoint under **one** rem lock (no BoundarySnapshot alloc).
    pub(crate) fn note_access_certified_checkpoint(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            let st = unsafe { self.state_mut(tx_idx) };
            st.note_access(tx_idx, location, AccessMode::Read);
            st.note_certified(location);
            let _ = st.push_checkpoint(tx_idx, CheckpointKind::EffectBoundary);
        }
    }

    /// OrderedAdmit-on-Data lite (legacy): certify + lightweight EffectBoundary under one lock.
    pub(crate) fn note_certified_with_effect_boundary(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            let st = unsafe { self.state_mut(tx_idx) };
            st.note_certified(location);
            let _ = st.push_checkpoint(tx_idx, CheckpointKind::EffectBoundary);
        }
    }

    pub(crate) fn note_value(&self, tx_idx: TxIdx, location: MemoryLocationHash, value: FfValue) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.note_value(location, value);
        }
    }

    /// Live incarnation value snap (for value-stable RebindOnly at validation).
    pub(crate) fn snapped_value(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> Option<FfValue> {
        (tx_idx < self.states.len()).then(|| ()).and_then(|_| {
            // SAFETY: single-executor invariant
            unsafe { self.state_ref(tx_idx) }
                .value_snap
                .get(&location)
                .cloned()
        })
    }

    /// Value-stable check without cloning FfValue out of the snap map.
    /// Storage: exact U256. Basic: balance+nonce (code excluded — Lazy/Estimate → false).
    pub(crate) fn value_stable_match(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        cur: &crate::MemoryValue,
    ) -> bool {
        if tx_idx >= self.states.len() {
            return false;
        }
        // SAFETY: single-executor invariant
        let st = unsafe { self.state_ref(tx_idx) };
        let snap = st
            .value_snap
            .get(&location)
            .or_else(|| st.inc_carry_snap.get(&location));
        let Some(snap) = snap else {
            return false;
        };
        match (snap, cur) {
            (FfValue::Storage { value, .. }, crate::MemoryValue::Storage(v)) => value == v,
            (FfValue::Basic { basic, .. }, crate::MemoryValue::Basic(b)) => {
                basic.balance == b.balance && basic.nonce == b.nonce
            }
            _ => false,
        }
    }

    /// ℓ already seen on a prior incarnation of this tx (residual OrderedAdmit, not OptimisticReadCold).
    #[inline]
    pub(crate) fn inc_carry_seen(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> bool {
        if tx_idx >= self.states.len() {
            return false;
        }
        // SAFETY: single-executor invariant
        unsafe { self.state_ref(tx_idx) }
            .inc_carry_seen
            .contains(&location)
    }

    /// M1i: Inspector post-SSTORE gas capture for write-prefix jump gas-equality.
    pub(crate) fn note_post_sstore_gas(&self, tx_idx: TxIdx, gas_remaining_after: u64) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.note_post_sstore_gas(gas_remaining_after);
        }
    }

    /// M1h/M1i: record storage write present/original for absolute-jump journal replay.
    pub(crate) fn note_write_replay(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        replay: StorageWriteReplay,
    ) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.note_write_replay(location, replay);
        }
    }

    /// M1j: record LOG* events for absolute-jump past LOG (hang-free vs blob path).
    pub(crate) fn note_log_replays(&self, tx_idx: TxIdx, logs: Vec<LogReplay>) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.note_log_replays(logs);
        }
    }

    /// Ordered write_replay values for Handler tip embedding (Iter9).
    pub(crate) fn write_replay_values(&self, tx_idx: TxIdx) -> Vec<StorageWriteReplay> {
        if tx_idx >= self.states.len() {
            return Vec::new();
        }
        // SAFETY: single-executor invariant
        unsafe {
            self.state_ref(tx_idx)
                .write_replays
                .iter()
                .map(|(_, r)| r.clone())
                .collect()
        }
    }

    /// Locations with flushed write_replays in the live incarnation journal.
    pub(crate) fn write_replay_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        if tx_idx >= self.states.len() {
            return Vec::new();
        }
        // SAFETY: single-executor invariant
        unsafe {
            self.state_ref(tx_idx)
                .write_replays
                .iter()
                .map(|(l, _)| *l)
                .collect()
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
        prefix_writes: Vec<MemoryLocationHash>,
    ) {
        let cont = unsafe { self.state_mut(tx_idx) }.build_continuation(
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
    /// live-Interpreter park). Otherwise return [`ParkResumeKind::FullAbortReexecute`]
    /// (tx-grain head reexec — M2 behaviour). Absolute PC jump remains gated by
    /// M1e/M1l safety on the resume path — this only arms journal FF + force-ordered_admit.
    pub(crate) fn try_arm_park_resume_at_k(
        &self,
        tx_idx: TxIdx,
        armed_at_k: u64,
    ) -> ParkResumeKind {
        self.try_arm_park_resume_at_k_inner(tx_idx, armed_at_k, true)
    }

    fn try_arm_park_resume_at_k_inner(
        &self,
        tx_idx: TxIdx,
        armed_at_k: u64,
        force_ordered_admit: bool,
    ) -> ParkResumeKind {
        let k_fail = armed_at_k as usize;
        if k_fail == 0 {
            self.repair.insert(tx_idx, RepairPlan::FullAbortReexecute);
            return ParkResumeKind::FullAbortReexecute;
        }
        let Some(cp) = self.last_checkpoint_before(tx_idx, k_fail) else {
            self.repair.insert(tx_idx, RepairPlan::FullAbortReexecute);
            return ParkResumeKind::FullAbortReexecute;
        };
        // Require real mid-tx progress — synthetic CallEntry at k=0 alone is FullAbortReexecute.
        if cp.k == 0 || cp.k >= k_fail {
            self.repair.insert(tx_idx, RepairPlan::FullAbortReexecute);
            return ParkResumeKind::FullAbortReexecute;
        }

        let st = unsafe { self.state_mut(tx_idx) };
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
            self.repair.insert(tx_idx, RepairPlan::FullAbortReexecute);
            return ParkResumeKind::FullAbortReexecute;
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
        if force_ordered_admit {
            self.set_force_ordered_admit(tx_idx, certified);
        }
        ParkResumeKind::ResumeAtK { checkpoint_k: cp.k }
    }

    /// WaitForDependency wake: RewindTo + FF **without** force-ordered_admit (OrderedAdmit-theater on a
    /// Done producer is the 14689597 abort class). SoftWait still uses
    /// [`Self::try_arm_park_resume_at_k`].
    pub(crate) fn try_arm_wait_for_dependency_resume_at_k(
        &self,
        tx_idx: TxIdx,
        armed_at_k: u64,
    ) -> ParkResumeKind {
        self.try_arm_park_resume_at_k_inner(tx_idx, armed_at_k, false)
    }

    /// wait_for_dependency: plant rem checkpoint from **snapped** prefix reads
    /// so wake [`Self::try_arm_wait_for_dependency_resume_at_k`] can ResumeAtK.
    ///
    /// Empty first-access (no `value_snap`) stays FullAbortReexecute — planting a
    /// synthetic k=1 grain livelocks 19807137 (WaitForDependency→RewindTo→WaitForDependency) and pays
    /// rem tax ≫ OCC full_abort_reexecute. Wait loc is **not** prefix-certified. Soft=0.
    ///
    /// Returns `armed_at_k` (`> checkpoint k`), or 0 if no honest prefix.
    pub(crate) fn arm_wait_for_dependency_checkpoint(
        &self,
        tx_idx: TxIdx,
        wait_loc: MemoryLocationHash,
        access_k: u32,
        prefix: &[(MemoryLocationHash, u32)],
    ) -> u64 {
        if tx_idx >= self.states.len() {
            return 0;
        }
        // SAFETY: single-executor invariant (waiter still owns this incarnation).
        let st = unsafe { self.state_mut(tx_idx) };
        for &(loc, k) in prefix {
            if k == 0 || loc == wait_loc {
                continue;
            }
            if !st.value_snap.contains_key(&loc) && !st.inc_carry_snap.contains_key(&loc) {
                continue;
            }
            let kk = k as usize;
            if st.first_k.get(&loc).is_none() {
                st.journal.push(RegionAccess {
                    tx_idx,
                    k: kk,
                    location: loc,
                    mode: AccessMode::Read,
                });
                st.first_k.insert(loc, kk);
                st.certified.insert(loc);
            }
            if kk > st.k {
                st.k = kk;
            }
        }
        if st.k == 0 {
            return 0;
        }
        // Tiny prefix ResumeAtK pays rem tax ≫ OCC FullAbortReexecute (14689597
        // 0.17 / 19807137 0.09 with 300–1200 resume_k). Cash only when
        // prefix skip is real (same bar as `prefix_skip_beats_full_abort`).
        if st.k < 8 {
            return 0;
        }
        let has_mid_cp = st.checkpoints.iter().any(|c| c.id.k > 0 && c.id.k <= st.k);
        if !has_mid_cp {
            let _ = st.push_checkpoint(tx_idx, CheckpointKind::EffectBoundary);
        }
        // Wait loc is the fail grain — not prefix-certified.
        let _ = wait_loc;
        st.k.saturating_add(1).max(access_k as usize) as u64
    }

    /// PartialAbortRewind timely Resolve: arm RewindTo when a mid-tx checkpoint exists.
    ///
    /// Unlike [`Self::apply_suffix_repair`], this does **not** require
    /// `prefix_skip_beats_full_abort` (cp_k≥8) — that gate made cert-covered partial_abort theater
    /// (attempt then OCC full_abort_reexecute). SoftWait Soft stays 0.
    pub(crate) fn try_arm_partial_abort_rewind(
        &self,
        tx_idx: TxIdx,
        read_locations: &[MemoryLocationHash],
        invalid: &[MemoryLocationHash],
        write_locations: &[MemoryLocationHash],
    ) -> Option<LeanAbortRepair> {
        // One RewindTo per tx. A second strip-cover without progress is the
        // 19807137 livelock (never-full_abort_reexecute ForceOrderedAdmit/PartialAbortRewind train).
        if self.suffix_repair_depth(tx_idx) != 0 {
            return None;
        }
        let plan = self.plan_partial_retry(tx_idx, read_locations, invalid, write_locations)?;
        if plan.certified.is_empty() {
            return None;
        }
        let k_fail = plan.k_fail;
        if k_fail == 0 {
            return None;
        }
        let cp = self.last_checkpoint_before(tx_idx, k_fail)?;
        if cp.k == 0 || cp.k >= k_fail {
            return None;
        }
        self.arm_rewind_to(
            tx_idx,
            cp,
            k_fail,
            plan.certified.clone(),
            plan.suffix_writes.clone(),
            plan.prefix_writes.clone(),
        );
        self.set_force_ordered_admit(tx_idx, plan.certified.clone());
        self.note_suffix_repair(tx_idx);
        Some(LeanAbortRepair::SuffixRepair {
            certified: plan.certified,
            suffix_writes: plan.suffix_writes,
            reexec_cost: 0.6,
        })
    }

    /// After `reset_incarnation`, replay FF continuation into the fresh journal.
    /// Returns effects replayed (0 if none).
    pub(crate) fn replay_ff_if_armed(&self, tx_idx: TxIdx) -> usize {
        let Some(cont) = self.ff_resume.get(&tx_idx).map(|c| c.clone()) else {
            return 0;
        };
        unsafe { self.state_mut(tx_idx) }.replay_continuation(&cont)
    }

    pub(crate) fn ff_value(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> Option<FfValue> {
        if let Some(v) = self
            .ff_resume
            .get(&tx_idx)
            .and_then(|c| c.values.get(&location).cloned())
        {
            return Some(v);
        }
        // Iter5: head-FF after escalate FullAbortReexecute (origin-checked in try_ff_*).
        self.ff_head
            .get(&tx_idx)
            .and_then(|m| m.get(&location).cloned())
    }

    /// True when escalate retained certified-prefix FF for head reexec DB skip.
    pub(crate) fn has_ff_head(&self, tx_idx: TxIdx) -> bool {
        self.ff_head.get(&tx_idx).is_some_and(|m| !m.is_empty())
    }

    /// Iter6: armed SuffixRepair FF continuation still has values (cheap resume).
    pub(crate) fn has_ff_resume_values(&self, tx_idx: TxIdx) -> bool {
        self.ff_resume
            .get(&tx_idx)
            .is_some_and(|c| !c.values.is_empty())
    }

    pub(crate) fn clear_ff_head(&self, tx_idx: TxIdx) {
        self.ff_head.remove(&tx_idx);
    }

    pub(crate) fn ff_entries(&self, tx_idx: TxIdx) -> usize {
        self.ff_resume
            .get(&tx_idx)
            .map(|c| c.effects.len().max(c.values.len()))
            .unwrap_or(0)
    }

    pub(crate) fn clear_ff(&self, tx_idx: TxIdx) {
        self.ff_resume.remove(&tx_idx);
        self.ff_head.remove(&tx_idx);
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
        (tx_idx < self.states.len()).then(|| {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.push_checkpoint_with_boundary(tx_idx, kind, boundary)
        })
    }

    /// Boundary snap attached to the rewind target checkpoint, if any.
    pub(crate) fn ff_boundary(&self, tx_idx: TxIdx) -> Option<BoundarySnapshot> {
        self.ff_resume.get(&tx_idx).and_then(|c| c.boundary.clone())
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
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.attach_live_boundary(snap, blob);
        }
    }

    /// Iter26: attach OrderedAdmit-snap at capture-time effect ordinal.
    pub(crate) fn attach_live_boundary_at(
        &self,
        tx_idx: TxIdx,
        k: usize,
        snap: BoundarySnapshot,
        blob: JournalBlob,
    ) {
        if tx_idx < self.states.len() {
            unsafe { self.state_mut(tx_idx) }.attach_live_boundary_at(k, snap, blob);
        }
    }

    /// M1g: persist nested CallOutcomes from Inspector capture into tx state.
    /// Also patch an already-armed RewindTo continuation (EarlyVal may arm mid-run
    /// before `with_plant_tls` ends and flushes captures).
    pub(crate) fn note_call_outcomes(&self, tx_idx: TxIdx, calls: Vec<CachedCallOutcome>) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.note_call_outcomes(calls.clone());
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

    pub(crate) fn ff_values(&self, tx_idx: TxIdx) -> Vec<(MemoryLocationHash, FfValue)> {
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
        if tx_idx >= self.states.len() {
            return None;
        }
        // SAFETY: single-executor invariant
        unsafe { self.state_ref(tx_idx).last_checkpoint_before(k_fail) }
    }

    pub(crate) fn current_k(&self, tx_idx: TxIdx) -> usize {
        self.states
            .get(tx_idx)
            .map(|_| unsafe { self.state_ref(tx_idx).current_k() })
            .unwrap_or(0)
    }

    pub(crate) fn journal_covers_prefix(&self, tx_idx: TxIdx, cp_k: usize) -> bool {
        if tx_idx >= self.states.len() {
            return false;
        }
        unsafe { self.state_ref(tx_idx).journal_covers_prefix(cp_k) }
    }

    /// G1: cheap effect-progress depth proxy for π (None on first incarnation).
    pub(crate) fn estimate_effect_depth(&self, tx_idx: TxIdx) -> Option<f64> {
        self.states
            .get(tx_idx)
            .and_then(|_| unsafe { self.state_ref(tx_idx).estimate_effect_depth() })
    }

    /// G1: record finished incarnation gas + k for next-try depth proxy.
    pub(crate) fn note_incarnation_finish(&self, tx_idx: TxIdx, tx_gas_used: u64) {
        if tx_idx < self.states.len() {
            // SAFETY: single-executor invariant
            unsafe { self.state_mut(tx_idx) }.note_incarnation_finish(tx_gas_used);
        }
    }

    pub(crate) fn first_k(&self, tx_idx: TxIdx, location: MemoryLocationHash) -> Option<usize> {
        unsafe { self.state_mut(tx_idx) }.first_k(location)
    }

    /// Locations π should force OrderedAdmit/WaitHard for this incarnation (from prior repair).
    pub(crate) fn force_ordered_admit_locations(&self, tx_idx: TxIdx) -> Vec<MemoryLocationHash> {
        self.force_ordered_admit
            .get(&tx_idx)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    pub(crate) fn must_force_ordered_admit(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> bool {
        self.force_ordered_admit
            .get(&tx_idx)
            .is_some_and(|v| v.iter().any(|l| *l == location))
    }

    pub(crate) fn set_force_ordered_admit(
        &self,
        tx_idx: TxIdx,
        locations: Vec<MemoryLocationHash>,
    ) {
        if locations.is_empty() {
            self.force_ordered_admit.remove(&tx_idx);
        } else {
            self.force_ordered_admit.insert(tx_idx, locations);
        }
    }

    pub(crate) fn clear_force_ordered_admit(&self, tx_idx: TxIdx) {
        self.force_ordered_admit.remove(&tx_idx);
    }

    /// U4: record the observed writer of ℓ for the next incarnation.
    pub(crate) fn note_force_writer(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        writer: TxIdx,
    ) {
        if writer >= tx_idx {
            return;
        }
        self.force_writers
            .entry(tx_idx)
            .or_default()
            .insert(location, writer);
    }

    /// U1/U4: resolvable writer id carried on force_prefix / repair.
    pub(crate) fn force_writer(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
    ) -> Option<TxIdx> {
        self.force_writers
            .get(&tx_idx)
            .and_then(|m| m.get(&location).copied())
            .filter(|&w| w < tx_idx)
    }

    pub(crate) fn clear_force_writers(&self, tx_idx: TxIdx) {
        self.force_writers.remove(&tx_idx);
    }

    /// U4 identity is a wall: every invalid ℓ still names a lower writer.
    pub(crate) fn identity_held(&self, tx_idx: TxIdx, invalid: &[MemoryLocationHash]) -> bool {
        !invalid.is_empty()
            && invalid
                .iter()
                .all(|&loc| self.force_writer(tx_idx, loc).is_some())
    }

    /// Value-stable via snap **or** certified-prefix FF (thin identity → R1).
    pub(crate) fn identity_stable_match(
        &self,
        tx_idx: TxIdx,
        location: MemoryLocationHash,
        cur: &crate::MemoryValue,
    ) -> bool {
        if self.value_stable_match(tx_idx, location, cur) {
            return true;
        }
        let Some(ff) = self.ff_value(tx_idx, location) else {
            return false;
        };
        match (ff, cur) {
            (FfValue::Storage { value, .. }, crate::MemoryValue::Storage(v)) => value == *v,
            (FfValue::Basic { basic, .. }, crate::MemoryValue::Basic(b)) => {
                basic.balance == b.balance && basic.nonce == b.nonce
            }
            _ => false,
        }
    }

    /// True when a certified-prefix force_ordered_admit set is armed for this tx.
    pub(crate) fn has_force_ordered_admit(&self, tx_idx: TxIdx) -> bool {
        self.force_ordered_admit
            .get(&tx_idx)
            .is_some_and(|v| !v.is_empty())
    }

    /// Sticky resolve: union conflict locations into the armed force_ordered_admit set.
    pub(crate) fn extend_force_ordered_admit(
        &self,
        tx_idx: TxIdx,
        locations: &[MemoryLocationHash],
    ) {
        if locations.is_empty() {
            return;
        }
        let mut merged = self
            .force_ordered_admit
            .get(&tx_idx)
            .map(|v| v.clone())
            .unwrap_or_default();
        for &loc in locations {
            if !merged.contains(&loc) {
                merged.push(loc);
            }
        }
        self.set_force_ordered_admit(tx_idx, merged);
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

    pub(crate) fn mark_await_at_a_parked(&self, tx_idx: TxIdx) {
        self.await_at_a_parked.insert(tx_idx, ());
    }

    pub(crate) fn take_await_at_a_parked(&self, tx_idx: TxIdx) -> bool {
        self.await_at_a_parked.remove(&tx_idx).is_some()
    }

    pub(crate) fn mark_post_await_at_a_wake(&self, tx_idx: TxIdx) {
        self.post_await_at_a_wake.insert(tx_idx, ());
    }

    pub(crate) fn take_post_await_at_a_wake(&self, tx_idx: TxIdx) -> bool {
        self.post_await_at_a_wake.remove(&tx_idx).is_some()
    }

    /// Count of SuffixRepair/ForceOrderedAdmit resolves armed for `tx` this block.
    #[inline]
    pub(crate) fn suffix_repair_depth(&self, tx_idx: TxIdx) -> usize {
        self.suffix_repair_depth
            .get(tx_idx)
            .map(|a| a.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Note one SuffixRepair / ForceOrderedAdmit arm (depth for escalate-after-N).
    #[inline]
    pub(crate) fn note_suffix_repair(&self, tx_idx: TxIdx) {
        if let Some(a) = self.suffix_repair_depth.get(tx_idx) {
            a.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Clear repair depth (success validate or escalate FullAbortReexecute).
    #[inline]
    pub(crate) fn clear_suffix_repair_depth(&self, tx_idx: TxIdx) {
        if let Some(a) = self.suffix_repair_depth.get(tx_idx) {
            a.store(0, Ordering::Relaxed);
        }
    }

    /// Iter3: claim a serial-barrier park (cap 1/tx/block — anti-cascade).
    pub(crate) fn try_claim_serial_barrier(&self, tx_idx: TxIdx) -> bool {
        const MAX_BARRIERS: usize = 1;
        let Some(a) = self.serial_barrier_count.get(tx_idx) else {
            return false;
        };
        let cur = a.load(Ordering::Relaxed);
        if cur >= MAX_BARRIERS {
            return false;
        }
        a.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// True when this tx has exhausted its serial-barrier budget.
    pub(crate) fn serial_barrier_used(&self, tx_idx: TxIdx) -> bool {
        const MAX_BARRIERS: usize = 1;
        self.serial_barrier_count
            .get(tx_idx)
            .is_some_and(|a| a.load(Ordering::Relaxed) >= MAX_BARRIERS)
    }

    /// Iter5: claim one fb-escalate defer for hang-free absolute jump after capture.
    /// Cap 1/tx/block — anti-storm (Iter2 blind defer hung under concurrency).
    pub(crate) fn try_claim_jump_defer(&self, tx_idx: TxIdx) -> bool {
        const MAX_DEFER: usize = 1;
        let Some(a) = self.jump_defer_count.get(tx_idx) else {
            return false;
        };
        let cur = a.load(Ordering::Relaxed);
        if cur >= MAX_DEFER {
            return false;
        }
        a.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Iter16: claim one validate-defer (RebindOnly-after-spine) per tx/block.
    pub(crate) fn try_claim_validation_defer(&self, tx_idx: TxIdx) -> bool {
        const MAX_DEFER: usize = 1;
        let Some(a) = self.validation_defer_count.get(tx_idx) else {
            return false;
        };
        let cur = a.load(Ordering::Relaxed);
        if cur >= MAX_DEFER {
            return false;
        }
        a.fetch_add(1, Ordering::Relaxed);
        true
    }

    pub(crate) fn escalate_full_abort_reexecute(&self, tx_idx: TxIdx) -> LeanAbortRepair {
        // Iter5 write-prefix skip: retain **armed continuation** FF values only for
        // head reexec DB skip (origin-checked in try_ff_*). Do **not** merge live
        // value_snap — that included post-fail reads and caused seq≠par / outliers.
        if let Some(cont) = self.ff_resume.get(&tx_idx) {
            if !cont.values.is_empty() {
                self.ff_head.insert(tx_idx, cont.values.clone());
            }
        }
        self.clear_force_ordered_admit(tx_idx);
        // U4: keep ℓ→writer identity through R4 FullAbortReexecute. force_prefix may
        // be cleared; the next incarnation still resolves the same writer.
        self.clear_repair(tx_idx);
        self.clear_suffix_repair_depth(tx_idx);
        self.needs_live_capture.remove(&tx_idx);
        LeanAbortRepair::FullAbortReexecute { reexec_cost: 2.2 }
    }

    /// Frozen-grain Resolve: **PartialAbortRebind RebindThis** at validate (`try_validate`);
    /// this arms **PartialAbortRewind CertifiedPrefixSkip** only when a mid-tx checkpoint
    /// exists. No checkpoint / grain identity lost → **full_abort_reexecute FullAbortReexecute**
    /// (OCC residual). SuffixRepair / ForcePrefix is **not** the default.
    ///
    /// ```text
    /// if plan_partial_retry + checkpoint with 0 < cp.k < k_fail:
    ///   arm_rewind_to + journal FF  → CertifiedPrefixSkip (SuffixRepair rust name)
    /// else:
    ///   clear_force_ordered_admit; clear_repair → full_abort_reexecute FullAbortReexecute
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
        let plan = self.plan_partial_retry(tx_idx, read_locations, invalid, write_locations);
        self.apply_suffix_repair_planned(tx_idx, plan)
    }

    /// Certified PrefixSkip cheaper than OCC full_abort_reexecute reincarnation?
    /// Tiny rewind (1–2 accesses) pays FF/repair tax ≫ saved work (19807137).
    /// First repair only; substantial prefix (≥8) covering ≥ half the grain.
    #[inline]
    pub(crate) fn prefix_skip_beats_full_abort(
        cp_k: usize,
        k_fail: usize,
        repair_depth: usize,
    ) -> bool {
        repair_depth == 0 && cp_k >= 8 && k_fail > cp_k + 2 && cp_k.saturating_mul(2) >= k_fail
    }

    /// SuffixRepair using a precomputed [`PartialRetryPlan`] (avoids double plan).
    pub(crate) fn apply_suffix_repair_planned(
        &self,
        tx_idx: TxIdx,
        plan: Option<PartialRetryPlan>,
    ) -> LeanAbortRepair {
        match plan {
            Some(plan) if !plan.certified.is_empty() => {
                let k_fail = plan.k_fail;
                // Certified prefix skip **only** when cheaper than OCC full_abort_reexecute.
                // Tiny PrefixSkip / rewind-on-every-fail loses to reincarnation
                // (19807137 SuffixRepair makespan). Default is full_abort_reexecute.
                if k_fail > 0 {
                    if let Some(cp) = self.last_checkpoint_before(tx_idx, k_fail) {
                        if Self::prefix_skip_beats_full_abort(
                            cp.k,
                            k_fail,
                            self.suffix_repair_depth(tx_idx),
                        ) && self.journal_covers_prefix(tx_idx, cp.k)
                        {
                            self.arm_rewind_to(
                                tx_idx,
                                cp,
                                k_fail,
                                plan.certified.clone(),
                                plan.suffix_writes.clone(),
                                plan.prefix_writes.clone(),
                            );
                            self.set_force_ordered_admit(tx_idx, plan.certified.clone());
                            return LeanAbortRepair::SuffixRepair {
                                certified: plan.certified,
                                suffix_writes: plan.suffix_writes,
                                reexec_cost: 0.6,
                            };
                        }
                    }
                }
                // No cheap certified skip → full_abort_reexecute residual reincarnation
                // (OCC-identical). ForceOrderedAdmit / SuffixRepair is not the default.
                self.clear_force_ordered_admit(tx_idx);
                self.clear_repair(tx_idx);
                LeanAbortRepair::FullAbortReexecute { reexec_cost: 2.2 }
            }
            _ => {
                self.clear_force_ordered_admit(tx_idx);
                self.clear_repair(tx_idx);
                LeanAbortRepair::FullAbortReexecute { reexec_cost: 2.2 }
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
    /// RewindTo + journal FF + force-ordered_admit when a checkpoint exists; otherwise
    /// clears to FullAbortReexecute. Does **not** enable absolute PC jump or valued
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
                    self.set_force_ordered_admit(tx_idx, certified.clone());
                    ResearchAbortRepair::RewindTo {
                        certified,
                        suffix_writes,
                        reexec_cost: 0.6,
                    }
                }
                RepairPlan::RebindOnly { .. } | RepairPlan::FullAbortReexecute => {
                    ResearchAbortRepair::FullAbortReexecute { reexec_cost: 2.2 }
                }
            },
            None => ResearchAbortRepair::FullAbortReexecute { reexec_cost: 2.2 },
        }
    }

    /// P3 EarlyAbort: arm rem repair for the next incarnation after cutting at `fail_location`.
    ///
    /// Mirrors EarlyVal-fail path: RewindTo + journal FF when a checkpoint exists,
    /// else FullAbortReexecute; always `set_force_ordered_admit` on the certified prefix so the
    /// reincarnation OrderedAdmit/WaitHards instead of OptimisticReading the same early cross.
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
        let cp = self
            .last_checkpoint_before(tx_idx, k_fail)
            .unwrap_or(CheckpointId {
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
            // No certified checkpoint → FullAbortReexecute from tx head on next incarnation.
            self.set_repair(tx_idx, RepairPlan::FullAbortReexecute);
        }
        self.set_force_ordered_admit(tx_idx, certified);
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

    /// Mark tx for one Lean inspect capture (live jump_snap) after SuffixRepair
    /// with Storage write_replays (Iter2 hang-free prime — not bare force_ordered_admit).
    pub(crate) fn mark_needs_live_capture(&self, tx_idx: TxIdx) {
        self.needs_live_capture.insert(tx_idx, ());
    }

    /// Take live-capture prime flag (one-shot).
    pub(crate) fn take_needs_live_capture(&self, tx_idx: TxIdx) -> bool {
        self.needs_live_capture.remove(&tx_idx).is_some()
    }

    pub(crate) fn needs_live_capture(&self, tx_idx: TxIdx) -> bool {
        self.needs_live_capture.contains_key(&tx_idx)
    }

    /// True when current incarnation has at least one live Inspector snap
    /// (Iter2: delay fb escalate so next SuffixRepair can absolute-jump).
    pub(crate) fn has_live_boundary(&self, tx_idx: TxIdx) -> bool {
        if tx_idx >= self.states.len() {
            return false;
        }
        // SAFETY: single-executor invariant
        unsafe { self.state_ref(tx_idx) }
            .live_boundaries
            .values()
            .any(|(snap, _)| snap.is_live_capture())
    }

    /// Iter2: true when planning SuffixRepair *now* would yield a `jump_is_safe`
    /// continuation (live snap + Storage/read-only or write_replay gates). Used to
    /// delay fb escalate only when the next resume can actually absolute-jump.
    pub(crate) fn preview_next_jump_safe(
        &self,
        tx_idx: TxIdx,
        plan: &Option<PartialRetryPlan>,
    ) -> bool {
        self.preview_next_jump_safe_why(tx_idx, plan).0
    }

    /// Like [`preview_next_jump_safe`] but returns `(ok, reason)` for Iter2 debug.
    pub(crate) fn preview_next_jump_safe_why(
        &self,
        tx_idx: TxIdx,
        plan: &Option<PartialRetryPlan>,
    ) -> (bool, &'static str) {
        if self.is_jump_disabled(tx_idx) {
            return (false, "jump_disabled");
        }
        if !self.has_live_boundary(tx_idx) {
            return (false, "no_live");
        }
        let Some(plan) = plan.as_ref() else {
            return (false, "no_plan");
        };
        if plan.certified.is_empty() || plan.k_fail == 0 {
            return (false, "bad_plan");
        }
        let Some(cp) = self.last_checkpoint_before(tx_idx, plan.k_fail) else {
            return (false, "no_cp");
        };
        if !(cp.k > 0 && cp.k < plan.k_fail) {
            return (false, "cp_k");
        }
        let cont = unsafe { self.state_ref(tx_idx) }.build_continuation(
            cp,
            plan.k_fail,
            plan.certified.clone(),
            plan.suffix_writes.clone(),
            plan.prefix_writes.clone(),
        );
        if !super::boundary::jump_is_safe(&cont) {
            let why = match cont.jump_snap.as_ref() {
                None => "snap_none",
                Some(s) if !s.is_live_capture() => "snap_not_live",
                Some(s) if s.opcode_steps == 0 || s.opcode_steps > 8192 => "steps",
                Some(s) if s.call_depth > 2 => "depth",
                Some(s) if s.bytecode_len > 4096 => "bytecode",
                _ if cont.valued_blocks_jump => "valued_blocks",
                _ if cont.effects.is_empty() => "no_effects",
                _ if cont.effects.iter().any(|e| e.mode == AccessMode::Write)
                    && cont.write_replays.is_empty() =>
                {
                    "write_no_replay"
                }
                _ if !cont.call_outcomes.is_empty()
                    && cont.jump_snap.as_ref().is_some_and(|s| !s.at_call_boundary) =>
                {
                    "need_call_boundary"
                }
                _ => "jump_gate",
            };
            return (false, why);
        }
        (true, "ok")
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

    /// Classify validation failure into PartialRetry plan, or `None` if unsafe → FullAbortReexecute.
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
        let st = unsafe { self.state_mut(tx_idx) };
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
    /// when a checkpoint exists; `FullAbortReexecute` only if prefix/control-flow
    /// cannot be recovered.
    pub(crate) fn plan_repair(&self, tx_idx: TxIdx, plan: &PartialRetryPlan) -> RepairPlan {
        if plan.certified.is_empty() {
            return RepairPlan::FullAbortReexecute;
        }
        match self.last_checkpoint_before(tx_idx, plan.k_fail) {
            Some(cp) => RepairPlan::RewindTo {
                cp,
                certified: plan.certified.clone(),
                k_fail: plan.k_fail,
                suffix_writes: plan.suffix_writes.clone(),
            },
            None => RepairPlan::FullAbortReexecute,
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
    fn try_arm_park_resume_falls_back_when_k_zero_or_no_cp() {
        let table = PartialRetryTable::new(4);
        table.reset_incarnation(1, 0);
        // k=0 → FullAbortReexecute
        assert_eq!(
            table.try_arm_park_resume_at_k(1, 0),
            ParkResumeKind::FullAbortReexecute
        );
        // Journaled observe but only synthetic path with no mid-tx cp > 0
        table.note_access(1, 10, AccessMode::Read);
        // CallEntry-style cp at k after note would be at current k; push at k=1
        let _ = table.push_checkpoint(1, CheckpointKind::CallEntry);
        // armed_at_k == cp.k → need cp.k < armed_at_k; arm at same k → FullAbortReexecute
        assert_eq!(
            table.try_arm_park_resume_at_k(1, 1),
            ParkResumeKind::FullAbortReexecute
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
            unsafe { &mut *table.states[2].get() }.note_value(
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
        assert!(table.must_force_ordered_admit(2, 1));
        assert!(table.must_force_ordered_admit(2, 2));
    }

    #[test]
    fn arm_wait_for_dependency_checkpoint_makes_product_park_resume() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        // Snapped prefix (maybe_note_value) — honest rem grain. k≥8 so
        // ResumeAtK is cheaper than FullAbortReexecute.
        let prefix: Vec<(u64, u32)> = (1..=8).map(|i| (10 + i as u64, i)).collect();
        for &(loc, i) in &prefix {
            unsafe { &mut *table.states[0].get() }.note_value(
                loc,
                FfValue::Storage {
                    address: Address::ZERO,
                    slot: U256::from(i),
                    value: U256::from(i),
                    origin: None,
                },
            );
        }
        let armed = table.arm_wait_for_dependency_checkpoint(0, 99, 9, &prefix);
        assert!(armed > 8, "armed_at_k must exceed prefix checkpoint");
        match table.try_arm_wait_for_dependency_resume_at_k(0, armed) {
            ParkResumeKind::ResumeAtK { checkpoint_k } => {
                assert!(checkpoint_k > 0 && checkpoint_k < armed as usize);
            }
            other => panic!("WaitForDependency must ResumeAtK, got {other:?}"),
        }
        assert!(table.is_rewind_resume(0));
        assert!(
            !table.must_force_ordered_admit(0, 10),
            "WaitForDependency resume must not force-ordered_admit"
        );
    }

    #[test]
    fn arm_wait_for_dependency_tiny_snapped_prefix_is_full_abort_reexecute() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        unsafe { &mut *table.states[0].get() }.note_value(
            10,
            FfValue::Storage {
                address: Address::ZERO,
                slot: U256::from(1),
                value: U256::from(1),
                origin: None,
            },
        );
        let armed = table.arm_wait_for_dependency_checkpoint(0, 99, 2, &[(10, 1)]);
        assert_eq!(armed, 0, "tiny prefix must not ResumeAtK");
    }

    #[test]
    fn arm_wait_for_dependency_first_access_is_full_abort_reexecute() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        let armed = table.arm_wait_for_dependency_checkpoint(0, 7, 1, &[]);
        assert_eq!(armed, 0, "no snapped prefix → no synthetic checkpoint");
        assert_eq!(
            table.try_arm_wait_for_dependency_resume_at_k(0, armed),
            ParkResumeKind::FullAbortReexecute
        );
    }

    #[test]
    fn try_arm_partial_abort_rewind_without_cp_k8_gate() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        table.note_access(0, 10, AccessMode::Read);
        table.note_certified(0, 10);
        table.note_access(0, 11, AccessMode::Read);
        table.note_certified(0, 11);
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 20, AccessMode::Read);
        match table.apply_suffix_repair(0, &[10, 11, 20], &[20], &[]) {
            LeanAbortRepair::FullAbortReexecute { .. } => {}
            other => panic!(
                "cp_k<8 must stay full_abort_reexecute on apply_suffix_repair, got {other:?}"
            ),
        }
        match table.try_arm_partial_abort_rewind(0, &[10, 11, 20], &[20], &[]) {
            Some(LeanAbortRepair::SuffixRepair { .. }) => {}
            other => {
                panic!("PartialAbortRewind covered must RewindTo without cp_k≥8, got {other:?}")
            }
        }
        assert!(table.is_rewind_resume(0));
        assert!(
            table
                .try_arm_partial_abort_rewind(0, &[10, 11, 20], &[20], &[])
                .is_none(),
            "second PartialAbortRewind must escalate (no RewindTo train)"
        );
    }
}

#[cfg(test)]
mod p3_early_abort_tests {
    use super::*;

    #[test]
    fn arm_early_abort_full_abort_reexecute_without_cp() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        table.note_access(0, 7, AccessMode::Read);
        table.arm_early_abort(0, 7, vec![1, 2]);
        assert!(table.must_force_ordered_admit(0, 1));
        assert!(table.must_force_ordered_admit(0, 2));
        // No mid-tx checkpoint → FullAbortReexecute repair, not RewindTo.
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
        assert!(table.must_force_ordered_admit(0, 1));
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
    fn inc_carry_seen_and_snap_survive_reset() {
        let table = PartialRetryTable::new(2);
        table.reset_incarnation(0, 0);
        table.note_access(0, 77, AccessMode::Read);
        table.note_certified(0, 77);
        table.note_value(
            0,
            77,
            FfValue::Storage {
                address: Address::ZERO,
                slot: U256::from(1),
                value: U256::from(9),
                origin: None,
            },
        );
        table.reset_incarnation(0, 1);
        assert!(
            table.inc_carry_seen(0, 77),
            "tx72-class: seen ℓ must survive repair incarnation"
        );
        assert!(!table.inc_carry_seen(0, 99));
        let cur = crate::MemoryValue::Storage(U256::from(9));
        assert!(
            table.value_stable_match(0, 77, &cur),
            "carried snap must match for R1"
        );
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

    /// Lean-style abort: CallEntry floor + certified prefix → RewindTo (not bare FullAbortReexecute).
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
                certified, k_fail, ..
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
        // arm hang-free RewindTo+force-ordered_admit (cheaper than bare FullAbortReexecute).
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
            table.plan_partial_retry(0, &[1], &[1], &[]).is_none(),
            "all-invalid → no PartialRetry plan → FullAbortReexecute caller path"
        );
    }

    #[test]
    fn arm_rewind_sets_force_ordered_admit_for_next_incarnation() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        table.note_access(0, 5, AccessMode::Read);
        table.note_certified(0, 5);
        table.note_access(0, 6, AccessMode::Read);
        let plan = table.plan_partial_retry(0, &[5, 6], &[6], &[]).unwrap();
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
        table.set_force_ordered_admit(0, certified);
        assert!(table.is_rewind_resume(0));
        assert!(table.must_force_ordered_admit(0, 5));
        assert!(!table.must_force_ordered_admit(0, 6));
    }

    /// Tiny certified prefix (1 access) is **not** cheaper than OCC full_abort_reexecute.
    #[test]
    fn apply_suffix_repair_tiny_prefix_is_b0_not_rewind() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        table.note_access(0, 10, AccessMode::Read);
        table.note_certified(0, 10);
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 11, AccessMode::Read);

        match table.apply_suffix_repair(0, &[10, 11], &[11], &[]) {
            LeanAbortRepair::FullAbortReexecute { reexec_cost } => {
                assert!((reexec_cost - 2.2).abs() < 1e-9);
            }
            other => panic!(
                "expected full_abort_reexecute FullAbortReexecute (tiny PrefixSkip tax), got {other:?}"
            ),
        }
        assert!(
            !table.must_force_ordered_admit(0, 10),
            "tiny prefix must not arm ForceOrderedAdmit / SuffixRepair"
        );
        assert!(!table.is_rewind_resume(0));
    }

    /// Substantial certified prefix (≥8, ≥ half grain) → PrefixSkip.
    #[test]
    fn apply_suffix_repair_arms_rewind_when_prefix_skip_cheaper() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        let mut reads = Vec::new();
        for i in 0..8 {
            let loc = 100 + i;
            table.note_access(0, loc, AccessMode::Read);
            table.note_certified(0, loc);
            reads.push(loc);
        }
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 200, AccessMode::Read);
        table.note_access(0, 201, AccessMode::Read);
        table.note_access(0, 202, AccessMode::Read);
        reads.extend_from_slice(&[200, 201, 202]);

        match table.apply_suffix_repair(0, &reads, &[202], &[]) {
            LeanAbortRepair::SuffixRepair {
                certified,
                suffix_writes: _,
                reexec_cost,
            } => {
                assert!(certified.contains(&100));
                assert!(!certified.contains(&202));
                assert!((reexec_cost - 0.6).abs() < 1e-9);
            }
            other => panic!("expected PrefixSkip, got {other:?}"),
        }
        assert!(table.must_force_ordered_admit(0, 100));
        assert!(
            table.is_rewind_resume(0),
            "cheap PrefixSkip must arm RewindTo"
        );
    }

    #[test]
    fn prefix_skip_rejects_optimistic_read_journal_holes() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        let mut reads = Vec::new();
        for i in 0..8 {
            let loc = 100 + i;
            table.note_access_k_only(0, loc); // OptimisticRead hole
            reads.push(loc);
        }
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 200, AccessMode::Read);
        table.note_access(0, 201, AccessMode::Read);
        table.note_access(0, 202, AccessMode::Read);
        reads.extend_from_slice(&[200, 201, 202]);
        assert!(!table.journal_covers_prefix(0, 8));
        match table.apply_suffix_repair(0, &reads, &[202], &[]) {
            LeanAbortRepair::FullAbortReexecute { .. } => {}
            other => panic!("OptimisticRead holes must full_abort_reexecute, got {other:?}"),
        }
    }

    #[test]
    fn prefix_skip_beats_full_abort_predicate() {
        assert!(!PartialRetryTable::prefix_skip_beats_full_abort(1, 2, 0));
        assert!(!PartialRetryTable::prefix_skip_beats_full_abort(8, 9, 0));
        assert!(PartialRetryTable::prefix_skip_beats_full_abort(8, 11, 0));
        assert!(!PartialRetryTable::prefix_skip_beats_full_abort(8, 11, 1));
        assert!(!PartialRetryTable::prefix_skip_beats_full_abort(8, 20, 0));
    }

    /// Certified prefix but only k=0 CallEntry → full_abort_reexecute FullAbortReexecute (no ForcePrefix π).
    #[test]
    fn apply_suffix_repair_force_ordered_admit_without_mid_tx_checkpoint() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        table.note_access(0, 10, AccessMode::Read);
        table.note_certified(0, 10);
        table.note_access(0, 10, AccessMode::Write);
        table.note_access(0, 11, AccessMode::Read);
        table.note_access(0, 20, AccessMode::Write);

        match table.apply_suffix_repair(0, &[10, 11], &[11], &[10, 20]) {
            LeanAbortRepair::FullAbortReexecute { reexec_cost } => {
                assert!((reexec_cost - 2.2).abs() < 1e-9);
            }
            other => panic!(
                "expected full_abort_reexecute FullAbortReexecute (no ForcePrefix default), got {other:?}"
            ),
        }
        assert!(
            !table.must_force_ordered_admit(0, 10),
            "ForceOrderedAdmit / ForcePrefix must not arm without a certified prefix skip"
        );
        assert!(
            !table.is_rewind_resume(0),
            "k=0-only checkpoint must not arm RewindTo (hang-free SoftWait subset)"
        );
    }

    /// No certified prefix → FullAbortReexecute + cleared force-ordered_admit.
    #[test]
    fn apply_suffix_repair_full_abort_reexecute_without_prefix() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        table.note_access(0, 1, AccessMode::Read);
        table.set_force_ordered_admit(0, vec![99]);
        match table.apply_suffix_repair(0, &[1], &[1], &[]) {
            LeanAbortRepair::FullAbortReexecute { reexec_cost } => {
                assert!((reexec_cost - 2.2).abs() < 1e-9);
            }
            other => panic!("expected FullAbortReexecute, got {other:?}"),
        }
        assert!(!table.must_force_ordered_admit(0, 99));
        assert!(!table.is_rewind_resume(0));
    }

    /// Research + Lean SuffixRepair both arm RewindTo when mid-tx cp exists.

    /// Escalate clears force_ordered_admit + repair depth and returns FullAbortReexecute.
    #[test]
    fn escalate_full_abort_reexecute_clears_force_ordered_admit_and_depth() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        let mut reads = Vec::new();
        for i in 0..8 {
            let loc = 100 + i;
            table.note_access(0, loc, AccessMode::Read);
            table.note_certified(0, loc);
            reads.push(loc);
        }
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 200, AccessMode::Read);
        table.note_access(0, 201, AccessMode::Read);
        table.note_access(0, 202, AccessMode::Read);
        reads.extend_from_slice(&[200, 201, 202]);
        let _ = table.apply_suffix_repair(0, &reads, &[202], &[]);
        table.note_suffix_repair(0);
        assert!(table.has_force_ordered_admit(0));
        assert_eq!(table.suffix_repair_depth(0), 1);
        match table.escalate_full_abort_reexecute(0) {
            LeanAbortRepair::FullAbortReexecute { reexec_cost } => {
                assert!((reexec_cost - 2.2).abs() < 1e-9);
            }
            other => panic!("expected FullAbortReexecute, got {other:?}"),
        }
        assert!(!table.has_force_ordered_admit(0));
        assert_eq!(table.suffix_repair_depth(0), 0);
        assert!(!table.is_rewind_resume(0));
    }

    #[test]
    fn escalate_full_abort_reexecute_retains_ff_head_for_db_skip() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        let loc = 0xabc_u64;
        unsafe { table.state_mut(0) }.note_value(
            loc,
            FfValue::Storage {
                address: Address::ZERO,
                slot: U256::ZERO,
                value: U256::from(7),
                origin: None,
            },
        );
        let _ = table.note_access(0, loc, AccessMode::Read);
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        // Arm SuffixRepair-like RewindTo so ff_resume has values.
        let cp = table.last_checkpoint_before(0, 2).expect("cp");
        table.arm_rewind_to(0, cp, 2, vec![loc], vec![], vec![loc]);
        table.set_force_ordered_admit(0, vec![loc]);
        assert!(table.ff_value(0, loc).is_some());
        match table.escalate_full_abort_reexecute(0) {
            LeanAbortRepair::FullAbortReexecute { .. } => {}
            other => panic!("expected FullAbortReexecute, got {other:?}"),
        }
        assert!(
            !table.has_force_ordered_admit(0),
            "force_ordered_admit cleared"
        );
        assert!(!table.is_rewind_resume(0), "RewindTo cleared");
        assert!(table.has_ff_head(0), "head FF retained");
        assert!(table.ff_value(0, loc).is_some(), "ff_value via ff_head");
        table.clear_ff_head(0);
        assert!(table.ff_value(0, loc).is_none());
    }

    #[test]
    fn r2_r4_preserve_location_writer_identity() {
        let table = PartialRetryTable::new(4);
        table.note_force_writer(2, 99, 1);
        assert_eq!(table.force_writer(2, 99), Some(1));
        table.set_force_ordered_admit(2, vec![99]);
        let _ = table.escalate_full_abort_reexecute(2);
        assert!(
            !table.has_force_ordered_admit(2),
            "R4 drops force_ordered_admit"
        );
        assert_eq!(
            table.force_writer(2, 99),
            Some(1),
            "U4: R4 keeps ℓ→writer identity"
        );
        table.clear_force_writers(2);
        assert_eq!(table.force_writer(2, 99), None);
    }

    #[test]
    fn partial_abort_value_stable_when_identity_and_ff_match() {
        let table = PartialRetryTable::new(4);
        table.note_force_writer(3, 11, 1);
        table.note_force_writer(3, 12, 2);
        assert!(table.identity_held(3, &[11, 12]));
        assert!(!table.identity_held(3, &[11, 99]));
        table.note_value(
            3,
            12,
            FfValue::Storage {
                address: Address::ZERO,
                slot: U256::from(2),
                value: U256::from(7),
                origin: None,
            },
        );
        assert!(table.identity_stable_match(3, 12, &crate::MemoryValue::Storage(U256::from(7)),));
        assert!(!table.identity_stable_match(3, 12, &crate::MemoryValue::Storage(U256::from(8)),));
    }

    #[test]
    fn has_ff_resume_values_true_while_rewind_armed() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        let loc = 0xdef_u64;
        unsafe { table.state_mut(0) }.note_value(
            loc,
            FfValue::Storage {
                address: Address::ZERO,
                slot: U256::from(1),
                value: U256::from(9),
                origin: None,
            },
        );
        let _ = table.note_access(0, loc, AccessMode::Read);
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        assert!(!table.has_ff_resume_values(0));
        let cp = table.last_checkpoint_before(0, 2).expect("cp");
        table.arm_rewind_to(0, cp, 2, vec![loc], vec![], vec![loc]);
        assert!(table.is_rewind_resume(0));
        assert!(table.has_ff_resume_values(0), "Iter6 cheap-resume gate");
        let _ = table.escalate_full_abort_reexecute(0);
        assert!(!table.has_ff_resume_values(0));
        assert!(table.has_ff_head(0));
    }

    #[test]
    fn research_and_lean_suffix_repair_both_arm_rewind() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        let mut reads = Vec::new();
        for i in 0..8 {
            let loc = 100 + i;
            table.note_access(0, loc, AccessMode::Read);
            table.note_certified(0, loc);
            reads.push(loc);
        }
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        table.note_access(0, 200, AccessMode::Read);
        table.note_access(0, 201, AccessMode::Read);
        table.note_access(0, 202, AccessMode::Read);
        reads.extend_from_slice(&[200, 201, 202]);

        match table.research_apply_abort_repair(0, &reads, &[202], &[]) {
            ResearchAbortRepair::RewindTo {
                certified,
                reexec_cost,
                ..
            } => {
                assert!(certified.contains(&100));
                assert!((reexec_cost - 0.6).abs() < 1e-9);
            }
            other => panic!("expected research RewindTo, got {other:?}"),
        }
        assert!(table.is_rewind_resume(0));
        assert!(table.must_force_ordered_admit(0, 100));

        // Lean PrefixSkip only when ROI says cheaper than full_abort_reexecute (same substantial prefix).
        match table.apply_suffix_repair(0, &reads, &[202], &[]) {
            LeanAbortRepair::SuffixRepair { .. } => {}
            other => panic!("expected Lean PrefixSkip, got {other:?}"),
        }
        assert!(
            table.is_rewind_resume(0),
            "Lean PrefixSkip must arm RewindTo when skip beats full_abort_reexecute"
        );
    }
    #[test]
    fn extend_force_ordered_admit_merges_conflict_locs_after_suffix_repair() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.push_checkpoint(0, CheckpointKind::CallEntry);
        let mut reads = Vec::new();
        for i in 0..8 {
            let loc = 100 + i;
            let _ = table.note_access(0, loc, AccessMode::Read);
            table.note_certified(0, loc);
            reads.push(loc);
        }
        let _ = table.push_checkpoint(0, CheckpointKind::EffectBoundary);
        let _ = table.note_access(0, 200, AccessMode::Read);
        let _ = table.note_access(0, 201, AccessMode::Read);
        let _ = table.note_access(0, 202, AccessMode::Read);
        reads.extend_from_slice(&[200, 201, 202]);
        let repair = table.apply_suffix_repair(0, &reads, &[202], &[]);
        assert!(
            matches!(repair, LeanAbortRepair::SuffixRepair { .. })
                || matches!(repair, LeanAbortRepair::ForceOrderedAdmit { .. })
        );
        assert!(table.has_force_ordered_admit(0));
        table.extend_force_ordered_admit(0, &[202, 203]);
        assert!(table.must_force_ordered_admit(0, 202));
        assert!(table.must_force_ordered_admit(0, 203));
    }

    #[test]
    fn has_true_suffix_writes_ignores_uncertified_prefix() {
        let table = PartialRetryTable::new(1);
        table.reset_incarnation(0, 0);
        let _ = table.note_access(0, 10, AccessMode::Write); // k=1
        let _ = table.note_access(0, 20, AccessMode::Read); // k=2 fail
        assert!(
            !table.has_true_suffix_writes(0, 2, &[10]),
            "write before k_fail is not true suffix"
        );
        let _ = table.note_access(0, 30, AccessMode::Write); // k=3 after fail
        assert!(table.has_true_suffix_writes(0, 2, &[10, 30]));
    }
}
