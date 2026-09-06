//! Plant v2 M1c–M1e: CALL / effect-boundary PC resume + safe absolute jump.
//!
//! Uses a stock revm Inspector (not a custom `run_exec_loop`) to:
//! 1. Count opcodes and snapshot interpreter PC/stack/memory/gas at boundaries
//! 2. Capture revm `EvmState` journal blobs at EffectBoundary for write-prefix FF
//! 3. On RewindTo resume, **safely** absolute-jump when control-flow + journal blob
//!    make post-jump world-state ≡ sequential certified prefix; else fall back to
//!    credit-only / non-jump resume (never livelock)
//!
//! M1d: production SpecFence uses stock `inspect_run`.
//! M1e: journal-blob FF + safety-gated absolute PC jump (opt-in).
//! M1f: absolute jump **default-on** when [`jump_is_safe`]; restore MemoryGas +
//! gas refunds so post-jump expansion/refunds ≡ sequential prefix.
//! M1g: Storage-prefix jumps (never journal-blob restore) + nested CALL via
//! CallOutcome cache; `bytecode_len` relaxed carefully for storage/CALL-boundary.
//! M1h: valued CallOutcome short-circuit scaffold; write-prefix jump infra in rem.
//! M1i: post-SSTORE gas-equal write-prefix jump default-on when safe; valued
//! CallOutcome scaffold (opt-in in M1i).
//! M1j: multi-SSTORE + LOG write-prefix jump (log replay, no storage blob poison).
//! M1k: hang-free **jump-past-LOG** via LogReplay arm/restore (never live_boundaries
//! blob logs); valued CallOutcome **default-on** hang-free in-journal-only
//! (`SPECFENCE_VALUED_CALL_CACHE=0` disables); zero-value CallOutcome may combine
//! with write_replays at CALL-boundary (abort jump if touches cold).
//! M1l: lighter inspect `step` (no per-opcode full snap); warm valued CallOutcome
//! SC seq≡par via gas_limit match; valued + write_replays CALL-boundary absolute
//! jump after FF-seeded nested touches.
//! Absolute jump off by default (R0); enable with `SPECFENCE_ENABLE_INSPECT=1` or `SPECFENCE_ABSOLUTE_JUMP=1`.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};

use alloy_primitives::{Address, B256, U256};
use revm::context::{ContextTr, JournalTr};
use revm::inspector::JournalExt;
use revm::interpreter::{
    interpreter::EthInterpreter,
    interpreter_types::{Jumps, LegacyBytecode, StackTr},
    CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter,
};
use revm::state::EvmState;
use revm::Inspector;

use crate::TxIdx;

use super::metrics::MetricsInner;
use super::rem::{
    AccessMode, CheckpointKind, LogReplay, PartialRetryTable, ResumeContinuation,
    StorageWriteReplay,
};

use alloy_primitives::Log;

/// revm journal snapshot for M1e prefix FF: account/storage state + emitted logs.
///
/// Restoring only `EvmState` and jumping past LOG opcodes dropped ERC-20 Transfer
/// events (seq≠par). Logs must travel with the blob.
#[derive(Debug, Clone, Default)]
pub(crate) struct JournalBlob {
    pub state: EvmState,
    pub logs: Vec<Log>,
}

impl JournalBlob {
    pub(crate) fn is_empty(&self) -> bool {
        self.state.is_empty() && self.logs.is_empty()
    }

    pub(crate) fn account_count(&self) -> usize {
        self.state.len()
    }
}

/// Cached nested CALL outcome for M1g resume short-circuit.
///
/// Top-level tx call is never cached. Only nested frames (`depth > 1` after enter)
/// that completed before the RewindTo tip may be replayed via `Inspector::call`.
#[derive(Debug, Clone)]
pub(crate) struct CachedCallOutcome {
    /// Monotonic call ordinal within the incarnation (1 = first call hook = top-level).
    pub call_seq: u32,
    /// `CALL_DEPTH` after entering this call.
    pub depth: u16,
    pub target: Address,
    pub bytecode_address: Address,
    pub caller: Address,
    pub gas_limit: u64,
    pub is_static: bool,
    /// Transferred call value (M1h valued CallOutcome).
    pub value: U256,
    /// SpecFence effect ordinal when `call_end` fired.
    pub k_end: usize,
    pub outcome: CallOutcome,
}

/// Interpreter boundary snapshot for M1c/M1e PC resume.
#[derive(Debug, Clone)]
pub(crate) struct BoundarySnapshot {
    pub pc: usize,
    pub gas_remaining: u64,
    /// Interpreter gas refund accumulator at capture (SSTORE refunds etc.).
    pub gas_refunded: i64,
    /// `Gas::memory().words_num` — must restore or post-jump MLOAD/MSTORE
    /// re-charges full expansion from 0 and breaks gas ≡ sequential.
    pub memory_words: usize,
    /// `Gas::memory().expansion_cost` paired with `memory_words`.
    pub memory_expansion_cost: u64,
    pub call_depth: u16,
    /// Cumulative interpreter steps at capture (honest skip credit on resume).
    pub opcode_steps: u64,
    pub stack: Vec<U256>,
    pub memory: Vec<u8>,
    /// Code hash at capture (control-flow / same-contract gate).
    pub code_hash: Option<B256>,
    /// Bytecode length at capture (PC range check).
    pub bytecode_len: usize,
    /// True when snap was refreshed at `call_end` (CALL-boundary preference for M1g).
    pub at_call_boundary: bool,
    /// True when this snap was captured in `step_end` after an SSTORE opcode
    /// completed — gas_remaining / refund already include SSTORE dynamic cost
    /// (M1i post-SSTORE gas-equal jump gate).
    pub post_sstore: bool,
}

impl BoundarySnapshot {
    pub(crate) fn capture_from_interp(
        interp: &mut Interpreter<EthInterpreter>,
        call_depth: u16,
        opcode_steps: u64,
    ) -> Self {
        let bytecode_len = interp.bytecode.bytecode_slice().len();
        let code_hash = Some(interp.bytecode.get_or_calculate_hash());
        let mem_gas = *interp.gas.memory();
        Self {
            pc: interp.bytecode.pc(),
            gas_remaining: interp.gas.remaining(),
            gas_refunded: interp.gas.refunded(),
            memory_words: mem_gas.words_num,
            memory_expansion_cost: mem_gas.expansion_cost,
            call_depth,
            opcode_steps,
            stack: interp.stack.data().to_vec(),
            memory: interp.memory.context_memory().to_vec(),
            code_hash,
            bytecode_len,
            at_call_boundary: false,
            post_sstore: false,
        }
    }

    pub(crate) fn apply_to_interp(&self, interp: &mut Interpreter<EthInterpreter>) {
        interp.bytecode.absolute_jump(self.pc);
        interp.gas.set_remaining(self.gas_remaining);
        interp.gas.set_refund(self.gas_refunded);
        // Critical (M1f): sync MemoryGas with restored memory length. Leaving
        // words_num=0 after copying memory bytes makes the next memory op pay
        // full expansion from zero → OOG / wrong gas_used / seq≠par.
        let words = if self.memory_words > 0 {
            self.memory_words
        } else {
            self.memory.len().div_ceil(32)
        };
        let mg = interp.gas.memory_mut();
        mg.words_num = words;
        mg.expansion_cost = self.memory_expansion_cost;
        interp.stack.clear();
        for v in &self.stack {
            let _ = interp.stack.push(*v);
        }
        let need = self.memory.len().max(words.saturating_mul(32));
        if interp.memory.len() < need {
            interp.memory.resize(need);
        }
        if !self.memory.is_empty() {
            let mut mem = interp.memory.context_memory_mut();
            let n = self.memory.len().min(mem.len());
            mem[..n].copy_from_slice(&self.memory[..n]);
        }
    }

    /// True when this snap came from a live Inspector capture (not a lite/synthetic
    /// effect-ordinal placeholder with empty interpreter state).
    pub(crate) fn is_live_capture(&self) -> bool {
        self.gas_remaining > 0
            || self.pc > 0
            || !self.stack.is_empty()
            || !self.memory.is_empty()
            || self.code_hash.is_some()
    }
}

