//! Plant v2 M1c: CALL / effect-boundary PC resume.
//!
//! Uses a stock revm Inspector (not a custom `run_exec_loop`) to:
//! 1. Count opcodes and snapshot interpreter PC/stack/memory/gas at boundaries
//! 2. On RewindTo resume, jump the matching call-depth frame to the certified
//!    prefix boundary PC so prefix opcodes are not re-interpreted
//!
//! Nested mid-CALL arbitrary-PC beyond a captured frame is TODO; invalid
//! control-flow → caller FullRestarts (no resume arm).

#![allow(dead_code)]

use std::cell::{Cell, RefCell};

use alloy_primitives::U256;
use revm::interpreter::{
    interpreter::EthInterpreter,
    interpreter_types::{Jumps, StackTr},
    CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter,
};
use revm::Inspector;

use crate::TxIdx;

use super::metrics::MetricsInner;
use super::rem::{CheckpointKind, PartialRetryTable};

/// Interpreter boundary snapshot for M1c PC resume.
#[derive(Debug, Clone)]
pub(crate) struct BoundarySnapshot {
    pub pc: usize,
    pub gas_remaining: u64,
    pub call_depth: u16,
    /// Cumulative interpreter steps at capture (honest skip credit on resume).
    pub opcode_steps: u64,
    pub stack: Vec<U256>,
    pub memory: Vec<u8>,
}

impl BoundarySnapshot {
    pub(crate) fn capture_from_interp(
        interp: &Interpreter<EthInterpreter>,
        call_depth: u16,
        opcode_steps: u64,
    ) -> Self {
        Self {
            pc: interp.bytecode.pc(),
            gas_remaining: interp.gas.remaining(),
            call_depth,
            opcode_steps,
            stack: interp.stack.data().to_vec(),
            memory: interp.memory.context_memory().to_vec(),
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
    PLANT.set(prev);
    out
}

/// Arm PC resume for the next matching-depth interpreter init (RewindTo path).
pub(crate) fn arm_pc_resume(snap: BoundarySnapshot) {
    PENDING_RESUME.with(|c| *c.borrow_mut() = Some(snap));
    RESUME_APPLIED.set(false);
}

pub(crate) fn clear_pc_resume() {
    PENDING_RESUME.with(|c| *c.borrow_mut() = None);
    RESUME_APPLIED.set(false);
}

/// Record an EffectBoundary checkpoint.
///
/// When SpecFenceInspector is driving the loop, defer to `step_end` for a full
/// PC/stack snap. Otherwise (Handler::run production path) emit a lite snap
/// whose `opcode_steps` equals the current SpecFence effect ordinal — an honest
/// lower-bound skip credit for certified-prefix resume accounting.
pub(crate) fn note_pending_effect_boundary() {
    // Prefer Inspector step_end fill when plant TLS + inspector are active.
    if PLANT.with(|p| p.get().is_some()) && STEPS_THIS_RUN.get() > 0 {
        PENDING_EFFECT_CP.set(true);
        return;
    }
    PLANT.with(|p| {
        if let Some(plant) = p.get() {
            let table = unsafe { &*plant.partial_retry };
            let k = table.current_k(plant.tx_idx);
            let snap = BoundarySnapshot {
                pc: 0,
                gas_remaining: 0,
                call_depth: 0,
                opcode_steps: k as u64,
                stack: Vec::new(),
                memory: Vec::new(),
            };
            let _ = table.push_checkpoint_with_boundary(
                plant.tx_idx,
                CheckpointKind::EffectBoundary,
                Some(snap),
            );
        }
    });
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
        }
    });
}

/// SpecFence boundary Inspector — observational except on armed PC resume.
#[derive(Debug, Default, Clone)]
pub(crate) struct SpecFenceInspector;

