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
//! M1e: production `arm_pc_resume` gated by [`jump_is_safe`] + journal-blob restore.
//! Nested mid-CALL (call_depth > 1) still falls back — CallOutcome cache TODO.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};

use alloy_primitives::{B256, U256};
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
    AccessMode, CheckpointKind, PartialRetryTable, ResumeContinuation,
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

/// Interpreter boundary snapshot for M1c/M1e PC resume.
#[derive(Debug, Clone)]
pub(crate) struct BoundarySnapshot {
    pub pc: usize,
    pub gas_remaining: u64,
    pub call_depth: u16,
    /// Cumulative interpreter steps at capture (honest skip credit on resume).
    pub opcode_steps: u64,
    pub stack: Vec<U256>,
    pub memory: Vec<u8>,
    /// Code hash at capture (control-flow / same-contract gate).
    pub code_hash: Option<B256>,
    /// Bytecode length at capture (PC range check).
    pub bytecode_len: usize,
}

impl BoundarySnapshot {
    pub(crate) fn capture_from_interp(
        interp: &mut Interpreter<EthInterpreter>,
        call_depth: u16,
        opcode_steps: u64,
    ) -> Self {
        let bytecode_len = interp.bytecode.bytecode_slice().len();
        let code_hash = Some(interp.bytecode.get_or_calculate_hash());
        Self {
            pc: interp.bytecode.pc(),
            gas_remaining: interp.gas.remaining(),
            call_depth,
            opcode_steps,
            stack: interp.stack.data().to_vec(),
            memory: interp.memory.context_memory().to_vec(),
            code_hash,
            bytecode_len,
        }
    }