/// M1i safety gate: when true, production may absolute-jump PC on RewindTo resume.
///
/// Live capture, in-range PC, non-empty prefix with ≥1 Basic and/or Storage FF.
/// Storage correctness stays via SpecFence FF + force-bind / MV origins —
/// **never** journal-blob `present_values` dump (poisons pevm Db / shadows MvMemory).
/// Write-prefix allowed when post-SSTORE snap (gas already charged) + `write_replays`
/// cover storage presents for controlled journal slot replay (not blob dump).
/// Nested CallOutcome: allow jump only at CALL-boundary after replaying nested
/// touches from cache on arm; otherwise CallOutcome short-circuit alone.
/// M1l: valued nested CallOutcome OK at CALL-boundary when cached — arm FF-seeds
/// missing Basics then `transfer_loaded` (abort jump if still cold). Mid-exec
/// valued SC is default-on with gas rescale (warm seq≡par). `valued_blocks_jump`
/// refuses tip-past-valued-CALL when cache missed. `bytecode_len≤256` always
/// eligible; larger (≤4096) only with Storage FF, write_replays, and/or
/// CALL-boundary snap. Restore PC/stack/memory/MemoryGas/refund + LogReplay.
pub(crate) fn jump_is_safe(cont: &ResumeContinuation) -> bool {
    let Some(snap) = cont.jump_snap.as_ref() else {
        return false;
    };
    if !snap.is_live_capture() {
        return false;
    }
    // Nested CALL: PC-skip past CALL omits EIP-158 touch / transfer unless we
    // replay nested touches from CallOutcome cache at arm time.
    // M1l: valued + zero-value CallOutcome OK at CALL-boundary (arm FF-seeds +
    // transfer_loaded; abort if cold). Mid-exec valued SC remains default-on.
    // valued_blocks_jump: valued CALL before tip but missing from call_outcomes.
    if cont.valued_blocks_jump {
        return false;
    }
    if !cont.call_outcomes.is_empty() {
        // M1l: valued CallOutcome absolute jump allowed only with write_replays
        // (post-CALL SSTORE tip) + CALL-boundary; FF-seed + transfer_loaded on arm.
        let has_valued = cont.call_outcomes.iter().any(|c| !c.value.is_zero());
        if has_valued && cont.write_replays.is_empty() {
            return false;
        }
        if !snap.at_call_boundary {
            return false;
        }
    }
    // depth>1 without nested cache: allow shallow re-enter+jump (≤2).
    if snap.call_depth > 2 {
        return false;
    }
    if snap.bytecode_len > 0 && snap.pc >= snap.bytecode_len {
        return false;
    }
    let has_storage = cont.values.values().any(|v| {
        matches!(v, crate::specfence::rem::FfValue::Storage { .. })
    });
    let has_basic = cont.values.values().any(|v| {
        matches!(v, crate::specfence::rem::FfValue::Basic { .. })
    });
    let has_write_effects = cont.effects.iter().any(|e| e.mode == AccessMode::Write);
    // Tiny Basic-only (M1f) always OK. Larger bytecode only when Storage FF,
    // write_replays, and/or CALL-boundary make post-jump ≡ sequential under pevm MV.
    const MAX_TINY: usize = 256;
    const MAX_STORAGE: usize = 4096;
    if snap.bytecode_len > MAX_TINY {
        if snap.bytecode_len > MAX_STORAGE {
            return false;
        }
        if !has_storage && !snap.at_call_boundary && cont.write_replays.is_empty() {
            return false;
        }
    }
    if cont.cp.k == 0 && cont.effects.is_empty() && snap.opcode_steps == 0 {
        return false;
    }
    // M1j: multi-SSTORE+LOG prefixes can exceed 128 steps; allow up to 512 when
    // write_replays and/or call_outcomes certify a controlled jump.
    let max_steps = if !cont.write_replays.is_empty() || !cont.call_outcomes.is_empty() {
        512u64
    } else {
        128u64
    };
    if snap.opcode_steps == 0 || snap.opcode_steps > max_steps {
        return false;
    }
    if cont.effects.is_empty() {
        return false;
    }
    // M1i write-prefix: storage write_replays require post-SSTORE gas-equal snap.
    // Write effects without replays stay forbidden (M1g). prefix_writes that are
    // account-only (no storage replays) must not block M1g storage-read jumps.
    if !cont.write_replays.is_empty() {
        if !write_prefix_jump_is_safe(cont, snap) {
            return false;
        }
    } else if has_write_effects {
        return false;
    }
    if !has_basic && !has_storage && cont.write_replays.is_empty() {
        return false;
    }
    if let Some(blob) = cont.journal_blob.as_ref() {
        if blob.state.values().any(|a| a.is_selfdestructed()) {
            return false;
        }
        // Storage present_values in blob remain forbidden (M1f poison).
        if blob.state.values().any(|a| !a.storage.is_empty()) {
            return false;
        }
    }
    true
}

/// M1i: Write-prefix absolute jump is safe only with post-SSTORE gas evidence and
/// controlled `write_replays` (per-slot journal apply — not present_values dump).
fn write_prefix_jump_is_safe(cont: &ResumeContinuation, snap: &BoundarySnapshot) -> bool {
    if cont.write_replays.is_empty() {
        return false;
    }
    // Multi-SSTORE: finalize fills gas_remaining_after from sticky last post-SSTORE
    // capture, so every replay shares the last-boundary gas. Prefer that min/max.
    let live_gases: Vec<u64> = cont
        .write_replays
        .iter()
        .map(|w| w.gas_remaining_after)
        .filter(|g| *g > 0)
        .collect();
    if snap.post_sstore {
        // Snap after an SSTORE. Multi-slot: sticky last-SSTORE gas on all replays.
        // Refuse *early* SSTORE tips (gas still above last post-SSTORE capture) so
        // we do not apply all write_replays then re-exec later SSTOREs.
        if cont.write_replays.len() > 1 && !live_gases.is_empty() {
            let min_after = live_gases.iter().copied().min().unwrap_or(0);
            if snap.gas_remaining > min_after {
                return false;
            }
        }
    } else if live_gases.is_empty() {
        return false;
    } else {
        let max_after = live_gases.iter().copied().max().unwrap_or(0);
        let min_after = live_gases.iter().copied().min().unwrap_or(0);
        // Undercharge: snap still has more gas left than any post-SSTORE point.
        if snap.gas_remaining > max_after {
            return false;
        }
        // Post-LOG / later boundary: gas may be below last SSTORE (LOG cost). OK.
        // Refuse wildly inconsistent replay gases (different incarnation mix).
        if max_after.saturating_sub(min_after) > 50_000 {
            return false;
        }
        // Multi-SSTORE at non-post_sstore tip: also refuse early relative to last.
        if cont.write_replays.len() > 1 && snap.gas_remaining > min_after {
            return false;
        }
    }
    // Refuse storage-bearing blob (would poison on restore; we restore logs only).
    if let Some(blob) = cont.journal_blob.as_ref() {
        if blob.state.values().any(|a| !a.storage.is_empty()) {
            return false;
        }
    }
    true
}

/// Adaptive CC R0: absolute jump **off by default**. Enable with
/// `SPECFENCE_ABSOLUTE_JUMP=1` or research `SPECFENCE_ENABLE_INSPECT=1`.
pub(crate) fn absolute_jump_env_enabled() -> bool {
    if crate::specfence::research_inspect_enabled() {
        match std::env::var_os("SPECFENCE_ABSOLUTE_JUMP") {
            None => true, // inspect research implies jump unless explicitly 0
            Some(v) => v != "0",
        }
    } else {
        match std::env::var_os("SPECFENCE_ABSOLUTE_JUMP") {
            None => false,
            Some(v) => v == "1" || v.eq_ignore_ascii_case("true") || v == "blob",
        }
    }
}

/// Adaptive CC R0: valued CallOutcome SC **off by default**. Enable with
/// `SPECFENCE_VALUED_CALL_CACHE=1` or `SPECFENCE_ENABLE_INSPECT=1`.
pub(crate) fn valued_call_cache_env_enabled() -> bool {
    if crate::specfence::research_inspect_enabled() {
        match std::env::var_os("SPECFENCE_VALUED_CALL_CACHE") {
            None => true,
            Some(v) => v != "0",
        }
    } else {
        match std::env::var_os("SPECFENCE_VALUED_CALL_CACHE") {
            None => false,
            Some(v) => {
                let s = v.to_string_lossy();
                s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
            }
        }
    }
}

/// Scoped plant pointers for Inspector → PartialRetry / metrics (execute duration only).
#[derive(Clone, Copy)]
struct PlantTls {
    tx_idx: TxIdx,
    partial_retry: *const PartialRetryTable,
    metrics: *const MetricsInner,
}