impl SpecFenceInspector {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl<CTX> Inspector<CTX, EthInterpreter> for SpecFenceInspector {
    fn initialize_interp(
        &mut self,
        interp: &mut Interpreter<EthInterpreter>,
        _context: &mut CTX,
    ) {
        if RESUME_APPLIED.get() {
            return;
        }
        let snap = PENDING_RESUME.with(|c| c.borrow().clone());
        let Some(snap) = snap else {
            return;
        };
        let depth = CALL_DEPTH.get();
        // Top-level call is depth 1 after `call()`; allow 0/1 soft match for
        // frames where call hooks did not fire.
        if snap.call_depth != depth && !(snap.call_depth <= 1 && depth <= 1) {
            return;
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

    fn step_end(&mut self, interp: &mut Interpreter<EthInterpreter>, _context: &mut CTX) {
        let depth = CALL_DEPTH.get();
        let n = OPCODE_STEPS.get();
        let snap = BoundarySnapshot::capture_from_interp(interp, depth, n);
        LAST_SNAP.with(|c| *c.borrow_mut() = Some(snap.clone()));
        if PENDING_EFFECT_CP.replace(false) {
            push_cp(CheckpointKind::EffectBoundary, Some(snap));
        }
    }

    fn call(
        &mut self,
        _context: &mut CTX,
        _inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        let d = CALL_DEPTH.get().saturating_add(1);
        CALL_DEPTH.set(d);
        let snap = LAST_SNAP.with(|c| c.borrow().clone());
        push_cp(CheckpointKind::CallEntry, snap);
        None
    }

    fn call_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CallInputs,
        _outcome: &mut CallOutcome,
    ) {
        let snap = LAST_SNAP.with(|c| c.borrow().clone());
        push_cp(CheckpointKind::CallExit, snap);
        CALL_DEPTH.set(CALL_DEPTH.get().saturating_sub(1));
    }

    fn create(
        &mut self,
        _context: &mut CTX,
        _inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        let d = CALL_DEPTH.get().saturating_add(1);
        CALL_DEPTH.set(d);
        let snap = LAST_SNAP.with(|c| c.borrow().clone());
        push_cp(CheckpointKind::CallEntry, snap);
        None
    }

    fn create_end(
        &mut self,
        _context: &mut CTX,
        _inputs: &CreateInputs,
        _outcome: &mut CreateOutcome,
    ) {
        let snap = LAST_SNAP.with(|c| c.borrow().clone());
        push_cp(CheckpointKind::CallExit, snap);
        CALL_DEPTH.set(CALL_DEPTH.get().saturating_sub(1));
    }
}

#[cfg(test)]
mod m1c_tests {
    use super::*;
    use revm::interpreter::interpreter::{EthInterpreter, ExtBytecode};
    use revm::interpreter::{Gas, Interpreter};
    use revm::state::Bytecode;

    #[test]
    fn boundary_snapshot_roundtrip_fields() {
        let snap = BoundarySnapshot {
            pc: 42,
            gas_remaining: 99_000,
            call_depth: 1,
            opcode_steps: 17,
            stack: vec![U256::from(1), U256::from(2)],
            memory: vec![0xab, 0xcd],
        };
        assert_eq!(snap.pc, 42);
        assert_eq!(snap.opcode_steps, 17);
        assert_eq!(snap.stack.len(), 2);
    }

    #[test]
    fn apply_to_interp_jumps_pc_and_restores_stack() {
        // Minimal bytecode: STOP at 0, then some padding so pc=2 is in range.
        let code = Bytecode::new_raw(vec![0x00, 0x00, 0x00, 0x00, 0x00].into());
        let mut interp = Interpreter::<EthInterpreter>::default();
        // Rebuild with our bytecode if default is empty — use set via absolute_jump after init.
        // Create via Interpreter::new is complex; test apply fields on a snap roundtrip of stack/gas/pc
        // through capture_from_interp after manual setup.
        let snap = BoundarySnapshot {
            pc: 2,
            gas_remaining: 50_000,
            call_depth: 1,
            opcode_steps: 9,
            stack: vec![U256::from(7), U256::from(8)],
            memory: vec![1, 2, 3, 4],
        };
        // Use ExtBytecode from code
        interp.bytecode = ExtBytecode::new(code);
        interp.gas = Gas::new(100_000);
        snap.apply_to_interp(&mut interp);
        assert_eq!(interp.bytecode.pc(), 2, "M1c must jump PC to boundary");
        assert_eq!(interp.gas.remaining(), 50_000);
        assert_eq!(interp.stack.data(), &[U256::from(7), U256::from(8)]);
        assert!(interp.memory.len() >= 4);
    }

    #[test]
    fn arm_pc_resume_records_skip_via_initialize_interp() {
        clear_pc_resume();
        let snap = BoundarySnapshot {
            pc: 0,
            gas_remaining: 10,
            call_depth: 0,
            opcode_steps: 42,
            stack: vec![],
            memory: vec![],
        };
        arm_pc_resume(snap);
        // Without plant TLS metrics, initialize_interp still applies.
        let code = Bytecode::new_raw(vec![0x00].into());
        let mut interp = Interpreter::<EthInterpreter>::default();
        interp.bytecode = ExtBytecode::new(code);
        interp.gas = Gas::new(100);
        let mut insp = SpecFenceInspector::new();
        let mut ctx = ();
        insp.initialize_interp(&mut interp, &mut ctx);
        assert!(resume_was_applied());
        assert_eq!(last_prefix_opcodes_skipped(), 42);
        clear_pc_resume();
    }
}