    pub(crate) fn apply_to_interp(&self, interp: &mut Interpreter<EthInterpreter>) {
        interp.bytecode.absolute_jump(self.pc);
        interp.gas.set_remaining(self.gas_remaining);
        interp.stack.clear();
        for v in &self.stack {
            let _ = interp.stack.push(*v);
        }
        let need = self.memory.len();
        if interp.memory.len() < need {
            interp.memory.resize(need);
        }
        if need > 0 {
            let mut mem = interp.memory.context_memory_mut();
            let n = need.min(mem.len());
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

/// M1e safety gate: when true, production may `arm_pc_resume` + restore journal blob.
///
/// Requires live capture, top-level frame (call_depth ≤ 1), in-range PC, certified
/// prefix, and a journal blob whenever the prefix contains writes (so post-jump
/// world-state ≡ sequential prefix without re-running SSTORE).
pub(crate) fn jump_is_safe(cont: &ResumeContinuation) -> bool {
    let Some(snap) = cont.jump_snap.as_ref() else {
        return false;
    };
    if !snap.is_live_capture() {
        return false;
    }
    // Nested CALL frames: fall back (CallOutcome cache TODO).
    if snap.call_depth > 1 {
        return false;
    }
    if snap.bytecode_len > 0 && snap.pc >= snap.bytecode_len {
        return false;
    }
    // Empty / synthetic-only prefix — nothing to jump over safely.
    if cont.cp.k == 0 && cont.effects.is_empty() && snap.opcode_steps == 0 {
        return false;
    }
    // Always require a journal blob (state + logs). Jumping without it drops
    // prefix SSTORE/LOG effects and breaks sequential ≡ parallel.
    let Some(blob) = cont.journal_blob.as_ref() else {
        return false;
    };
    if blob.is_empty() {
        return false;
    }
    let _ = cont.effects.iter().any(|e| e.mode == AccessMode::Write);
    true
}

/// Scoped plant pointers for Inspector → PartialRetry / metrics (execute duration only).
#[derive(Clone, Copy)]
struct PlantTls {
    tx_idx: TxIdx,
    partial_retry: *const PartialRetryTable,
    metrics: *const MetricsInner,
}

thread_local! {
    static PLANT: Cell<Option<PlantTls>> = const { Cell::new(None) };
    static OPCODE_STEPS: Cell<u64> = const { Cell::new(0) };
    static CALL_DEPTH: Cell<u16> = const { Cell::new(0) };
    static LAST_SNAP: RefCell<Option<BoundarySnapshot>> = const { RefCell::new(None) };
    static PENDING_EFFECT_CP: Cell<bool> = const { Cell::new(false) };
    static PENDING_RESUME: RefCell<Option<BoundarySnapshot>> = const { RefCell::new(None) };
    static PENDING_JOURNAL_BLOB: RefCell<Option<JournalBlob>> = const { RefCell::new(None) };
    static RESUME_APPLIED: Cell<bool> = const { Cell::new(false) };
    static STEPS_THIS_RUN: Cell<u64> = const { Cell::new(0) };
    static LAST_SKIPPED: Cell<u64> = const { Cell::new(0) };
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
    LAST_SNAP.with(|c| *c.borrow_mut() = None);
    PENDING_EFFECT_CP.set(false);
    RESUME_APPLIED.set(false);
    STEPS_THIS_RUN.set(0);
    LAST_SKIPPED.set(0);
    let out = f();
    PENDING_RESUME.with(|c| *c.borrow_mut() = None);
    PENDING_JOURNAL_BLOB.with(|c| *c.borrow_mut() = None);
    PLANT.set(prev);
    out
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
    RESUME_APPLIED.set(false);
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
    // Side-channel jump_snap + journal_blob + safety gate are implemented, but
    // arming absolute_jump on conflict schedules still breaks seq≡par (and can
    // livelock) even with log/state restore + circuit breaker. Default: always
    // fall back to M1d credit-only resume. Research opt-in: SPECFENCE_ABSOLUTE_JUMP=1.
    let enable = std::env::var_os("SPECFENCE_ABSOLUTE_JUMP").is_some_and(|v| v != "0");
    if !enable || partial_retry.is_jump_disabled(tx_idx) || !jump_is_safe(cont) {
        metrics.record_absolute_jump_fallback();
        let _ = (tx_idx, cont); // keep params used for future enable
        return false;
    }
    let snap = cont.jump_snap.clone().expect("jump_is_safe implies jump_snap");
    arm_pc_resume_with_blob(snap, cont.journal_blob.clone());
    true
}

/// Record an EffectBoundary checkpoint.
///
/// Always emit a lite snap immediately (M1c-compatible k-tracking for repair).
/// When SpecFenceInspector is driving `inspect_run`, set `PENDING_EFFECT_CP` so
/// `step_end` attaches a live PC/stack snap + journal blob to this k (M1e).
pub(crate) fn note_pending_effect_boundary(
    tx_idx: TxIdx,
    partial_retry: &PartialRetryTable,
) {
    let k = partial_retry.current_k(tx_idx);
    let live_steps = LAST_SNAP.with(|c| c.borrow().as_ref().map(|s| s.opcode_steps));
    let snap = BoundarySnapshot {
        pc: 0,
        gas_remaining: 0,
        call_depth: 0,
        opcode_steps: live_steps.filter(|n| *n > 0).unwrap_or(k as u64),
        stack: Vec::new(),
        memory: Vec::new(),
        code_hash: None,
        bytecode_len: 0,
    };
    let _ = partial_retry.push_checkpoint_with_boundary(
        tx_idx,
        CheckpointKind::EffectBoundary,
        Some(snap),
    );
    // Capture live snap+blob only when absolute jump research flag is on.
    if std::env::var_os("SPECFENCE_ABSOLUTE_JUMP").is_some_and(|v| v != "0") {
        PENDING_EFFECT_CP.set(true);
    }
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
        // M1e: restore revm journal blob so prefix SSTORE world-state is present
        // without re-executing those opcodes after the PC jump.
        let blob = PENDING_JOURNAL_BLOB.with(|c| c.borrow_mut().take());
        if let Some(blob) = blob {
            let n = blob.account_count();
            let state = context.journal_mut().evm_state_mut();
            for (addr, acc) in blob.state {
                state.insert(addr, acc);
            }
            for log in blob.logs {
                context.journal_mut().log(log);
            }
            if n > 0 {
                record_journal_blob_ff(n);
            }
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
        let depth = CALL_DEPTH.get();
        let snap = BoundarySnapshot::capture_from_interp(interp, depth, n);
        LAST_SNAP.with(|c| *c.borrow_mut() = Some(snap));
    }

    fn step_end(&mut self, interp: &mut Interpreter<EthInterpreter>, context: &mut CTX) {
        let depth = CALL_DEPTH.get();
        let n = OPCODE_STEPS.get();
        let snap = BoundarySnapshot::capture_from_interp(interp, depth, n);
        LAST_SNAP.with(|c| *c.borrow_mut() = Some(snap.clone()));
        if PENDING_EFFECT_CP.replace(false) {
            let full = context.journal().evm_state();
            let mut state = EvmState::default();
            for (addr, acc) in full.iter() {
                if acc.is_touched() {
                    state.insert(*addr, acc.clone());
                }
            }
            let blob = JournalBlob {
                state,
                logs: context.journal().logs().to_vec(),
            };
            attach_live_to_plant(snap, blob);
        }
    }

    fn call(
        &mut self,
        _context: &mut CTX,
        _inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        CALL_DEPTH.set(CALL_DEPTH.get().saturating_add(1));
        None
    }

    fn call_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CallInputs,
        _outcome: &mut CallOutcome,
    ) {
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

    use crate::specfence::rem::{CheckpointId, ResumeContinuation};
    use hashbrown::HashMap;
    use crate::BuildIdentityHasher;

    fn lite_snap(pc: usize, steps: u64) -> BoundarySnapshot {
        BoundarySnapshot {
            pc,
            gas_remaining: 0,
            call_depth: 0,
            opcode_steps: steps,
            stack: Vec::new(),
            memory: Vec::new(),
            code_hash: None,
            bytecode_len: 0,
        }
    }

    #[test]
    fn boundary_snapshot_roundtrip_fields() {
        let snap = BoundarySnapshot {
            pc: 42,
            gas_remaining: 99_000,
            call_depth: 1,
            opcode_steps: 17,
            stack: vec![U256::from(1), U256::from(2)],
            memory: vec![0xab, 0xcd],
            code_hash: None,
            bytecode_len: 64,
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
            call_depth: 1,
            opcode_steps: 9,
            stack: vec![U256::from(7), U256::from(8)],
            memory: vec![1, 2, 3, 4],
            code_hash: None,
            bytecode_len: 5,
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
            call_depth: 0,
            opcode_steps: 42,
            stack: vec![],
            memory: vec![],
            code_hash: None,
            bytecode_len: 1,
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
        };
        assert!(!jump_is_safe(&lite), "lite snap must not jump");

        let nested = ResumeContinuation {
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 1_000,
                call_depth: 2,
                opcode_steps: 10,
                stack: vec![U256::from(1)],
                memory: vec![],
                code_hash: Some(B256::ZERO),
                bytecode_len: 32,
            }),
            journal_blob: None,
            ..lite.clone()
        };
        assert!(!jump_is_safe(&nested), "nested CALL must fall back");
    }

    #[test]
    fn jump_is_safe_accepts_live_with_journal_blob() {
        let mut state = EvmState::default();
        state.insert(alloy_primitives::Address::ZERO, Default::default());
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1, 2],
            suffix_writes: vec![],
            effects: vec![],
            checkpoints: vec![],
            values: HashMap::with_hasher(BuildIdentityHasher::default()),
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 50_000,
                call_depth: 1,
                opcode_steps: 12,
                stack: vec![U256::from(9)],
                memory: vec![0],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
            }),
            journal_blob: Some(JournalBlob {
                state,
                logs: vec![],
            }),
        };
        assert!(
            jump_is_safe(&cont),
            "live top-level snap + journal blob may jump"
        );
    }

    #[test]
    fn jump_is_safe_rejects_live_without_blob() {
        let cont = ResumeContinuation {
            cp: CheckpointId {
                tx_idx: 0,
                incarnation: 1,
                k: 2,
            },
            k_fail: 4,
            certified: vec![1],
            suffix_writes: vec![],
            effects: vec![],
            checkpoints: vec![],
            values: HashMap::with_hasher(BuildIdentityHasher::default()),
            boundary: Some(lite_snap(0, 2)),
            jump_snap: Some(BoundarySnapshot {
                pc: 4,
                gas_remaining: 50_000,
                call_depth: 1,
                opcode_steps: 12,
                stack: vec![U256::from(9)],
                memory: vec![0],
                code_hash: Some(B256::ZERO),
                bytecode_len: 64,
            }),
            journal_blob: None,
        };
        assert!(!jump_is_safe(&cont), "live snap without blob must fall back");
    }
}