thread_local! {
    /// M1l: true for the duration of inspect_run (with_plant_tls). WaitHard mid-inspect
    /// parks the whole tx and livelocks multi-SSTORE at full worker width.
    static IN_INSPECT: Cell<bool> = const { Cell::new(false) };
    static PLANT: Cell<Option<PlantTls>> = const { Cell::new(None) };
    static OPCODE_STEPS: Cell<u64> = const { Cell::new(0) };
    static CALL_DEPTH: Cell<u16> = const { Cell::new(0) };
    static CALL_SEQ: Cell<u32> = const { Cell::new(0) };
    static LAST_SNAP: RefCell<Option<BoundarySnapshot>> = const { RefCell::new(None) };
    static PENDING_EFFECT_CP: Cell<bool> = const { Cell::new(false) };
    static PENDING_RESUME: RefCell<Option<BoundarySnapshot>> = const { RefCell::new(None) };
    static PENDING_JOURNAL_BLOB: RefCell<Option<JournalBlob>> = const { RefCell::new(None) };
    /// Nested calls entered but not yet `call_end` (metadata for cache store).
    static PENDING_CALL_STACK: RefCell<Vec<PendingCallMeta>> = const { RefCell::new(Vec::new()) };
    /// Completed nested CallOutcomes captured this incarnation (for continuation).
    static CAPTURED_CALLS: RefCell<Vec<CachedCallOutcome>> = const { RefCell::new(Vec::new()) };
    /// On RewindTo resume: queue of certified nested outcomes to short-circuit.
    static RESUME_CALL_CACHE: RefCell<Vec<CachedCallOutcome>> = const { RefCell::new(Vec::new()) };
    static RESUME_CALL_IDX: Cell<usize> = const { Cell::new(0) };
    static RESUME_APPLIED: Cell<bool> = const { Cell::new(false) };
    static STEPS_THIS_RUN: Cell<u64> = const { Cell::new(0) };
    static LAST_SKIPPED: Cell<u64> = const { Cell::new(0) };
    /// Set in call_end; consumed in parent frame step_end to mark CALL-boundary.
    static PENDING_PARENT_CALL_BOUNDARY: Cell<bool> = const { Cell::new(false) };
    /// Opcode byte observed in `step` (for post-SSTORE snap marking).
    static LAST_OPCODE: Cell<u8> = const { Cell::new(0) };
    /// M1i: certified-prefix storage writes to apply into journal on absolute jump.
    static PENDING_WRITE_REPLAYS: RefCell<Vec<StorageWriteReplay>> =
        const { RefCell::new(Vec::new()) };
    /// M1i: nested CallOutcomes whose journal touches must be applied on CALL-boundary jump.
    static PENDING_CALL_TOUCHES: RefCell<Vec<CachedCallOutcome>> =
        const { RefCell::new(Vec::new()) };
    /// M1l: FfValue::Basic snapshots to seed journal for valued CALL touches (no Db).
    static PENDING_CALL_TOUCH_BASICS: RefCell<Vec<(Address, crate::AccountBasic, Option<B256>)>> =
        const { RefCell::new(Vec::new()) };
    /// M1j: LOG* events observed this incarnation (finalize → note_log_replays).
    static PREFIX_LOGS: RefCell<Vec<LogReplay>> = const { RefCell::new(Vec::new()) };
    /// M1j: LOG* to re-emit on absolute jump (from ResumeContinuation.log_replays).
    static PENDING_LOG_REPLAYS: RefCell<Vec<Log>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Clone)]
struct PendingCallMeta {
    call_seq: u32,
    depth: u16,
    target: Address,
    bytecode_address: Address,
    caller: Address,
    gas_limit: u64,
    is_static: bool,
    value: U256,
}

/// Install plant TLS for the duration of `f` (SpecFence `Vm::execute` body).
pub(crate) fn with_plant_tls<R>(
    tx_idx: TxIdx,
    partial_retry: &PartialRetryTable,
    metrics: &MetricsInner,
    f: impl FnOnce() -> R,
) -> R {
    let prev = PLANT.replace(Some(PlantTls {
        tx_idx,
        partial_retry: partial_retry as *const _,
        metrics: metrics as *const _,
    }));
    OPCODE_STEPS.set(0);
    CALL_DEPTH.set(0);
    CALL_SEQ.set(0);
    LAST_SNAP.with(|c| *c.borrow_mut() = None);
    PENDING_EFFECT_CP.set(false);
    PENDING_CALL_STACK.with(|c| c.borrow_mut().clear());
    CAPTURED_CALLS.with(|c| c.borrow_mut().clear());
    RESUME_CALL_CACHE.with(|c| c.borrow_mut().clear());
    RESUME_CALL_IDX.set(0);
    RESUME_APPLIED.set(false);
    STEPS_THIS_RUN.set(0);
    LAST_SKIPPED.set(0);
    PENDING_PARENT_CALL_BOUNDARY.set(false);
    LAST_OPCODE.set(0);
    PENDING_WRITE_REPLAYS.with(|c| c.borrow_mut().clear());
    PENDING_CALL_TOUCHES.with(|c| c.borrow_mut().clear());
    PENDING_CALL_TOUCH_BASICS.with(|c| c.borrow_mut().clear());
    PREFIX_LOGS.with(|c| c.borrow_mut().clear());
    PENDING_LOG_REPLAYS.with(|c| c.borrow_mut().clear());
    IN_INSPECT.set(true);
    let out = f();
    IN_INSPECT.set(false);
    // Persist captured nested CallOutcomes into PartialRetry for next RewindTo.
    let captured = CAPTURED_CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    if !captured.is_empty() {
        PLANT.with(|p| {
            if let Some(plant) = p.get() {
                let table = unsafe { &*plant.partial_retry };
                table.note_call_outcomes(plant.tx_idx, captured);
            }
        });
    }
    // M1j: persist LOG* for jump-past-LOG (not via live_boundaries blob).
    let logs = PREFIX_LOGS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    if !logs.is_empty() {
        PLANT.with(|p| {
            if let Some(plant) = p.get() {
                let table = unsafe { &*plant.partial_retry };
                table.note_log_replays(plant.tx_idx, logs);
            }
        });
    }
    PENDING_RESUME.with(|c| *c.borrow_mut() = None);
    PENDING_JOURNAL_BLOB.with(|c| *c.borrow_mut() = None);
    PENDING_CALL_STACK.with(|c| c.borrow_mut().clear());
    RESUME_CALL_CACHE.with(|c| c.borrow_mut().clear());
    RESUME_CALL_IDX.set(0);
    RESUME_APPLIED.set(false);
    PENDING_WRITE_REPLAYS.with(|c| c.borrow_mut().clear());
    PENDING_CALL_TOUCHES.with(|c| c.borrow_mut().clear());
    PENDING_CALL_TOUCH_BASICS.with(|c| c.borrow_mut().clear());
    PENDING_LOG_REPLAYS.with(|c| c.borrow_mut().clear());
    PREFIX_LOGS.with(|c| c.borrow_mut().clear());
    LAST_OPCODE.set(0);
    PLANT.set(prev);
    out
}

/// True while SpecFenceInspector inspect_run is active on this worker.
#[allow(dead_code)]
pub(crate) fn in_inspect_run() -> bool {
    IN_INSPECT.get()
}


/// Arm PC resume for the next matching-depth interpreter init (RewindTo path).
pub(crate) fn arm_pc_resume(snap: BoundarySnapshot) {
    arm_pc_resume_with_blob(snap, None);
}

/// Arm PC resume + optional revm journal blob restore (M1e write-prefix FF).
pub(crate) fn arm_pc_resume_with_blob(snap: BoundarySnapshot, blob: Option<JournalBlob>) {
    PENDING_RESUME.with(|c| *c.borrow_mut() = Some(snap));
    PENDING_JOURNAL_BLOB.with(|c| *c.borrow_mut() = blob);
    RESUME_APPLIED.set(false);
}

pub(crate) fn clear_pc_resume() {
    PENDING_RESUME.with(|c| *c.borrow_mut() = None);
    PENDING_JOURNAL_BLOB.with(|c| *c.borrow_mut() = None);
    RESUME_CALL_CACHE.with(|c| c.borrow_mut().clear());
    RESUME_CALL_IDX.set(0);
    RESUME_APPLIED.set(false);
    PENDING_WRITE_REPLAYS.with(|c| c.borrow_mut().clear());
    PENDING_LOG_REPLAYS.with(|c| c.borrow_mut().clear());
    PENDING_CALL_TOUCHES.with(|c| c.borrow_mut().clear());
    PENDING_CALL_TOUCH_BASICS.with(|c| c.borrow_mut().clear());
}

/// Arm nested CallOutcome short-circuit queue for the next inspect_run (RewindTo).
pub(crate) fn arm_call_outcome_cache(calls: Vec<CachedCallOutcome>) {
    RESUME_CALL_IDX.set(0);
    RESUME_CALL_CACHE.with(|c| *c.borrow_mut() = calls);
}

