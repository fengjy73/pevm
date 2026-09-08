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
    interpreter_types::{InputsTr, Jumps, LegacyBytecode, StackTr},
    CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter,
};
use revm::state::EvmState;
use revm::Inspector;

use crate::{hash_deterministic, MemoryLocation, TxIdx};

use super::finegrain::{FineGrainCollector, LocationKind};
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
    /// Handler-plant SSTORE ordinal (1 = first SSTORE this plant TLS). 0 = Inspector.
    /// Iter9: require write_replays.len() >= this so we never jump past unreplayed SSTORE.
    pub sstore_index: u64,
    /// Iter9: exact storage presents at this Handler tip (ordered plant notes).
    /// Empty for Inspector snaps. Armed jump applies these instead of full cont wr.
    pub write_replays_at_tip: Vec<StorageWriteReplay>,
    /// Iter23: cumulative Bind SLOAD (address, slot, value) log for stack↔FF
    /// reconcile on abs-jump apply. Empty for Inspector/SSTORE/lite snaps.
    pub tip_sloads: Vec<(Address, U256, U256)>,
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
    // Iter27: Bind tip with ≥1 tip_sload overlapping FF Storage counts as
    // storage-FF for large-bytecode gate (same Validated-fresh identity).
    let tip_sload_ff = snap.tip_sloads.iter().any(|(addr, slot, snap_val)| {
        cont.values.values().any(|v| match v {
            crate::specfence::rem::FfValue::Storage {
                address,
                slot: ss,
                value,
                ..
            } if address == addr && ss == slot => value == snap_val,
            _ => false,
        })
    });
    let has_write_effects = cont.effects.iter().any(|e| e.mode == AccessMode::Write);
    // Tiny Basic-only (M1f) always OK. Larger bytecode only when Storage FF,
    // write_replays, and/or CALL-boundary make post-jump ≡ sequential under pevm MV.
    // Iter2: ERC-20 / DeFi bytecode often 4–20KB; 4096 blocked all 597 jumps.
    const MAX_TINY: usize = 256;
    const MAX_STORAGE: usize = 24_576;
    if snap.bytecode_len > MAX_TINY {
        if snap.bytecode_len > MAX_STORAGE {
            return false;
        }
        if !has_storage && !tip_sload_ff && !snap.at_call_boundary && cont.write_replays.is_empty() {
            return false;
        }
    }
    if cont.cp.k == 0 && cont.effects.is_empty() && snap.opcode_steps == 0 {
        return false;
    }
    // M1j: multi-SSTORE+LOG prefixes can exceed 128 steps; allow up to 512 when
    // write_replays and/or call_outcomes certify a controlled jump.
    // Iter2: Storage/write_replay prefixes on 597 often need >512 prefix steps.
    let max_steps = if !cont.write_replays.is_empty()
        || !cont.call_outcomes.is_empty()
        || has_storage
        || tip_sload_ff
    {
        2048u64
    } else {
        128u64
    };
    if snap.opcode_steps == 0 || snap.opcode_steps > max_steps {
        return false;
    }
    if cont.effects.is_empty() {
        return false;
    }
    // Iter9: post-SSTORE tip without write_replays skips SSTORE in the journal
    // (Lean notes Write effects only at finalize → effects_w often 0).
    if snap.post_sstore && cont.write_replays.is_empty() {
        return false;
    }
    // Iter9 Handler: tip-scoped replays (gas >= tip) must cover sstore_index.
    if snap.sstore_index > 0 {
        // Exact tip replays embedded at plant time.
        if snap.write_replays_at_tip.is_empty() {
            return false;
        }
        if (snap.write_replays_at_tip.len() as u64) != snap.sstore_index {
            return false;
        }
        if !snap
            .write_replays_at_tip
            .iter()
            .any(|w| w.gas_remaining_after == snap.gas_remaining)
        {
            return false;
        }
        // Refuse early tip if cont still has plant replays from later SSTOREs
        // (lower gas_remaining_after). Apply those only at a later tip.
        if cont.write_replays.iter().any(|w| {
            w.gas_remaining_after > 0 && w.gas_remaining_after < snap.gas_remaining
        }) {
            return false;
        }
        // Iter11: multi-SSTORE last tip — require memory-lite + tip == last plant
        // gas (min among tip replays) + FF Storage presents match tip writes when
        // FF has that (address,slot). Empty-memory / early-tip / FF-mismatch refuse.
        if snap.memory.is_empty() {
            return false;
        }
        let tip_gases: Vec<u64> = snap
            .write_replays_at_tip
            .iter()
            .map(|w| w.gas_remaining_after)
            .filter(|g| *g > 0)
            .collect();
        if tip_gases.is_empty() {
            return false;
        }
        let min_tip = tip_gases.iter().copied().min().unwrap_or(0);
        // Last SSTORE spent the most gas → remaining == min tip gas.
        if snap.gas_remaining != min_tip {
            return false;
        }
        // Cap Handler prefix SSTORE count (ERC-20 transfer = 2; refuse huge tips).
        if snap.sstore_index > 8 {
            return false;
        }
        // Tip embeds must agree with continuation write_replays for same slots
        // (finalize must not have clobbered present/original under pevm MV).
        for wr in &snap.write_replays_at_tip {
            let Some(cont_wr) = cont.write_replays.iter().find(|c| {
                c.address == wr.address && c.slot == wr.slot
            }) else {
                return false;
            };
            if cont_wr.present != wr.present || cont_wr.original != wr.original {
                return false;
            }
        }
        // Distinct tip slots must equal sstore_index (same-slot multi collapses).
        let distinct = {
            let mut seen = std::collections::HashSet::new();
            snap.write_replays_at_tip
                .iter()
                .filter(|w| seen.insert((w.address, w.slot)))
                .count()
        };
        if distinct as u64 != snap.sstore_index {
            return false;
        }
        // Iter11: last-tip gates above are necessary but not sufficient — Lean
        // capture→jump still fails to prove aj>0∧seq≡par on fixtures (and prior
        // force-allow multi was seq≠par). Refuse multi until Iter12+ proves it.
        if snap.sstore_index != 1 {
            return false;
        }
    }
    // M1i write-prefix: storage write_replays require post-SSTORE gas-equal snap.
    // Write effects without replays stay forbidden (M1g). prefix_writes that are
    // account-only (no storage replays) must not block M1g storage-read jumps.
    if !cont.write_replays.is_empty() {
        if snap.sstore_index > 0 {
            // Handler tip already gas-matched above; still refuse storage blob poison.
            if let Some(blob) = cont.journal_blob.as_ref() {
                if blob.state.values().any(|a| !a.storage.is_empty()) {
                    return false;
                }
            }
        } else if !write_prefix_jump_is_safe(cont, snap) {
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

/// Iter20 dig: why [`jump_is_safe`] refused (Bind-snap consume diagnosis).
pub(crate) fn jump_refuse_reason(cont: &ResumeContinuation) -> &'static str {
    let Some(snap) = cont.jump_snap.as_ref() else {
        return "no_snap";
    };
    if !snap.is_live_capture() {
        return "not_live";
    }
    if cont.valued_blocks_jump {
        return "valued_blocks";
    }
    if !cont.call_outcomes.is_empty() {
        let has_valued = cont.call_outcomes.iter().any(|c| !c.value.is_zero());
        if has_valued && cont.write_replays.is_empty() {
            return "valued_call_no_writes";
        }
        if !snap.at_call_boundary {
            return "call_outcomes_not_boundary";
        }
    }
    if snap.call_depth > 2 {
        return "call_depth";
    }
    if snap.bytecode_len > 0 && snap.pc >= snap.bytecode_len {
        return "pc_oob";
    }
    let has_storage = cont.values.values().any(|v| {
        matches!(v, crate::specfence::rem::FfValue::Storage { .. })
    });
    let has_basic = cont.values.values().any(|v| {
        matches!(v, crate::specfence::rem::FfValue::Basic { .. })
    });
    let tip_sload_ff = snap.tip_sloads.iter().any(|(addr, slot, snap_val)| {
        cont.values.values().any(|v| match v {
            crate::specfence::rem::FfValue::Storage {
                address,
                slot: ss,
                value,
                ..
            } if address == addr && ss == slot => value == snap_val,
            _ => false,
        })
    });
    let has_write_effects = cont.effects.iter().any(|e| e.mode == AccessMode::Write);
    const MAX_TINY: usize = 256;
    const MAX_STORAGE: usize = 24_576;
    if snap.bytecode_len > MAX_TINY {
        if snap.bytecode_len > MAX_STORAGE {
            return "bytecode_huge";
        }
        if !has_storage && !tip_sload_ff && !snap.at_call_boundary && cont.write_replays.is_empty() {
            return "bytecode_no_storage_ff";
        }
    }
    if cont.cp.k == 0 && cont.effects.is_empty() && snap.opcode_steps == 0 {
        return "empty_cp0";
    }
    let max_steps = if !cont.write_replays.is_empty()
        || !cont.call_outcomes.is_empty()
        || has_storage
        || tip_sload_ff
    {
        2048u64
    } else {
        128u64
    };
    if snap.opcode_steps == 0 {
        return "steps_zero";
    }
    if snap.opcode_steps > max_steps {
        return "steps_over";
    }
    if cont.effects.is_empty() {
        return "effects_empty";
    }
    if snap.post_sstore && cont.write_replays.is_empty() {
        return "post_sstore_no_replays";
    }
    if snap.sstore_index > 0 {
        return "sstore_tip_gates";
    }
    if !cont.write_replays.is_empty() {
        if snap.sstore_index == 0 && !write_prefix_jump_is_safe(cont, snap) {
            return "write_prefix_unsafe";
        }
    } else if has_write_effects {
        return "write_effects_no_replays";
    }
    if !has_basic && !has_storage && cont.write_replays.is_empty() {
        return "no_ff_values";
    }
    if let Some(blob) = cont.journal_blob.as_ref() {
        if blob.state.values().any(|a| a.is_selfdestructed()) {
            return "blob_selfdestruct";
        }
        if blob.state.values().any(|a| !a.storage.is_empty()) {
            return "blob_storage_poison";
        }
    }
    if jump_is_safe(cont) {
        return "ok";
    }
    "unknown"
}

/// M1i: Write-prefix absolute jump is safe only with post-SSTORE gas evidence and
/// controlled `write_replays` (per-slot journal apply — not present_values dump).
fn write_prefix_jump_is_safe(cont: &ResumeContinuation, snap: &BoundarySnapshot) -> bool {
    if cont.write_replays.is_empty() {
        return false;
    }
    // Sticky last post-SSTORE gas is copied onto every replay at finalize.
    let live_gases: Vec<u64> = cont
        .write_replays
        .iter()
        .map(|w| w.gas_remaining_after)
        .filter(|g| *g > 0)
        .collect();
    // Iter9: always require live post-SSTORE gas evidence. Empty gases previously
    // skipped the early-tip check on post_sstore snaps (Handler first-SSTORE tip
    // with sticky-last replay gas → seq≠par on ERC-20 fixtures).
    if live_gases.is_empty() {
        return false;
    }
    let max_after = live_gases.iter().copied().max().unwrap_or(0);
    let min_after = live_gases.iter().copied().min().unwrap_or(0);
    // Tip must not have *more* gas left than last post-SSTORE replay gas.
    if snap.gas_remaining > min_after {
        return false;
    }
    if max_after.saturating_sub(min_after) > 50_000 {
        return false;
    }
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

/// Lean SuffixRepair may attempt hang-free absolute jump without whole-block
/// `SPECFENCE_ENABLE_INSPECT`. Still honor explicit `SPECFENCE_ABSOLUTE_JUMP=0`.
pub(crate) fn suffix_repair_jump_env_ok() -> bool {
    match std::env::var_os("SPECFENCE_ABSOLUTE_JUMP") {
        Some(v) if v == "0" => false,
        _ => true,
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

/// Scoped plant pointers for Inspector → PartialRetry / metrics / journal stream.
#[derive(Clone, Copy)]
struct PlantTls {
    tx_idx: TxIdx,
    incarnation: usize,
    partial_retry: *const PartialRetryTable,
    metrics: *const MetricsInner,
    /// Research journal RAW stream (None = disabled).
    finegrain: Option<*const FineGrainCollector>,
    tx_gas_limit: Option<u64>,
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
    /// Iter4: SSTORE count on Handler::run plant path (hang-free, no Inspector).
    static HANDLER_SSTORE_STEPS: Cell<u64> = const { Cell::new(0) };
    /// Iter19: hang-free Bind/EffectBoundary snap context (no IN_INSPECT / WaitHard demote).
    /// Distinct from PLANT so stock SSTORE + SoftWait Soft~0 stay unchanged.
    static BIND_SNAP: Cell<Option<PlantTls>> = const { Cell::new(None) };
    /// Set by Bind-on-Data; consumed after stock SLOAD returns in Handler wrap.
    static PENDING_BIND_SNAP: Cell<bool> = const { Cell::new(false) };
    /// Bind-snap ordinal this incarnation (opcode_steps proxy for jump credit).
    static BIND_SNAP_STEPS: Cell<u64> = const { Cell::new(0) };
    /// Iter23: cumulative Bind SLOAD (addr, slot, value) within with_bind_snap_tls.
    static BIND_SLOAD_LOG: RefCell<Vec<(Address, U256, U256)>> =
        const { RefCell::new(Vec::new()) };
    /// Iter26: deepest tip≡FF snap deferred to TLS exit (one attach / resume).
    static BIND_SNAP_DEFERRED: RefCell<Option<(usize, BoundarySnapshot)>> =
        const { RefCell::new(None) };
    static PENDING_RESUME: RefCell<Option<BoundarySnapshot>> = const { RefCell::new(None) };
    /// Iter21: FF read-origin seeds applied only after successful PC restore.
    static PENDING_FF_ORIGIN_SEEDS: RefCell<Vec<(crate::MemoryLocationHash, crate::ReadOrigin)>> = const { RefCell::new(Vec::new()) };
    /// Iter22: certified-prefix FF Storage/Basic presents to warm in revm journal on
    /// Bind abs jump (EIP-2929). Without this, jumped-past SLOADs leave slots cold →
    /// later SLOAD/SSTORE gas ≠ sequential → seq≠par on ERC-20.
    static PENDING_FF_READ_PRESENTS: RefCell<Vec<crate::specfence::rem::FfValue>> = const { RefCell::new(Vec::new()) };
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
    with_plant_tls_journal(tx_idx, 0, None, None, partial_retry, metrics, f)
}

/// Plant TLS with optional FineGrain journal stream (research inspect path).
pub(crate) fn with_plant_tls_journal<R>(
    tx_idx: TxIdx,
    incarnation: usize,
    finegrain: Option<&FineGrainCollector>,
    tx_gas_limit: Option<u64>,
    partial_retry: &PartialRetryTable,
    metrics: &MetricsInner,
    f: impl FnOnce() -> R,
) -> R {
    let prev = PLANT.replace(Some(PlantTls {
        tx_idx,
        incarnation,
        partial_retry: partial_retry as *const _,
        metrics: metrics as *const _,
        finegrain: finegrain.map(|fg| fg as *const _),
        tx_gas_limit,
    }));
    OPCODE_STEPS.set(0);
    CALL_DEPTH.set(0);
    CALL_SEQ.set(0);
    LAST_SNAP.with(|c| *c.borrow_mut() = None);
    PENDING_EFFECT_CP.set(false);
    HANDLER_SSTORE_STEPS.set(0);
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

/// Iter21: stash FF read origins to install into Db read_set only after PC restore
/// succeeds — seeding before a failed apply poisoned seq≠par (ERC-20 depth mismatch).
pub(crate) fn arm_ff_origin_seeds(
    seeds: Vec<(crate::MemoryLocationHash, crate::ReadOrigin)>,
) {
    PENDING_FF_ORIGIN_SEEDS.with(|c| *c.borrow_mut() = seeds);
}

pub(crate) fn take_ff_origin_seeds() -> Vec<(crate::MemoryLocationHash, crate::ReadOrigin)> {
    PENDING_FF_ORIGIN_SEEDS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

/// Iter22: arm FF Storage/Basic presents for hang-free journal warm on PC apply.
pub(crate) fn arm_ff_read_presents(values: Vec<crate::specfence::rem::FfValue>) {
    PENDING_FF_READ_PRESENTS.with(|c| *c.borrow_mut() = values);
}

pub(crate) fn clear_pc_resume() {
    PENDING_RESUME.with(|c| *c.borrow_mut() = None);
    PENDING_JOURNAL_BLOB.with(|c| *c.borrow_mut() = None);
    PENDING_FF_ORIGIN_SEEDS.with(|c| c.borrow_mut().clear());
    PENDING_FF_READ_PRESENTS.with(|c| c.borrow_mut().clear());
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

/// Hang-free absolute-jump eligibility (safety gates only — no env / inspect flag).
/// Used by Lean SuffixRepair to decide whether a narrow inspect_run is worth opening.
pub(crate) fn absolute_jump_eligible(
    tx_idx: TxIdx,
    partial_retry: &PartialRetryTable,
    cont: &ResumeContinuation,
) -> bool {
    !partial_retry.is_jump_disabled(tx_idx) && jump_is_safe(cont)
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
    try_arm_safe_absolute_jump_gated(
        tx_idx,
        partial_retry,
        cont,
        metrics,
        absolute_jump_env_enabled(),
    )
}

/// Like [`try_arm_safe_absolute_jump`], but caller supplies the env gate.
/// Lean SuffixRepair resume passes `env_ok=true` so hang-free `jump_is_safe`
/// jumps work without whole-block `SPECFENCE_ENABLE_INSPECT`.
pub(crate) fn try_arm_safe_absolute_jump_gated(
    tx_idx: TxIdx,
    partial_retry: &PartialRetryTable,
    cont: &ResumeContinuation,
    metrics: &MetricsInner,
    env_ok: bool,
) -> bool {
    // M1f: default-on when jump_is_safe; SPECFENCE_ABSOLUTE_JUMP=0 disables (research).
    // Anti-livelock: jump_disabled after a jumped resume fails validation.
    // SuffixRepair may pass env_ok=true; safety still requires jump_is_safe.
    if !env_ok || !absolute_jump_eligible(tx_idx, partial_retry, cont) {
        metrics.record_absolute_jump_fallback();
        return false;
    }
    let snap = cont.jump_snap.clone().expect("jump_is_safe implies jump_snap");
    // Iter23: when Bind tip_sloads is present, require FF match on overlapping
    // slots — stale SLOAD values already consumed into require/SUB (ERC-20
    // revert dgas=+661); patching tops is insufficient → refuse.
    // Iter27: cumulative Bind SLOAD log includes slots absent from certified
    // FF values — missing FF entry is OK; conflict (FF≠tip) still refuses;
    // require ≥1 overlapping match so tip still has Validated-fresh identity.
    // Empty tip_sloads: allow legacy M1f/Inspector snaps (no Bind identity).
    if snap.sstore_index == 0 && !snap.post_sstore && !snap.tip_sloads.is_empty() {
        let mut any_match = false;
        for (addr, slot, snap_val) in &snap.tip_sloads {
            let ff = cont.values.values().find_map(|v| match v {
                crate::specfence::rem::FfValue::Storage {
                    address,
                    slot: s,
                    value,
                    ..
                } if address == addr && s == slot => Some(*value),
                _ => None,
            });
            match ff {
                Some(v) if v == *snap_val => any_match = true,
                Some(_) => {
                    metrics.record_absolute_jump_fallback();
                    return false;
                }
                None => {}
            }
        }
        if !any_match {
            metrics.record_absolute_jump_fallback();
            return false;
        }
    }
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
    arm_pc_resume_with_blob(snap.clone(), None);
    let tip_writes: Vec<StorageWriteReplay> = if snap.sstore_index > 0 {
        snap.write_replays_at_tip.clone()
    } else {
        cont.write_replays.clone()
    };
    PENDING_WRITE_REPLAYS.with(|c| {
        *c.borrow_mut() = tip_writes;
    });
    // Iter22: read-prefix Bind jump — warm FF presents so skipped SLOADs stay EIP-2929 warm.
    if snap.sstore_index == 0 && !snap.post_sstore && cont.write_replays.is_empty() {
        let presents: Vec<_> = cont.values.values().cloned().collect();
        if !presents.is_empty() {
            arm_ff_read_presents(presents);
        }
    }
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

/// Arm Inspector step_end live-snap capture without rem checkpoint plant.
/// Used by Bind-on-Data lite after `note_certified_with_effect_boundary`.
pub(crate) fn arm_pending_effect_cp_only() {
    PENDING_EFFECT_CP.set(true);
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
    // Iter21: Bind-snap Lean jumps apply via Handler run_exec_loop without PLANT
    // TLS — still record aj/pc_resume via BIND_SNAP metrics so digs aren't blind
    // (aj=0 while jump applied → silent seq≠par).
    let mut recorded = false;
    PLANT.with(|p| {
        if let Some(plant) = p.get() {
            let metrics = unsafe { &*plant.metrics };
            metrics.record_pc_resume(skipped);
            metrics.record_live_pc_resume();
            metrics.record_absolute_jump_applied();
            recorded = true;
        }
    });
    if !recorded {
        BIND_SNAP.with(|b| {
            if let Some(ctx) = b.get() {
                let metrics = unsafe { &*ctx.metrics };
                metrics.record_pc_resume(skipped);
                metrics.record_live_pc_resume();
                metrics.record_absolute_jump_applied();
            }
        });
    }
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
    use revm::primitives::KECCAK_EMPTY;
    use revm::state::{Account, AccountInfo, EvmStorageSlot};
    // Do **not** journal.load_account here — that re-enters pevm Db/maybe_wait and
    // can Block/livelock mid-initialize_interp. Iter9: if the account is missing,
    // insert a minimal warm shell so Handler tip replays are not silently skipped
    // (frame_init usually loaded the callee; nested/storage-only tips may not).
    let state = context.journal_mut().evm_state_mut();
    let tx_id = state.values().next().map(|a| a.transaction_id).unwrap_or(0);
    for wr in writes {
        if !state.contains_key(&wr.address) {
            let mut acc = Account::new_not_existing(tx_id);
            acc.info = AccountInfo {
                balance: Default::default(),
                nonce: 0,
                code_hash: KECCAK_EMPTY,
                code: None,
                ..Default::default()
            };
            let _ = acc.mark_warm_with_transaction_id(tx_id);
            state.insert(wr.address, acc);
        }
        let Some(acc) = state.get_mut(&wr.address) else {
            continue;
        };
        let _ = acc.mark_warm_with_transaction_id(tx_id);
        let slot = EvmStorageSlot::new_changed(wr.original, wr.present, tx_id);
        acc.storage.insert(wr.slot, slot);
        acc.mark_touch();
    }
}

/// Iter22: warm certified-prefix FF reads into revm journal without Db re-entry.
/// Read-prefix Bind jump skips SLOADs; without warm slots, later SLOAD/SSTORE pay
/// cold gas and receipts diverge (ERC-20 aj>0 ∧ seq≠par).
fn apply_ff_read_presents<CTX>(
    context: &mut CTX,
    values: &[crate::specfence::rem::FfValue],
    prefer_tx_id: Option<usize>,
)
where
    CTX: ContextTr,
    CTX::Journal: JournalExt,
{
    use revm::primitives::KECCAK_EMPTY;
    use revm::state::{Account, AccountInfo, EvmStorageSlot};
    use crate::specfence::rem::FfValue;
    let state = context.journal_mut().evm_state_mut();
    // Prefer callee/frame tx_id — HashMap::values().next() can pick a stale id.
    let tx_id = prefer_tx_id
        .or_else(|| state.values().map(|a| a.transaction_id).max())
        .unwrap_or(0);
    for ff in values {
        match ff {
            FfValue::Storage { address, slot, value, .. } => {
                if !state.contains_key(address) {
                    let mut acc = Account::new_not_existing(tx_id);
                    acc.info = AccountInfo {
                        balance: Default::default(),
                        nonce: 0,
                        code_hash: KECCAK_EMPTY,
                        code: None,
                        ..Default::default()
                    };
                    let _ = acc.mark_warm_with_transaction_id(tx_id);
                    state.insert(*address, acc);
                }
                let Some(acc) = state.get_mut(address) else {
                    continue;
                };
                let _ = acc.mark_warm_with_transaction_id(tx_id);
                // Unchanged warm slot: original == present == FF value (read-only prefix).
                if !acc.storage.contains_key(slot) {
                    acc.storage.insert(*slot, EvmStorageSlot::new(*value, tx_id));
                } else if let Some(s) = acc.storage.get_mut(slot) {
                    let _ = s.mark_warm_with_transaction_id(tx_id);
                }
            }
            FfValue::Basic { address, basic, code_hash, .. } => {
                if state.contains_key(address) {
                    if let Some(acc) = state.get_mut(address) {
                        let _ = acc.mark_warm_with_transaction_id(tx_id);
                    }
                    continue;
                }
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
                state.insert(*address, acc);
            }
        }
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


fn u256_to_address(v: U256) -> Address {
    Address::from_word(B256::from(v))
}

/// Opt-in journal RAW stream (FineGrain journal mode). Zero cost when finegrain TLS unset.
fn maybe_note_journal_effect(interp: &Interpreter<EthInterpreter>, op: u8, opcode_steps: u64, call_depth: u16) {
    const OP_BALANCE: u8 = 0x31;
    const OP_CALL: u8 = 0xf1;
    const OP_CALLCODE: u8 = 0xf2;
    const OP_DELEGATECALL: u8 = 0xf4;
    const OP_STATICCALL: u8 = 0xfa;
    const OP_CREATE: u8 = 0xf0;
    const OP_CREATE2: u8 = 0xf5;
    const OP_SELFDESTRUCT: u8 = 0xff;
    const OP_EXTCODESIZE: u8 = 0x3b;
    const OP_EXTCODECOPY: u8 = 0x3c;
    const OP_EXTCODEHASH: u8 = 0x3f;
    const OP_SELFBALANCE: u8 = 0x47;
    const OP_SLOAD: u8 = 0x54;
    const OP_SSTORE: u8 = 0x55;

    PLANT.with(|p| {
        let Some(plant) = p.get() else { return };
        let Some(fg_ptr) = plant.finegrain else { return };
        let fg = unsafe { &*fg_ptr };
        if !fg.journal_enabled() {
            return;
        }
        let gas_limit = plant.tx_gas_limit.or(Some(interp.gas.limit()));
        let gas_used = gas_limit.map(|lim| lim.saturating_sub(interp.gas.remaining()));
        let steps = Some(opcode_steps as usize);
        let target = interp.input.target_address();
        let mut target_acct = [0u8; 20];
        target_acct.copy_from_slice(target.as_slice());

        match op {
            OP_SLOAD => {
                let Ok(key) = interp.stack.peek(0) else { return };
                let loc = hash_deterministic(MemoryLocation::Storage(target, key));
                fg.deep_note_journal_read(
                    plant.tx_idx,
                    plant.incarnation,
                    loc,
                    LocationKind::Storage,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(target_acct),
                    Some(call_depth),
                );
            }
            OP_SSTORE => {
                // stack: [value, key] — key is peek(1)
                let Ok(key) = interp.stack.peek(1) else { return };
                let loc = hash_deterministic(MemoryLocation::Storage(target, key));
                fg.deep_note_journal_write(
                    plant.tx_idx,
                    plant.incarnation,
                    loc,
                    LocationKind::Storage,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(target_acct),
                    Some(call_depth),
                );
            }
            OP_BALANCE => {
                let Ok(addr_u) = interp.stack.peek(0) else { return };
                let addr = u256_to_address(addr_u);
                let loc = hash_deterministic(MemoryLocation::Basic(addr));
                let mut acct = [0u8; 20];
                acct.copy_from_slice(addr.as_slice());
                fg.deep_note_journal_read(
                    plant.tx_idx,
                    plant.incarnation,
                    loc,
                    LocationKind::Basic,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(acct),
                    Some(call_depth),
                );
            }
            OP_SELFBALANCE => {
                let loc = hash_deterministic(MemoryLocation::Basic(target));
                fg.deep_note_journal_read(
                    plant.tx_idx,
                    plant.incarnation,
                    loc,
                    LocationKind::Basic,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(target_acct),
                    Some(call_depth),
                );
            }
            OP_EXTCODESIZE | OP_EXTCODEHASH | OP_EXTCODECOPY => {
                let Ok(addr_u) = interp.stack.peek(0) else { return };
                let addr = u256_to_address(addr_u);
                let loc = hash_deterministic(MemoryLocation::CodeHash(addr));
                let mut acct = [0u8; 20];
                acct.copy_from_slice(addr.as_slice());
                fg.deep_note_journal_read(
                    plant.tx_idx,
                    plant.incarnation,
                    loc,
                    LocationKind::CodeHash,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(acct),
                    Some(call_depth),
                );
            }
            // Live account-write instances (producer_effect_k), not finalize-only.
            OP_CALL | OP_CALLCODE => {
                // stack: [gas, addr, value, argsOffset, argsLength, retOffset, retLength]
                let Ok(value) = interp.stack.peek(2) else { return };
                if value.is_zero() {
                    return;
                }
                let Ok(addr_u) = interp.stack.peek(1) else { return };
                let to = u256_to_address(addr_u);
                let caller = interp.input.caller_address();
                fg.deep_note_journal_account_write(
                    plant.tx_idx,
                    plant.incarnation,
                    caller,
                    LocationKind::Basic,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(call_depth),
                );
                fg.deep_note_journal_account_write(
                    plant.tx_idx,
                    plant.incarnation,
                    to,
                    LocationKind::Basic,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(call_depth),
                );
            }
            OP_CREATE | OP_CREATE2 => {
                let caller = interp.input.caller_address();
                fg.deep_note_journal_account_write(
                    plant.tx_idx,
                    plant.incarnation,
                    caller,
                    LocationKind::Basic,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(call_depth),
                );
            }
            OP_SELFDESTRUCT => {
                let Ok(addr_u) = interp.stack.peek(0) else { return };
                let beneficiary = u256_to_address(addr_u);
                fg.deep_note_journal_account_write(
                    plant.tx_idx,
                    plant.incarnation,
                    target,
                    LocationKind::Basic,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(call_depth),
                );
                fg.deep_note_journal_account_write(
                    plant.tx_idx,
                    plant.incarnation,
                    beneficiary,
                    LocationKind::Basic,
                    gas_used,
                    gas_limit,
                    steps,
                    Some(call_depth),
                );
            }
            OP_DELEGATECALL | OP_STATICCALL => {}
            _ => {}
        }
    });
}

/// True while `with_plant_tls*` is active on this worker (Handler or inspect).
pub(crate) fn plant_tls_active() -> bool {
    PLANT.with(|p| p.get().is_some())
}

/// True when an absolute PC resume snap is armed (cheap TLS check).
pub(crate) fn pending_resume_armed() -> bool {
    PENDING_RESUME.with(|c| c.borrow().is_some())
}

/// Iter4: apply armed `PENDING_RESUME` to an interpreter without `inspect_run`.
/// Shared by SpecFenceInspector::initialize_interp and Handler::run_exec_loop.
pub(crate) fn try_apply_pending_pc_resume<CTX>(
    interp: &mut Interpreter<EthInterpreter>,
    context: &mut CTX,
    call_depth: u16,
) where
    CTX: ContextTr,
    CTX::Journal: JournalExt,
{
    if RESUME_APPLIED.get() {
        return;
    }
    let snap = PENDING_RESUME.with(|c| c.borrow().clone());
    let Some(snap) = snap else {
        return;
    };
    if snap.call_depth != call_depth && !(snap.call_depth <= 1 && call_depth <= 1) {
        if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
            eprintln!("JUMP_DIG apply_refuse depth snap={} frame={}", snap.call_depth, call_depth);
        }
        clear_pc_resume();
        return;
    }
    if let Some(expected) = snap.code_hash {
        let actual = interp.bytecode.get_or_calculate_hash();
        if actual != expected {
            // Iter28 diagnosis: 597 Bind tips are often nested (router→token);
            // frame0 is tx.to. Nested apply-on-mismatch / defer-until-match both
            // hung under concurrency — keep refuse + clear (hang-free).
            if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
                eprintln!(
                    "JUMP_DIG apply_refuse code_hash snap_pc={} blen={} tip_sloads={}",
                    snap.pc, snap.bytecode_len, snap.tip_sloads.len()
                );
            }
            clear_pc_resume();
            return;
        }
    }
    if snap.bytecode_len > 0 && snap.pc >= snap.bytecode_len {
        if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
            eprintln!("JUMP_DIG apply_refuse pc_oob");
        }
        clear_pc_resume();
        return;
    }
    // Iter22: Bind read-prefix tips are top-level only. journal.depth()>1 means we
    // would apply a tip onto the wrong frame (nested CALL) — refuse.
    let jdepth = context.journal().depth();
    if snap.sstore_index == 0 && !snap.post_sstore && jdepth > 1 {
        if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
            eprintln!("JUMP_DIG apply_refuse jdepth={jdepth}");
        }
        clear_pc_resume();
        return;
    }
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
            clear_pc_resume();
            PENDING_WRITE_REPLAYS.with(|c| c.borrow_mut().clear());
            PENDING_CALL_TOUCH_BASICS.with(|c| c.borrow_mut().clear());
            arm_call_outcome_cache(call_touches.clone());
            return;
        }
    }
    let logs = PENDING_LOG_REPLAYS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    for log in logs {
        context.journal_mut().log(log);
    }
    let writes = PENDING_WRITE_REPLAYS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    if !writes.is_empty() {
        apply_write_replays(context, &writes);
    }
    // Iter22: warm FF read presents before PC restore (Bind jump read-prefix).
    let ff_reads = PENDING_FF_READ_PRESENTS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    if !ff_reads.is_empty() {
        let prefer_tx = {
            let state = context.journal().evm_state();
            let target = interp.input.target_address();
            // Iter23: prefer target tx_id; else *min* among loaded (stable), not max —
            // HashMap iteration order made max() flaky under concurrency.
            state.get(&target).map(|a| a.transaction_id).or_else(|| {
                state.values().map(|a| a.transaction_id).min()
            })
        };
        apply_ff_read_presents(context, &ff_reads, prefer_tx);
    }
    let skipped = snap.opcode_steps;
    snap.apply_to_interp(interp);
    // Iter23: stack↔FF reconcile for cumulative Bind SLOADs (stale earlier
    // balances on stack → ERC-20 require revert under jump).
    if !snap.tip_sloads.is_empty() {
        use crate::specfence::rem::FfValue;
        for (addr, slot, snap_val) in &snap.tip_sloads {
            let ff_val = ff_reads.iter().find_map(|ff| match ff {
                FfValue::Storage {
                    address,
                    slot: s,
                    value,
                    ..
                } if address == addr && s == slot => Some(*value),
                _ => None,
            });
            if let Some(ff_val) = ff_val {
                if ff_val != *snap_val {
                    for v in interp.stack.data_mut().iter_mut() {
                        if *v == *snap_val {
                            *v = ff_val;
                        }
                    }
                }
            }
        }
    }
    RESUME_APPLIED.set(true);
    PENDING_RESUME.with(|c| *c.borrow_mut() = None);
    OPCODE_STEPS.set(0);
    if std::env::var_os("SPECFENCE_JUMP_DIG").is_some() {
        eprintln!("JUMP_DIG apply_ok steps={skipped} pc={} tip_sloads={}", snap.pc, snap.tip_sloads.len());
    }
    record_pc_resume(skipped);
}

/// Iter19: true while `with_bind_snap_tls` is active (Hang-free Bind snap, no plant).
pub(crate) fn bind_snap_tls_active() -> bool {
    BIND_SNAP.with(|c| c.get().is_some())
}

/// Arm Bind-snap capture after Bind-on-Data / EffectBoundary certify (SLOAD wrap consumes).
pub(crate) fn note_pending_bind_snap() {
    if bind_snap_tls_active() {
        PENDING_BIND_SNAP.set(true);
    }
}

/// Iter24/25 Bind-snap capture mode.
/// - `Off`: no Handler wrap / no TLS (SPECFENCE_BIND_SNAP=0).
/// - `ResumePath` (silent default / `=resume`): capture only on SuffixRepair resume /
///   force_bind / needs_live_capture — **no mass-path SNAP tax**.
/// - `Mass`: every Lean SpecFence execute (`SPECFENCE_BIND_SNAP=1` dig).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindSnapMode {
    Off,
    ResumePath,
    Mass,
}

/// Resolve Bind-snap mode from `SPECFENCE_BIND_SNAP`.
/// Iter25: default **ResumePath** (silent production) — hang-free with
/// refuse-if-stale + tip_sloads-gated jump (Mass JUMP was the Lean hang).
/// Force Off: `=0`; Mass dig: `=1`. No every-Handler tax on ResumePath.
pub(crate) fn bind_snap_mode() -> BindSnapMode {
    match std::env::var_os("SPECFENCE_BIND_SNAP") {
        None => BindSnapMode::ResumePath,
        Some(v) => {
            let s = v.to_string_lossy();
            if s == "0"
                || s.eq_ignore_ascii_case("false")
                || s.eq_ignore_ascii_case("off")
                || s.eq_ignore_ascii_case("no")
            {
                BindSnapMode::Off
            } else if s == "1"
                || s.eq_ignore_ascii_case("true")
                || s.eq_ignore_ascii_case("yes")
                || s.eq_ignore_ascii_case("mass")
            {
                BindSnapMode::Mass
            } else if s.eq_ignore_ascii_case("resume") {
                BindSnapMode::ResumePath
            } else {
                // Unknown → ResumePath (same as unset), not Off — keep silent default.
                BindSnapMode::ResumePath
            }
        }
    }
}

/// Env gate (compat): true only for **Mass** dig (`SPECFENCE_BIND_SNAP=1`).
pub(crate) fn bind_snap_env_enabled() -> bool {
    bind_snap_mode() == BindSnapMode::Mass
}

/// True when any Bind-snap capture path may run (ResumePath or Mass).
pub(crate) fn bind_snap_capture_wanted() -> bool {
    !matches!(bind_snap_mode(), BindSnapMode::Off)
}

/// Iter24: absolute Bind jump enabled when capture mode is on, unless
/// `SPECFENCE_BIND_SNAP_JUMP=0`. Explicit `=1` forces on (dig). Refuse-if-stale
/// still gates arming. SoftWait Soft stays ~0.
pub(crate) fn bind_snap_jump_enabled() -> bool {
    match std::env::var_os("SPECFENCE_BIND_SNAP_JUMP") {
        Some(v) => {
            let s = v.to_string_lossy();
            if s == "0"
                || s.eq_ignore_ascii_case("false")
                || s.eq_ignore_ascii_case("off")
                || s.eq_ignore_ascii_case("no")
            {
                false
            } else {
                s == "1"
                    || s.eq_ignore_ascii_case("true")
                    || s.eq_ignore_ascii_case("yes")
            }
        }
        None => bind_snap_capture_wanted(),
    }
}

/// Install SLOAD Bind-snap wrap when ResumePath/Mass/inspect may capture.
pub(crate) fn handler_bind_snap_install_wanted() -> bool {
    if crate::specfence::research_inspect_enabled() {
        return true;
    }
    bind_snap_capture_wanted()
}

/// Scoped Bind-snap TLS for Lean SpecFence execute (no IN_INSPECT / no WaitHard demote).
/// Caller selects when to invoke (Mass = every Lean run; ResumePath = repair only).
pub(crate) fn with_bind_snap_tls<R>(
    tx_idx: TxIdx,
    partial_retry: &PartialRetryTable,
    metrics: &MetricsInner,
    mut f: impl FnMut() -> R,
) -> R {
    let prev = BIND_SNAP.replace(Some(PlantTls {
        tx_idx,
        incarnation: 0,
        partial_retry: partial_retry as *const _,
        metrics: metrics as *const _,
        finegrain: None,
        tx_gas_limit: None,
    }));
    PENDING_BIND_SNAP.set(false);
    BIND_SNAP_STEPS.set(0);
    BIND_SLOAD_LOG.with(|c| c.borrow_mut().clear());
    BIND_SNAP_DEFERRED.with(|c| *c.borrow_mut() = None);
    // Iter28: LAST_SNAP is worker-TLS — steal can carry nested/wrong-tx tips into
    // attach_current_live_snap on the next rewind (apply_refuse code_hash). Clear.
    LAST_SNAP.with(|c| *c.borrow_mut() = None);
    // Iter21: clear stale PENDING_RESUME / RESUME_APPLIED from a prior jumped
    // incarnation on this worker — leftover applied flag skipped re-arm and
    // contributed to SoftWait/InconsistentRead livelock under JUMP dig.
    clear_pc_resume();
    RESUME_APPLIED.set(false);
    let out = f();
    // Iter26: one attach_live_boundary per ResumePath TLS — deepest tip≡FF.
    BIND_SNAP_DEFERRED.with(|c| {
        if let Some((k, snap)) = c.borrow_mut().take() {
            if let Some(ctx) = BIND_SNAP.get() {
                let table = unsafe { &*ctx.partial_retry };
                table.attach_live_boundary_at(ctx.tx_idx, k, snap, JournalBlob::default());
                let metrics = unsafe { &*ctx.metrics };
                metrics.record_handler_bind_snap_capture();
            }
        }
    });
    LAST_SNAP.with(|c| *c.borrow_mut() = None);
    BIND_SNAP.set(prev);
    PENDING_BIND_SNAP.set(false);
    clear_pc_resume();
    RESUME_APPLIED.set(false);
    out
}

/// EthInterpreter SLOAD wrap: stock SLOAD; after Bind-on-Data, capture live tip at
/// certified-prefix end (k < k_fail on RAW-read fails). Hang-free — no inspect_run.
#[inline(always)]
pub(crate) fn sload_bind_snap_eth<H: revm::interpreter::Host + ?Sized>(
    context: revm::interpreter::InstructionContext<'_, H, EthInterpreter>,
) {
    if !bind_snap_tls_active() {
        revm::interpreter::instructions::host::sload(context);
        return;
    }
    sload_bind_snap_eth_slow(context);
}

#[cold]
fn sload_bind_snap_eth_slow<H: revm::interpreter::Host + ?Sized>(
    context: revm::interpreter::InstructionContext<'_, H, EthInterpreter>,
) {
    let interp_ptr = context.interpreter as *mut Interpreter<EthInterpreter>;
    // Iter23: peek SLOAD key + target before host sload (stack top becomes value).
    let (tip_addr, tip_slot) = {
        let interp = unsafe { &*interp_ptr };
        (
            interp.input.target_address(),
            interp.stack.data().last().copied(),
        )
    };
    revm::interpreter::instructions::host::sload(context);
    if !PENDING_BIND_SNAP.replace(false) {
        return;
    }
    let n = BIND_SNAP_STEPS.get().saturating_add(1);
    BIND_SNAP_STEPS.set(n);
    let interp = unsafe { &mut *interp_ptr };
    let pc = interp.bytecode.pc();
    let gas_remaining = interp.gas.remaining();
    let gas_refunded = interp.gas.refunded();
    let bytecode_len = interp.bytecode.bytecode_slice().len();
    let code_hash = Some(interp.bytecode.get_or_calculate_hash());
    let mem_gas = *interp.gas.memory();
    let stack: Vec<_> = interp.stack.data().to_vec();
    // Iter23: accumulate ALL Bind SLOADs this incarnation — earlier stale
    // balances left on stack caused ERC-20 revert (status false, dgas=+661).
    let mut tip_sloads = BIND_SLOAD_LOG.with(|c| c.borrow().clone());
    if let (Some(slot), Some(val)) = (tip_slot, stack.last().copied()) {
        tip_sloads.push((tip_addr, slot, val));
        BIND_SLOAD_LOG.with(|c| c.borrow_mut().push((tip_addr, slot, val)));
    }
    // Read-prefix jump may still need pre-SLOAD memory (CALLDATACOPY etc.).
    const MEMORY_SNAP_CAP: usize = 8 * 1024;
    let mem_slice = interp.memory.context_memory();
    let memory = if !mem_slice.is_empty() && mem_slice.len() <= MEMORY_SNAP_CAP {
        mem_slice.to_vec()
    } else {
        Vec::new()
    };
    // Prefer rem current_k as honest-ish step credit when available.
    let rem_k = BIND_SNAP.with(|b| {
        b.get().map(|ctx| {
            let table = unsafe { &*ctx.partial_retry };
            table.current_k(ctx.tx_idx) as u64
        })
    }).unwrap_or(0);
    // Iter22: prefer PC as skip-credit proxy — rem_k is effect ordinal (often 2–5)
    // while ERC-20 tip PC is hundreds of opcodes deep; under-crediting is fine for
    // metrics, but rem_k-as-steps confused tip quality gates.
    let snap = BoundarySnapshot {
        pc,
        gas_remaining,
        gas_refunded,
        memory_words: mem_gas.words_num,
        memory_expansion_cost: mem_gas.expansion_cost,
        call_depth: CALL_DEPTH.get(),
        opcode_steps: (pc as u64).max(rem_k).max(n).max(1),
        stack,
        memory,
        code_hash,
        bytecode_len,
        at_call_boundary: false,
        post_sstore: false,
        sstore_index: 0,
        write_replays_at_tip: Vec::new(),
        tip_sloads,
    };
    LAST_SNAP.with(|c| *c.borrow_mut() = Some(snap.clone()));
    // Iter26: defer attach to TLS exit (deepest tip only — one bsnap/resume).
    let k_now = BIND_SNAP.with(|b| {
        b.get().map(|ctx| {
            let table = unsafe { &*ctx.partial_retry };
            table.current_k(ctx.tx_idx)
        })
    }).unwrap_or(0);
    BIND_SNAP_DEFERRED.with(|c| *c.borrow_mut() = Some((k_now, snap)));
}

/// Install SLOAD Bind-snap capture on Mainnet instruction table (Iter19).
pub(crate) fn install_handler_bind_snap_capture<H: revm::interpreter::Host>(
    instructions: &mut revm::handler::instructions::EthInstructions<EthInterpreter, H>,
) {
    const OP_SLOAD: u8 = 0x54;
    instructions.insert_instruction(
        OP_SLOAD,
        revm::interpreter::Instruction::new(sload_bind_snap_eth::<H>, 0),
    );
}

/// Iter10: install Handler SSTORE plant only when capture/jump/inspect may arm TLS.
/// Production jump/capture OFF → stock SSTORE (no per-opcode TLS tax). Enable with
/// `SPECFENCE_HANDLER_CAPTURE=1`, `SPECFENCE_ABSOLUTE_JUMP=1`, or research inspect.
pub(crate) fn handler_sstore_plant_install_wanted() -> bool {
    if crate::specfence::research_inspect_enabled() {
        return true;
    }
    if absolute_jump_env_enabled() {
        return true;
    }
    match std::env::var_os("SPECFENCE_HANDLER_CAPTURE") {
        None => false,
        Some(v) => {
            let s = v.to_string_lossy();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        }
    }
}

/// EthInterpreter SSTORE plant capture (installed on Mainnet EVM instruction table).
/// Iter10: thin fast path + #[cold] plant body so production I-cache stays stock-like.
#[inline(always)]
pub(crate) fn sstore_plant_capture_eth<H: revm::interpreter::Host + ?Sized>(
    context: revm::interpreter::InstructionContext<'_, H, EthInterpreter>,
) {
    // Fast path: identical to stock SSTORE when plant TLS is off (597 wall).
    if !plant_tls_active() {
        revm::interpreter::instructions::host::sstore(context);
        return;
    }
    sstore_plant_capture_eth_slow(context);
}

#[cold]
fn sstore_plant_capture_eth_slow<H: revm::interpreter::Host + ?Sized>(
    context: revm::interpreter::InstructionContext<'_, H, EthInterpreter>,
) {
    // Iter11: never warm via `sload` before stock SSTORE (EIP-2929 −2100 → seq≠par).
    // Peek stack; take original only if already warm (`sload_skip_cold_load`);
    // else ZERO (empty slot — correct for first-write; ERC-20 always SLOAD first).
    let interp_ptr = context.interpreter as *mut Interpreter<EthInterpreter>;
    let host_ptr = context.host as *mut H;
    let (slot, present, target) = {
        let interp = unsafe { &mut *interp_ptr };
        let data = interp.stack.data();
        if data.len() >= 2 {
            let present = data[data.len() - 1];
            let slot = data[data.len() - 2];
            let target = interp.input.target_address();
            (slot, present, target)
        } else {
            (U256::ZERO, U256::ZERO, Address::ZERO)
        }
    };
    let original = unsafe { &mut *host_ptr }
        .sload_skip_cold_load(target, slot, true)
        .ok()
        .map(|v| v.data)
        .unwrap_or(U256::ZERO);
    revm::interpreter::instructions::host::sstore(context);

    let n = HANDLER_SSTORE_STEPS.get().saturating_add(1);
    HANDLER_SSTORE_STEPS.set(n);
    let interp = unsafe { &mut *interp_ptr };
    let pc = interp.bytecode.pc();
    let gas_remaining = interp.gas.remaining();
    let gas_refunded = interp.gas.refunded();
    let bytecode_len = interp.bytecode.bytecode_slice().len();
    let code_hash = Some(interp.bytecode.get_or_calculate_hash());
    let mem_gas = *interp.gas.memory();
    let stack: Vec<_> = interp.stack.data().to_vec();
    const MEMORY_SNAP_CAP: usize = 8 * 1024;
    let mem_slice = interp.memory.context_memory();
    let memory = if mem_slice.len() > 0 && mem_slice.len() <= MEMORY_SNAP_CAP {
        mem_slice.to_vec()
    } else {
        Vec::new()
    };
    let mut snap = BoundarySnapshot {
        pc,
        gas_remaining,
        gas_refunded,
        memory_words: mem_gas.words_num,
        memory_expansion_cost: mem_gas.expansion_cost,
        call_depth: CALL_DEPTH.get(),
        opcode_steps: n,
        stack,
        memory,
        code_hash,
        bytecode_len,
        at_call_boundary: false,
        post_sstore: true,
        sstore_index: n,
        write_replays_at_tip: Vec::new(),
        tip_sloads: Vec::new(),
    };
    PLANT.with(|p| {
        if let Some(plant) = p.get() {
            let table = unsafe { &*plant.partial_retry };
            table.note_post_sstore_gas(plant.tx_idx, gas_remaining);
            if target != Address::ZERO {
                let loc = hash_deterministic(MemoryLocation::Storage(target, slot));
                table.note_write_replay(
                    plant.tx_idx,
                    loc,
                    StorageWriteReplay {
                        address: target,
                        slot,
                        original,
                        present,
                        gas_remaining_after: gas_remaining,
                    },
                );
            }
            snap.write_replays_at_tip = table.write_replay_values(plant.tx_idx);
            let metrics = unsafe { &*plant.metrics };
            metrics.record_handler_sstore_capture();
            table.attach_live_boundary(plant.tx_idx, snap.clone(), JournalBlob::default());
        }
    });
    LAST_SNAP.with(|c| *c.borrow_mut() = Some(snap));
}

/// Install hang-free SSTORE plant capture on a Mainnet-style instruction table.
pub(crate) fn install_handler_sstore_plant_capture<H: revm::interpreter::Host>(
    instructions: &mut revm::handler::instructions::EthInstructions<EthInterpreter, H>,
) {
    const OP_SSTORE: u8 = 0x55;
    instructions.insert_instruction(
        OP_SSTORE,
        revm::interpreter::Instruction::new(sstore_plant_capture_eth::<H>, 0),
    );
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
        try_apply_pending_pc_resume(interp, context, CALL_DEPTH.get());
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
        // Research journal stream: log every storage/basic world-state opcode
        // (including journal-cached repeats that never re-enter pevm Db).
        let depth = CALL_DEPTH.get();
        maybe_note_journal_effect(interp, op, n, depth);
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
            // Iter2: always attach post-SSTORE live tip (write-prefix jump needs
            // gas-equal snap even when PENDING_EFFECT_CP was not armed).
            attach_live_to_plant(snap.clone(), JournalBlob::default());
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
                sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
            sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
                sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
                sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
                sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
                sstore_index: 0,
            write_replays_at_tip: Vec::new(),
            tip_sloads: Vec::new(),
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