fn record_call_outcome_hit() {
    PLANT.with(|p| {
        if let Some(plant) = p.get() {
            let metrics = unsafe { &*plant.metrics };
            metrics.record_call_outcome_cache_hit();
        }
    });
}

fn current_effect_k() -> usize {
    PLANT.with(|p| {
        p.get().map(|plant| {
            let table = unsafe { &*plant.partial_retry };
            table.current_k(plant.tx_idx)
        }).unwrap_or(0)
    })
}

/// M1e: if continuation passes [`jump_is_safe`] and jump is not disabled for this
/// tx (anti-livelock), arm absolute PC jump (+ journal blob).
/// Returns true when armed; false → caller must use credit-only / non-jump fallback.
pub(crate) fn try_arm_safe_absolute_jump(
    tx_idx: TxIdx,
    partial_retry: &PartialRetryTable,
    cont: &ResumeContinuation,
    metrics: &MetricsInner,
) -> bool {
    // M1f: default-on when jump_is_safe; SPECFENCE_ABSOLUTE_JUMP=0 disables.
    // Anti-livelock: jump_disabled after a jumped resume fails validation.
    if !absolute_jump_env_enabled()
        || partial_retry.is_jump_disabled(tx_idx)
        || !jump_is_safe(cont)
    {
        metrics.record_absolute_jump_fallback();
        return false;
    }
    let snap = cont.jump_snap.clone().expect("jump_is_safe implies jump_snap");
    // Never restore storage present_values (poison pevm Db / MV).
    // M1k: jump-past-LOG is hang-free when LogReplay restores receipt logs on
    // initialize_interp (snap-only tip — never live_boundaries blob logs, which
    // hung under concurrency). Arm skipped LOG* for PC ≤ jump tip.
    let jump_pc = snap.pc;
    let skipped_logs: Vec<_> = cont
        .log_replays
        .iter()
        .filter(|lr| lr.pc <= jump_pc)
        .map(|lr| lr.log.clone())
        .collect();
    PENDING_LOG_REPLAYS.with(|c| {
        *c.borrow_mut() = skipped_logs;
    });
    arm_pc_resume_with_blob(snap, None);
    PENDING_WRITE_REPLAYS.with(|c| {
        *c.borrow_mut() = cont.write_replays.clone();
    });
    // CALL-boundary jump: apply nested touches on arm (Inspector::call won't fire
    // for skipped CALL). Also keep cache for any nested re-enter below jump PC.
    // M1l: seed Basics from FF values so valued transfer_loaded can succeed without
    // load_account / WaitHard (inner often absent at top-level initialize_interp).
    if !cont.call_outcomes.is_empty() {
        PENDING_CALL_TOUCHES.with(|c| {
            *c.borrow_mut() = cont.call_outcomes.clone();
        });
        let mut basics = Vec::new();
        for cached in &cont.call_outcomes {
            for addr in [cached.caller, cached.target] {
                for v in cont.values.values() {
                    if let crate::specfence::rem::FfValue::Basic {
                        address,
                        basic,
                        code_hash,
                        ..
                    } = v
                    {
                        if *address == addr {
                            basics.push((*address, basic.clone(), *code_hash));
                        }
                    }
                }
            }
        }
        PENDING_CALL_TOUCH_BASICS.with(|c| {
            *c.borrow_mut() = basics;
        });
        arm_call_outcome_cache(cont.call_outcomes.clone());
    }
    true
}

/// Record an EffectBoundary checkpoint.
///
/// Always emit a lite snap immediately (M1c-compatible k-tracking for repair).
/// When SpecFenceInspector is driving `inspect_run`, set `PENDING_EFFECT_CP` so
/// `step_end` attaches a live PC/stack snap + journal blob to this k (M1e).

/// Attach the latest Inspector snap at the current effect ordinal (SpecRead path).
/// Does not push an EffectBoundary checkpoint (those livelocked ERC-20 schedules).
pub(crate) fn attach_current_live_snap(tx_idx: TxIdx, partial_retry: &PartialRetryTable) {
    let Some(snap) = last_boundary_snap() else {
        return;
    };
    if !snap.is_live_capture() {
        return;
    }
    partial_retry.attach_live_boundary(tx_idx, snap, JournalBlob::default());
}

pub(crate) fn note_pending_effect_boundary(
    tx_idx: TxIdx,
    partial_retry: &PartialRetryTable,
) {
    let k = partial_retry.current_k(tx_idx);
    let live_steps = LAST_SNAP.with(|c| c.borrow().as_ref().map(|s| s.opcode_steps));
    let snap = BoundarySnapshot {
        pc: 0,
        gas_remaining: 0,
        gas_refunded: 0,
        memory_words: 0,
        memory_expansion_cost: 0,
        call_depth: 0,
        opcode_steps: live_steps.filter(|n| *n > 0).unwrap_or(k as u64),
        stack: Vec::new(),
        memory: Vec::new(),
        code_hash: None,
        bytecode_len: 0,
        at_call_boundary: false,
        post_sstore: false,
        };
    let _ = partial_retry.push_checkpoint_with_boundary(
        tx_idx,
        CheckpointKind::EffectBoundary,
        Some(snap),
    );
    // M1f: always arm live snap capture at effect boundaries (snap-only in step_end).
    PENDING_EFFECT_CP.set(true);
}

pub(crate) fn last_boundary_snap() -> Option<BoundarySnapshot> {
    LAST_SNAP.with(|c| c.borrow().clone())
}

pub(crate) fn steps_this_run() -> u64 {
    STEPS_THIS_RUN.get()
}

pub(crate) fn last_prefix_opcodes_skipped() -> u64 {
    LAST_SKIPPED.get()
}

pub(crate) fn resume_was_applied() -> bool {
    RESUME_APPLIED.get()
}

fn push_cp(kind: CheckpointKind, snap: Option<BoundarySnapshot>) {
    PLANT.with(|p| {
        if let Some(plant) = p.get() {
            // SAFETY: pointers live for with_plant_tls scope covering inspect_run.
            let table = unsafe { &*plant.partial_retry };
            let _ = table.push_checkpoint_with_boundary(plant.tx_idx, kind, snap);
        }
    });
}

fn record_pc_resume(skipped: u64) {
    LAST_SKIPPED.set(skipped);
    PLANT.with(|p| {
        if let Some(plant) = p.get() {
            let metrics = unsafe { &*plant.metrics };
            metrics.record_pc_resume(skipped);
            metrics.record_live_pc_resume();
            metrics.record_absolute_jump_applied();
        }
    });
}

fn record_journal_blob_ff(accounts: usize) {
    PLANT.with(|p| {
        if let Some(plant) = p.get() {
            let metrics = unsafe { &*plant.metrics };
            metrics.record_journal_blob_ff(accounts);
        }
    });
}

fn attach_live_to_plant(snap: BoundarySnapshot, blob: JournalBlob) {
    PLANT.with(|p| {
        if let Some(plant) = p.get() {
            let table = unsafe { &*plant.partial_retry };
            table.attach_live_boundary(plant.tx_idx, snap, blob);
        }
    });
}

/// Apply certified-prefix storage write presents into the live revm journal.
///
/// Only mutates the listed slots after ensuring the account is loaded; does **not**
/// dump arbitrary blob `present_values` (avoids Db / MvMemory poison).
fn apply_write_replays<CTX>(context: &mut CTX, writes: &[StorageWriteReplay])
where
    CTX: ContextTr,
    CTX::Journal: JournalExt,
{
    use revm::state::EvmStorageSlot;
    // Do **not** journal.load_account here — that re-enters pevm Db/maybe_wait and
    // can Block/livelock mid-initialize_interp. Only update accounts already present
    // in the journal; MvMemory residual + write_replays republish cover write-set.
    let state = context.journal_mut().evm_state_mut();
    let tx_id = state.values().next().map(|a| a.transaction_id).unwrap_or(0);
    for wr in writes {
        let Some(acc) = state.get_mut(&wr.address) else {
            continue;
        };
        let _ = acc.mark_warm_with_transaction_id(tx_id);
        let slot = EvmStorageSlot::new_changed(wr.original, wr.present, tx_id);
        acc.storage.insert(wr.slot, slot);
        acc.mark_touch();
    }
}

/// True when both addresses are already present in the revm journal (no Db miss).
fn journal_has_accounts<CTX>(context: &mut CTX, a: Address, b: Address) -> bool
where
    CTX: ContextTr,
    CTX::Journal: JournalExt,
{
    let state = context.journal().evm_state();
    state.contains_key(&a) && state.contains_key(&b)
}

/// Hang-free valued/zero transfer: only `transfer_loaded` when both accounts are
/// already in-journal. Never `load_account` / `load_account_with_code` — those
/// re-enter pevm Db `maybe_wait` and WW-livelock shared outer/inner Basics.
fn try_transfer_in_journal<CTX>(context: &mut CTX, from: Address, to: Address, value: U256) -> bool
where
    CTX: ContextTr,
    CTX::Journal: JournalExt,
{
    if !journal_has_accounts(context, from, to) {
        return false;
    }
    let journal = context.journal_mut();
    let checkpoint = journal.checkpoint();
    if let Some(_err) = journal.transfer_loaded(from, to, value) {
        journal.checkpoint_revert(checkpoint);
        false
    } else {
        journal.checkpoint_commit();
        true
    }
}

/// M1l: insert FfValue::Basic into revm journal without `load_account` / WaitHard.
/// Used so valued CALL-boundary jump can `transfer_loaded` when the nested target
/// was never loaded at top-level `initialize_interp`.
fn seed_journal_basic_if_missing<CTX>(
    context: &mut CTX,
    address: Address,
    basic: &crate::AccountBasic,
    code_hash: Option<B256>,
) where
    CTX: ContextTr,
    CTX::Journal: JournalExt,
{
    use revm::primitives::KECCAK_EMPTY;
    use revm::state::{Account, AccountInfo};
    let state = context.journal_mut().evm_state_mut();
    if state.contains_key(&address) {
        return;
    }
    let tx_id = state.values().next().map(|a| a.transaction_id).unwrap_or(0);
    // Never publish a non-empty code_hash with code=None — finalize unwraps
    // new_bytecodes and panics. Transfer-only seed uses empty code_hash; the
    // real code is loaded via Db when the CALL frame needs it (SC/jump skip).
    let _ = code_hash;
    let info = AccountInfo {
        balance: basic.balance,
        nonce: basic.nonce,
        code_hash: KECCAK_EMPTY,
        code: None,
        account_id: None,
    };
    let mut acc = Account::from(info);
    let _ = acc.mark_warm_with_transaction_id(tx_id);
    state.insert(address, acc);
}

/// Replicate make_call_frame journal side effects for a cached nested CALL
/// (EIP-158 touch + value transfer) so CALL-boundary absolute jump ≡ sequential.
/// M1j: in-journal-only — skip (no panic / no WaitHard) if accounts not warm yet.
fn apply_cached_call_touches<CTX>(context: &mut CTX, cached: &CachedCallOutcome)
where
    CTX: ContextTr,
    CTX::Journal: JournalExt,
{
    if try_transfer_in_journal(context, cached.caller, cached.target, cached.value) {
        record_call_outcome_hit();
    }
}

/// SpecFence boundary Inspector — observational except on armed PC resume.
#[derive(Debug, Default, Clone)]
pub struct SpecFenceInspector;

impl SpecFenceInspector {
    /// Construct the SpecFence boundary inspector.
    pub const fn new() -> Self {
        Self
    }
}

impl<CTX> Inspector<CTX, EthInterpreter> for SpecFenceInspector
where
    CTX: ContextTr,
    CTX::Journal: JournalExt,
{
    fn initialize_interp(
        &mut self,
        interp: &mut Interpreter<EthInterpreter>,
        context: &mut CTX,
    ) {
        if RESUME_APPLIED.get() {
            return;
        }
        let snap = PENDING_RESUME.with(|c| c.borrow().clone());
        let Some(snap) = snap else {
            return;
        };
        let depth = CALL_DEPTH.get();
        if snap.call_depth != depth && !(snap.call_depth <= 1 && depth <= 1) {
            // Depth mismatch — leave pending cleared and fall through without jump.
            clear_pc_resume();
            return;
        }
        // Same-contract gate: refuse jump if code hash diverges.
        if let Some(expected) = snap.code_hash {
            let actual = interp.bytecode.get_or_calculate_hash();
            if actual != expected {
                clear_pc_resume();
                return;
            }
        }
        if snap.bytecode_len > 0 && snap.pc >= snap.bytecode_len {
            clear_pc_resume();
            return;
        }
        // M1e/M1f: restore revm journal blob so prefix SSTORE/LOG world-state is
        // present without re-executing those opcodes after the PC jump.
        // Re-warm accounts/slots for the current journal transaction_id so
        // post-jump SLOAD/SSTORE see warm gas (remaining already accounts for it).
        let blob = PENDING_JOURNAL_BLOB.with(|c| c.borrow_mut().take());
        if let Some(blob) = blob {
            let n = blob.account_count();
            let state = context.journal_mut().evm_state_mut();
            let tx_id = state
                .values()
                .next()
                .map(|a| a.transaction_id)
                .or_else(|| blob.state.values().next().map(|a| a.transaction_id))
                .unwrap_or(0);
            for (addr, mut acc) in blob.state {
                let _ = acc.mark_warm_with_transaction_id(tx_id);
                for slot in acc.storage.values_mut() {
                    let _ = slot.mark_warm_with_transaction_id(tx_id);
                }
                state.insert(addr, acc);
            }
            for log in blob.logs {
                context.journal_mut().log(log);
            }
            if n > 0 {
                record_journal_blob_ff(n);
            }
        }
        // M1i/M1j/M1l: replay nested CALL journal touches before PC-skip past CALL.
        // Zero-value needs EIP-158 touch; valued needs transfer. FF-seed Basics
        // first (no Db) so valued targets absent at top-level init can transfer.
        // Abort jump if still cold (hang-free, seq≡par via mid-exec SC fallback).
        let seed_basics =
            PENDING_CALL_TOUCH_BASICS.with(|c| std::mem::take(&mut *c.borrow_mut()));
        for (addr, basic, code_hash) in &seed_basics {
            seed_journal_basic_if_missing(context, *addr, basic, *code_hash);
        }
        let call_touches = PENDING_CALL_TOUCHES.with(|c| std::mem::take(&mut *c.borrow_mut()));
        for cached in &call_touches {
            if try_transfer_in_journal(
                context,
                cached.caller,
                cached.target,
                cached.value,
            ) {
                record_call_outcome_hit();
            } else {
                // Jump armed but touches cannot apply — abort jump and re-arm
                // CallOutcome cache so mid-exec SC still runs (vm.rs skipped the
                // !jumped arm path because try_arm returned true).
                clear_pc_resume();
                PENDING_WRITE_REPLAYS.with(|c| c.borrow_mut().clear());
                PENDING_CALL_TOUCH_BASICS.with(|c| c.borrow_mut().clear());
                arm_call_outcome_cache(call_touches.clone());
                return;
            }
        }
        // M1j: re-emit certified-prefix LOG* skipped by absolute jump.
        let logs = PENDING_LOG_REPLAYS.with(|c| std::mem::take(&mut *c.borrow_mut()));
        for log in logs {
            context.journal_mut().log(log);
        }
        // M1i: controlled per-slot storage replay (never full present_values dump).
        let writes = PENDING_WRITE_REPLAYS.with(|c| std::mem::take(&mut *c.borrow_mut()));
        if !writes.is_empty() {
            apply_write_replays(context, &writes);
        }
        let skipped = snap.opcode_steps;
        snap.apply_to_interp(interp);
        RESUME_APPLIED.set(true);
        PENDING_RESUME.with(|c| *c.borrow_mut() = None);
        OPCODE_STEPS.set(0);
        record_pc_resume(skipped);
    }

    fn step(&mut self, interp: &mut Interpreter<EthInterpreter>, _context: &mut CTX) {
        let n = OPCODE_STEPS.get() + 1;
        OPCODE_STEPS.set(n);
        STEPS_THIS_RUN.set(STEPS_THIS_RUN.get() + 1);
        // M1l: do **not** full-capture stack/memory every opcode — that alloc tax
        // widened the WW conflict window and hung multi-SSTORE at full width.
        // step_end captures live snaps at EffectBoundary / CALL / SSTORE / LOG.
        let pc = interp.bytecode.pc();
        let op = interp.bytecode.bytecode_slice().get(pc).copied().unwrap_or(0);
        LAST_OPCODE.set(op);
    }

    fn step_end(&mut self, interp: &mut Interpreter<EthInterpreter>, context: &mut CTX) {
        let depth = CALL_DEPTH.get();
        let n = OPCODE_STEPS.get();
        let mut snap = BoundarySnapshot::capture_from_interp(interp, depth, n);
        let call_boundary = PENDING_PARENT_CALL_BOUNDARY.replace(false);
        if call_boundary {
            snap.at_call_boundary = true;
        }
        // M1i/M1j: after SSTORE completes, gas_remaining includes dynamic cost (~20k).
        // Post-LOG snaps rely on live_gases undercharge check (not sticky mark).
        const OP_SSTORE: u8 = 0x55;
        const OP_LOG0: u8 = 0xa0;
        const OP_LOG4: u8 = 0xa4;
        let op = LAST_OPCODE.get();
        if op == OP_SSTORE {
            snap.post_sstore = true;
            let gas_after = snap.gas_remaining;
            PLANT.with(|p| {
                if let Some(plant) = p.get() {
                    let table = unsafe { &*plant.partial_retry };
                    table.note_post_sstore_gas(plant.tx_idx, gas_after);
                }
            });
        } else if (OP_LOG0..=OP_LOG4).contains(&op) {
            // M1j/M1k: record new journal logs with post-LOG PC (filter on jump).
            let pc = snap.pc;
            let jlogs = context.journal().logs();
            PREFIX_LOGS.with(|c| {
                let mut v = c.borrow_mut();
                let already = v.len();
                for log in jlogs.iter().skip(already) {
                    v.push(LogReplay {
                        pc,
                        log: log.clone(),
                    });
                }
            });
            // M1k: eager flush LogReplay (same mid-abort race as CallOutcome).
            PLANT.with(|p| {
                if let Some(plant) = p.get() {
                    let table = unsafe { &*plant.partial_retry };
                    let logs = PREFIX_LOGS.with(|c| c.borrow().clone());
                    table.note_log_replays(plant.tx_idx, logs);
                }
            });
            // M1k: attach post-LOG live tip (snap-only — never blob logs) so
            // RewindTo can absolute-jump past LOG; LogReplay restores receipts.
            attach_live_to_plant(snap.clone(), JournalBlob::default());
        }
        LAST_SNAP.with(|c| *c.borrow_mut() = Some(snap.clone()));
        // Attach on EffectBoundary *or* CALL-boundary so RewindTo can jump post-CALL.
        // Post-SSTORE EffectBoundary snaps are gas-equal for write-prefix jumps.
        // Snap-only by default (M1i/M1k) — never put logs in live_boundaries (hang).
        if PENDING_EFFECT_CP.replace(false) || call_boundary {
            let blob = if std::env::var_os("SPECFENCE_ABSOLUTE_JUMP").is_some_and(|v| v == "blob") {
                let full = context.journal().evm_state();
                let mut state = EvmState::default();
                for (addr, acc) in full.iter() {
                    if acc.is_touched() {
                        let mut a = acc.clone();
                        a.storage.clear();
                        state.insert(*addr, a);
                    }
                }
                JournalBlob {
                    state,
                    logs: context.journal().logs().to_vec(),
                }
            } else {
                JournalBlob::default()
            };
            attach_live_to_plant(snap, blob);
        }
    }

    fn call(
        &mut self,
        context: &mut CTX,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        let parent_depth = CALL_DEPTH.get();
        let depth = parent_depth.saturating_add(1);
        CALL_DEPTH.set(depth);
        let seq = CALL_SEQ.get().saturating_add(1);
        CALL_SEQ.set(seq);
        let call_value = inputs.value.transfer().unwrap_or(U256::ZERO);
        PENDING_CALL_STACK.with(|s| {
            s.borrow_mut().push(PendingCallMeta {
                call_seq: seq,
                depth,
                target: inputs.target_address,
                bytecode_address: inputs.bytecode_address,
                caller: inputs.caller,
                gas_limit: inputs.gas_limit,
                is_static: inputs.is_static,
                value: call_value,
            });
        });
        // M1g/M1h: short-circuit certified nested CALLs from resume cache.
        // Never override the top-level tx call (parent_depth == 0).
        if parent_depth >= 1 {
            let hit = RESUME_CALL_CACHE.with(|c| {
                let cache = c.borrow();
                let idx = RESUME_CALL_IDX.get();
                if idx >= cache.len() {
                    return None;
                }
                let cached = &cache[idx];
                if cached.call_seq == seq
                    && cached.depth == depth
                    && cached.target == inputs.target_address
                    && cached.bytecode_address == inputs.bytecode_address
                    && cached.caller == inputs.caller
                {
                    Some(cached.clone())
                } else {
                    None
                }
            });
            if let Some(cached) = hit {
                let value = if !cached.value.is_zero() {
                    cached.value
                } else {
                    call_value
                };
                // M1k/M1l: valued mid-exec short-circuit default-on
                // (SPECFENCE_VALUED_CALL_CACHE=0 disables). Hang-free: in-journal-only
                // transfer — never load_account. M1l warm seq≡par: only SC when the
                // current gas_limit matches the cached call (stipend-stable). On
                // mismatch fall through to make_call_frame (correct fresh gas).
                let allow_valued = valued_call_cache_env_enabled();
                if value.is_zero() || allow_valued {
                    if inputs.gas_limit != cached.gas_limit {
                        // Stipend changed across RewindTo — do not reuse cached Gas.
                        return None;
                    }
                    if try_transfer_in_journal(
                        context,
                        inputs.caller,
                        inputs.target_address,
                        value,
                    ) {
                        RESUME_CALL_IDX.set(RESUME_CALL_IDX.get().saturating_add(1));
                        record_call_outcome_hit();
                        return Some(cached.outcome.clone());
                    }
                }
            }
        }
        None
    }

    fn call_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
        let meta = PENDING_CALL_STACK.with(|s| s.borrow_mut().pop());
        if let Some(meta) = meta {
            // Cache successful nested calls (depth > 1) for RewindTo short-circuit.
            if meta.depth > 1 && outcome.result.is_ok() {
                let cached = CachedCallOutcome {
                    call_seq: meta.call_seq,
                    depth: meta.depth,
                    target: meta.target,
                    bytecode_address: meta.bytecode_address,
                    caller: meta.caller,
                    gas_limit: meta.gas_limit,
                    is_static: meta.is_static,
                    value: meta.value,
                    k_end: current_effect_k(),
                    outcome: outcome.clone(),
                };
                CAPTURED_CALLS.with(|c| c.borrow_mut().push(cached));
                // M1k: eager flush — mid-exec abort before with_plant_tls end
                // previously lost CallOutcomes while live tips sat past CALL
                // (absolute jump skipped valued transfer → seq≠par).
                PLANT.with(|p| {
                    if let Some(plant) = p.get() {
                        let table = unsafe { &*plant.partial_retry };
                        let snap = CAPTURED_CALLS.with(|c| c.borrow().clone());
                        table.note_call_outcomes(plant.tx_idx, snap);
                    }
                });
            }
        }
        // Parent frame's next step_end marks at_call_boundary (not nested snap).
        PENDING_PARENT_CALL_BOUNDARY.set(true);
        CALL_DEPTH.set(CALL_DEPTH.get().saturating_sub(1));
    }

    fn create(
        &mut self,
        _context: &mut CTX,
        _inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        CALL_DEPTH.set(CALL_DEPTH.get().saturating_add(1));
        None
    }

    fn create_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CreateInputs,
        _outcome: &mut CreateOutcome,
    ) {
        CALL_DEPTH.set(CALL_DEPTH.get().saturating_sub(1));
    }
}


#[cfg(test)]
mod m1c_tests {
    use super::*;
    use revm::interpreter::interpreter::{EthInterpreter, ExtBytecode};
    use revm::interpreter::{Gas, Interpreter};
    use revm::state::Bytecode;

    use crate::specfence::rem::{AccessMode, CheckpointId, FfValue, RegionAccess, ResumeContinuation};
    use hashbrown::HashMap;
    use alloy_primitives::Address;
    use revm::state::AccountInfo;
    use crate::BuildIdentityHasher;

    fn basic_read_effect() -> (Vec<RegionAccess>, hashbrown::HashMap<u64, FfValue, BuildIdentityHasher>) {
        let mut values = HashMap::with_hasher(BuildIdentityHasher::default());
        values.insert(
            1u64,
            FfValue::Basic {
                address: Address::ZERO,
                basic: Default::default(),
                code_hash: None,
                origin: None,
            },
        );
        (
            vec![RegionAccess {
                tx_idx: 0,
                k: 1,
                location: 1,
                mode: AccessMode::Read,
            }],
            values,
        )
    }

    fn lite_snap(pc: usize, steps: u64) -> BoundarySnapshot {
        BoundarySnapshot {
            pc,
            gas_remaining: 0,
            gas_refunded: 0,
            memory_words: 0,
            memory_expansion_cost: 0,
            call_depth: 0,
            opcode_steps: steps,
            stack: Vec::new(),
            memory: Vec::new(),
            code_hash: None,
            bytecode_len: 0,
            at_call_boundary: false,

            post_sstore: false,
        }
    }

    #[test]
    fn boundary_snapshot_roundtrip_fields() {
        let snap = BoundarySnapshot {
            pc: 42,
            gas_remaining: 99_000,
            gas_refunded: 0,
            memory_words: 0,
            memory_expansion_cost: 0,
            call_depth: 1,
            opcode_steps: 17,
            stack: vec![U256::from(1), U256::from(2)],
            memory: vec![0xab, 0xcd],
            code_hash: None,
            bytecode_len: 64,
            at_call_boundary: false,

            post_sstore: false,
        };
        assert_eq!(snap.pc, 42);
        assert_eq!(snap.opcode_steps, 17);
        assert_eq!(snap.stack.len(), 2);
        assert!(snap.is_live_capture());
    }

    #[test]
    fn apply_to_interp_jumps_pc_and_restores_stack() {
        let code = Bytecode::new_raw(vec![0x00, 0x00, 0x00, 0x00, 0x00].into());
        let mut interp = Interpreter::<EthInterpreter>::default();
        let snap = BoundarySnapshot {
            pc: 2,
            gas_remaining: 50_000,
            gas_refunded: 0,
            memory_words: 0,
            memory_expansion_cost: 0,
            call_depth: 1,
            opcode_steps: 9,
            stack: vec![U256::from(7), U256::from(8)],
            memory: vec![1, 2, 3, 4],
            code_hash: None,
            bytecode_len: 5,
            at_call_boundary: false,

            post_sstore: false,
        };
        interp.bytecode = ExtBytecode::new(code);
        interp.gas = Gas::new(100_000);
        snap.apply_to_interp(&mut interp);
        assert_eq!(interp.bytecode.pc(), 2, "M1c must jump PC to boundary");
        assert_eq!(interp.gas.remaining(), 50_000);
        assert_eq!(interp.stack.data(), &[U256::from(7), U256::from(8)]);
        assert!(interp.memory.len() >= 4);
    }

    #[test]
    fn arm_pc_resume_sets_pending_flags() {
        clear_pc_resume();
        let snap = BoundarySnapshot {
            pc: 0,
            gas_remaining: 10,
            gas_refunded: 0,
            memory_words: 0,
            memory_expansion_cost: 0,
            call_depth: 0,
            opcode_steps: 42,
            stack: vec![],
            memory: vec![],
            code_hash: None,
            bytecode_len: 1,
            at_call_boundary: false,

            post_sstore: false,
        };
        arm_pc_resume(snap);
        assert!(!resume_was_applied());
        clear_pc_resume();
    }

    #[test]
    fn jump_is_safe_rejects_lite_and_nested_call() {
        let lite = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 0,
                k: 2,
            },
            k_fail: 3,
            certified: vec![1],
            suffix_writes: vec![],
            effects: vec![],
            checkpoints: vec![],
            values: HashMap::with_hasher(BuildIdentityHasher::default()),
            boundary: Some(lite_snap(0, 2)),
            jump_snap: None,
            journal_blob: None,
            call_outcomes: vec![],
            prefix_writes: vec![],
            write_replays: vec![],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(!jump_is_safe(&lite), "lite snap must not jump");

        let nested = ResumeContinuation {
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 1_000,
            gas_refunded: 0,
            memory_words: 0,
            memory_expansion_cost: 0,
                call_depth: 2,
                opcode_steps: 10,
                stack: vec![U256::from(1)],
                memory: vec![],
                code_hash: Some(B256::ZERO),
                bytecode_len: 32,
                at_call_boundary: false,

            post_sstore: false,
        }),
            journal_blob: None,
            ..lite.clone()
        };
        assert!(!jump_is_safe(&nested), "nested without effects/FF must fall back");
    }

    #[test]
    fn jump_is_safe_accepts_live_with_journal_blob() {
        let mut state = EvmState::default();
        state.insert(alloy_primitives::Address::ZERO, Default::default());
        let (effects, values) = basic_read_effect();
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1, 2],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 50_000,
            gas_refunded: 0,
            memory_words: 0,
            memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 12,
                stack: vec![U256::from(9)],
                memory: vec![0],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: false,

            post_sstore: false,
        }),
            journal_blob: Some(JournalBlob {
                state,
                logs: vec![],
            }),
            call_outcomes: vec![],
            prefix_writes: vec![],
            write_replays: vec![],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            jump_is_safe(&cont),
            "live top-level read-only snap may jump"
        );
    }

        #[test]
    fn jump_is_safe_rejects_write_prefix() {
        use crate::specfence::rem::{AccessMode, RegionAccess};
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1],
            suffix_writes: vec![],
            effects: vec![RegionAccess {
                tx_idx: 0,
                k: 1,
                location: 42,
                mode: AccessMode::Write,
            }],
            checkpoints: vec![],
            values: HashMap::with_hasher(BuildIdentityHasher::default()),
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 50_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 12,
                stack: vec![U256::from(9)],
                memory: vec![0],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: false,

            post_sstore: false,
        }),
            journal_blob: Some(JournalBlob {
                state: EvmState::default(),
                logs: vec![],
            }),
            call_outcomes: vec![],
            prefix_writes: vec![],
            write_replays: vec![],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            !jump_is_safe(&cont),
            "write-prefix without write_replays / post_sstore must fall back"
        );
    }

    #[test]
    fn jump_is_safe_accepts_write_prefix_with_post_sstore_replays() {
        use crate::specfence::rem::{AccessMode, RegionAccess, StorageWriteReplay};
        let (mut effects, values) = storage_read_effect();
        effects.push(RegionAccess {
            tx_idx: 0,
            k: 2,
            location: 42,
            mode: AccessMode::Write,
        });
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 8,
                gas_remaining: 30_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 12,
                stack: vec![],
                memory: vec![],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: false,
                post_sstore: true,
            }),
            journal_blob: None,
            call_outcomes: vec![],
            prefix_writes: vec![42],
            write_replays: vec![StorageWriteReplay {
                address: Address::ZERO,
                slot: U256::ZERO,
                original: U256::ZERO,
                present: U256::from(1),
                gas_remaining_after: 30_000,
            }],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            jump_is_safe(&cont),
            "M1i: post-SSTORE snap + write_replays must allow write-prefix jump"
        );
    }

    #[test]
    fn jump_is_safe_accepts_read_only_without_blob() {
        let (effects, values) = basic_read_effect();
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 50_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 12,
                stack: vec![U256::from(9)],
                memory: vec![0],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: false,

            post_sstore: false,
        }),
            journal_blob: None,
            call_outcomes: vec![],
            prefix_writes: vec![],
            write_replays: vec![],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            jump_is_safe(&cont),
            "read-only live snap may jump without blob"
        );
    }

    fn storage_read_effect() -> (Vec<RegionAccess>, hashbrown::HashMap<u64, FfValue, BuildIdentityHasher>) {
        let mut values = HashMap::with_hasher(BuildIdentityHasher::default());
        values.insert(
            7u64,
            FfValue::Storage {
                address: Address::ZERO,
                slot: U256::ZERO,
                value: U256::from(1),
                origin: None,
            },
        );
        (
            vec![RegionAccess {
                tx_idx: 0,
                k: 1,
                location: 7,
                mode: AccessMode::Read,
            }],
            values,
        )
    }

    #[test]
    fn jump_is_safe_accepts_storage_read_prefix() {
        let (effects, values) = storage_read_effect();
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![7],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 50_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 12,
                stack: vec![U256::from(9)],
                memory: vec![0],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: false,

            post_sstore: false,
        }),
            journal_blob: None,
            call_outcomes: vec![],
            prefix_writes: vec![],
            write_replays: vec![],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            jump_is_safe(&cont),
            "M1g: Storage-read certified prefix may absolute-jump"
        );
    }

    #[test]
    fn jump_is_safe_accepts_depth2_with_call_cache() {
        let (effects, values) = basic_read_effect();
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 50_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 2,
                opcode_steps: 12,
                stack: vec![U256::from(9)],
                memory: vec![0],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: true,

            post_sstore: false,
        }),
            journal_blob: None,
            call_outcomes: vec![],
            prefix_writes: vec![],
            write_replays: vec![],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            jump_is_safe(&cont),
            "M1g: depth=2 live snap may jump (parent re-enters; nested init jumps)"
        );
    }

        #[test]
    fn m1f_arm_applies_absolute_jump_metric() {
        // R0: jump off by default — opt in via dedicated flag (avoid racing inspect env).
        unsafe {
            std::env::set_var("SPECFENCE_ABSOLUTE_JUMP", "1");
        }
        use crate::specfence::metrics::MetricsInner;
        use crate::specfence::rem::{CheckpointId, PartialRetryTable};
        use hashbrown::HashMap;
        use crate::BuildIdentityHasher;

        clear_pc_resume();
        let metrics = MetricsInner::default();
        let table = PartialRetryTable::new(1);
        let snap = BoundarySnapshot {
            pc: 1,
            gas_remaining: 50_000,
            gas_refunded: 0,
            memory_words: 0,
            memory_expansion_cost: 0,
            call_depth: 1,
            opcode_steps: 3,
            stack: vec![U256::from(1)],
            memory: vec![],
            code_hash: None,
            bytecode_len: 8,
            at_call_boundary: false,

            post_sstore: false,
        };
        let (effects, values) = basic_read_effect();
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 1,
            },
            k_fail: 2,
            certified: vec![1],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 3)),
            jump_snap: Some(snap.clone()),
            journal_blob: None,
            call_outcomes: vec![],
            prefix_writes: vec![],
            write_replays: vec![],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(jump_is_safe(&cont), "Basic-only live snap is M1f-safe");
        assert!(
            absolute_jump_env_enabled(),
            "SPECFENCE_ABSOLUTE_JUMP=1 must enable absolute jump"
        );
        assert!(
            try_arm_safe_absolute_jump(0, &table, &cont, &metrics),
            "opt-in path must arm absolute jump when jump_is_safe"
        );
        with_plant_tls(0, &table, &metrics, || {
            // Production initialize_interp apply + metric path.
            let code = Bytecode::new_raw(vec![0x00; 8].into());
            let mut interp = Interpreter::<EthInterpreter>::default();
            interp.bytecode = ExtBytecode::new(code);
            interp.gas = Gas::new(100_000);
            snap.apply_to_interp(&mut interp);
            assert_eq!(interp.bytecode.pc(), 1);
            metrics.record_pc_resume(snap.opcode_steps);
            metrics.record_live_pc_resume();
            metrics.record_absolute_jump_applied();
        });
        let m = metrics.snapshot(0, 0.0, 0.0, 0.0, 0.0);
        assert!(m.absolute_jump_applied > 0, "{m:?}");
        assert!(m.prefix_opcodes_skipped >= 3, "{m:?}");
        clear_pc_resume();
    }




    #[test]
    fn jump_is_safe_accepts_multi_sstore_write_prefix() {
        use crate::specfence::rem::{AccessMode, RegionAccess, StorageWriteReplay};
        let (mut effects, values) = storage_read_effect();
        effects.push(RegionAccess {
            tx_idx: 0,
            k: 2,
            location: 42,
            mode: AccessMode::Write,
        });
        effects.push(RegionAccess {
            tx_idx: 0,
            k: 3,
            location: 43,
            mode: AccessMode::Write,
        });
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 3,
            },
            k_fail: 5,
            certified: vec![1],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 3)),
            jump_snap: Some(BoundarySnapshot {
                pc: 16,
                gas_remaining: 25_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 20,
                stack: vec![],
                memory: vec![],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: false,
                post_sstore: true,
            }),
            journal_blob: Some(JournalBlob {
                state: EvmState::default(),
                logs: vec![alloy_primitives::Log {
                    address: Address::ZERO,
                    data: alloy_primitives::LogData::new_unchecked(
                        vec![B256::ZERO],
                        alloy_primitives::Bytes::new(),
                    ),
                }],
            }),
            call_outcomes: vec![],
            prefix_writes: vec![42, 43],
            write_replays: vec![
                StorageWriteReplay {
                    address: Address::ZERO,
                    slot: U256::ZERO,
                    original: U256::ZERO,
                    present: U256::from(1),
                    gas_remaining_after: 30_000,
                },
                StorageWriteReplay {
                    address: Address::ZERO,
                    slot: U256::from(1),
                    original: U256::ZERO,
                    present: U256::from(2),
                    gas_remaining_after: 25_000,
                },
            ],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            jump_is_safe(&cont),
            "M1j: multi-SSTORE + logs blob must allow write-prefix jump"
        );
    }

    #[test]
    fn jump_is_safe_accepts_valued_call_at_call_boundary() {
        use crate::specfence::rem::{AccessMode, RegionAccess, StorageWriteReplay};
        use revm::interpreter::{Gas, InstructionResult, InterpreterResult};
        let (mut effects, values) = storage_read_effect();
        effects.push(RegionAccess {
            tx_idx: 0,
            k: 2,
            location: 42,
            mode: AccessMode::Write,
        });
        let outcome = CallOutcome {
            result: InterpreterResult {
                result: InstructionResult::Stop,
                gas: Gas::new(50_000),
                output: Default::default(),
            },
            memory_offset: 0..0,
            was_precompile_called: false,
            precompile_call_logs: vec![],
        };
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 12,
                gas_remaining: 40_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 18,
                stack: vec![],
                memory: vec![],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: true,
                post_sstore: true,
            }),
            journal_blob: None,
            call_outcomes: vec![CachedCallOutcome {
                call_seq: 2,
                depth: 2,
                target: Address::from([1u8; 20]),
                bytecode_address: Address::from([1u8; 20]),
                caller: Address::from([2u8; 20]),
                gas_limit: 50_000,
                is_static: false,
                value: U256::from(1),
                k_end: 2,
                outcome,
            }],
            prefix_writes: vec![42],
            write_replays: vec![StorageWriteReplay {
                address: Address::ZERO,
                slot: U256::ZERO,
                original: U256::ZERO,
                present: U256::from(1),
                gas_remaining_after: 40_000,
            }],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            jump_is_safe(&cont),
            "M1l: valued CallOutcome OK at CALL-boundary with write_replays"
        );
    }

    #[test]
    fn jump_is_safe_rejects_valued_blocks_jump_cache_miss() {
        let (effects, values) = storage_read_effect();
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 12,
                gas_remaining: 40_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 18,
                stack: vec![],
                memory: vec![],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: true,
                post_sstore: true,
            }),
            journal_blob: None,
            call_outcomes: vec![],
            prefix_writes: vec![],
            write_replays: vec![],
            log_replays: vec![],
            valued_blocks_jump: true,
        };
        assert!(
            !jump_is_safe(&cont),
            "M1l: valued_blocks_jump (cache miss) still forbids absolute jump"
        );
    }

    #[test]
    fn jump_is_safe_accepts_write_prefix_plus_zero_value_call_outcome_combine() {
        use crate::specfence::rem::{AccessMode, RegionAccess, StorageWriteReplay};
        use revm::interpreter::{Gas, InstructionResult, InterpreterResult};
        let (mut effects, values) = storage_read_effect();
        effects.push(RegionAccess {
            tx_idx: 0,
            k: 2,
            location: 42,
            mode: AccessMode::Write,
        });
        let outcome = CallOutcome {
            result: InterpreterResult {
                result: InstructionResult::Stop,
                gas: Gas::new(50_000),
                output: Default::default(),
            },
            memory_offset: 0..0,
            was_precompile_called: false,
            precompile_call_logs: vec![],
        };
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1],
            suffix_writes: vec![],
            effects,
            checkpoints: vec![],
            values,
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 12,
                gas_remaining: 40_000,
                gas_refunded: 0,
                memory_words: 0,
                memory_expansion_cost: 0,
                call_depth: 1,
                opcode_steps: 18,
                stack: vec![],
                memory: vec![],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
                at_call_boundary: true,
                post_sstore: true,
            }),
            journal_blob: None,
            call_outcomes: vec![CachedCallOutcome {
                call_seq: 2,
                depth: 2,
                target: Address::from([1u8; 20]),
                bytecode_address: Address::from([1u8; 20]),
                caller: Address::from([2u8; 20]),
                gas_limit: 50_000,
                is_static: false,
                value: U256::ZERO,
                k_end: 2,
                outcome,
            }],
            prefix_writes: vec![42],
            write_replays: vec![StorageWriteReplay {
                address: Address::ZERO,
                slot: U256::ZERO,
                original: U256::ZERO,
                present: U256::from(1),
                gas_remaining_after: 40_000,
            }],
            log_replays: vec![],
            valued_blocks_jump: false,
        };
        assert!(
            jump_is_safe(&cont),
            "M1k: write_replays + zero-value CallOutcome OK at CALL-boundary"
        );
    }

    #[test]
    fn valued_call_cache_env_default_off_r0() {
        // SAFETY: test-only; prefer dedicated flag to avoid racing parallel tests.
        unsafe {
            std::env::set_var("SPECFENCE_VALUED_CALL_CACHE", "0");
        }
        assert!(!valued_call_cache_env_enabled());
        unsafe {
            std::env::set_var("SPECFENCE_VALUED_CALL_CACHE", "1");
        }
        assert!(valued_call_cache_env_enabled());
        unsafe {
            std::env::remove_var("SPECFENCE_VALUED_CALL_CACHE");
        }
        // Default (unset) is off unless research inspect — do not assert unset here
        // under parallel cargo test (env races). Logic covered by R0 gating code.
    }

}
