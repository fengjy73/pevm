//! Same-frame suspend across `Database::basic` → `Blocking`.
//!
//! Stock revm halts the interpreter, reverts the call checkpoint, and
//! `Handler::catch_error` drops the frame stack and journal. The worker then
//! `set_tx` + `journal.clear()` for the next transaction. A later `Vm::execute`
//! is a new `Handler::run` from k = 0.
//!
//! This module is the thread-local handshake for the forked handler in
//! `tx_runner`:
//!
//! 1. `Vm` arms `allow` only for a known protected toucher while the worker's
//!    spare `Evm` is free.
//! 2. The handler single-steps that transaction. Before a rewind-safe opcode
//!    (`BALANCE` / `SELFBALANCE` / `EXTCODESIZE` / `EXTCODEHASH`) it marks the
//!    opcode safe and enters the interpreter.
//! 3. `VmDb::basic` requests a suspend instead of only returning `Blocking`.
//! 4. The handler rewinds that opcode (PC and static gas), leaves the frame
//!    stack and journal in place, and skips `catch_error`.
//! 5. `Vm` swaps the live `Evm` into the spare slot. The next transaction's
//!    `set_tx` / `journal.clear()` hits the other `Evm`.
//! 6. When the predecessor is `is_validated`, `resume_pevm_tx` continues the
//!    same frame loop. It does not call `Handler::run`.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use revm::interpreter::InitialAndFloorGas;

static SUSPENDS: AtomicU64 = AtomicU64::new(0);
static RESUMES: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static ALLOW: Cell<bool> = const { Cell::new(false) };
    static IN_INTERP: Cell<bool> = const { Cell::new(false) };
    static OP_SAFE: Cell<bool> = const { Cell::new(false) };
    static REQUESTED: Cell<Option<usize>> = const { Cell::new(None) };
    static HELD_PRED: Cell<Option<usize>> = const { Cell::new(None) };
    static GAS: Cell<Option<(InitialAndFloorGas, i64)>> = const { Cell::new(None) };
}

/// `(suspends, resumes)` since process start. Soft=0 compare prints these.
#[must_use]
pub fn frame_suspend_counts() -> (u64, u64) {
    (
        SUSPENDS.load(Ordering::Relaxed),
        RESUMES.load(Ordering::Relaxed),
    )
}

/// Arm single-step + suspend for the handler invocation on this thread.
pub(crate) fn set_allow(allow: bool) {
    ALLOW.set(allow);
    if !allow {
        // A request is only meaningful for the handler invocation that armed it.
        REQUESTED.set(None);
    }
}

#[inline]
pub(crate) fn allow() -> bool {
    ALLOW.with(Cell::get)
}

/// True only while a rewind-safe opcode is inside `Interpreter::step`.
#[inline]
pub(crate) fn op_safe_and_in_interp() -> bool {
    IN_INTERP.with(Cell::get) && OP_SAFE.with(Cell::get)
}

pub(crate) fn set_op_safe(safe: bool) {
    OP_SAFE.set(safe);
}

/// Enter / leave the interpreter step. A guard clears the flag on panic.
pub(crate) struct InterpGuard;

impl InterpGuard {
    pub(crate) fn enter() -> Self {
        IN_INTERP.set(true);
        Self
    }
}

impl Drop for InterpGuard {
    fn drop(&mut self) {
        IN_INTERP.set(false);
        OP_SAFE.set(false);
    }
}

/// `basic` wants this read to suspend. The handler confirms it after rewind.
pub(crate) fn request(pred: usize) {
    REQUESTED.set(Some(pred));
}

/// Predecessor captured by [`request`], if `basic` asked to suspend.
pub(crate) fn take_request() -> Option<usize> {
    REQUESTED.take()
}

/// Remember the predecessor after the handler has rewound the opcode.
pub(crate) fn mark_held(pred: usize) {
    HELD_PRED.set(Some(pred));
    SUSPENDS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub(crate) fn is_held() -> bool {
    HELD_PRED.with(|c| c.get().is_some())
}

/// Gas inputs `post_execution` needs when the frame continues without
/// `Handler::validate` / `pre_execution`.
pub(crate) fn stash_gas(init: InitialAndFloorGas, eip7702_refund: i64) {
    GAS.set(Some((init, eip7702_refund)));
}

/// Outcome of a handler return that left the frame intact.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SuspendOutcome {
    /// Predecessor the protected read is waiting on (`is_validated`).
    pub pred: usize,
    /// Copied from `Handler::validate` before the frame was built.
    pub init: InitialAndFloorGas,
    /// EIP-7702 refund from `pre_execution`, passed back into `post_execution`.
    pub eip7702_refund: i64,
}

/// Take the suspend outcome, if the handler skipped `catch_error`.
pub(crate) fn take_outcome() -> Option<SuspendOutcome> {
    let pred = HELD_PRED.take()?;
    let (init, eip7702_refund) = GAS.take().unwrap_or_default();
    Some(SuspendOutcome {
        pred,
        init,
        eip7702_refund,
    })
}

pub(crate) fn note_resume() {
    RESUMES.fetch_add(1, Ordering::Relaxed);
}

/// `BALANCE`, `EXTCODESIZE`, `EXTCODEHASH`, `SELFBALANCE`.
///
/// These opcodes do not pop the stack or resize memory before `basic`.
/// Rewinding PC by one and refunding static gas re-executes the same read.
/// `CALL` / `EXTCODECOPY` pop first; those stay on the ordinary `Blocking` path.
#[inline]
pub(crate) const fn opcode_rewind_safe(op: u8) -> bool {
    matches!(op, 0x31 | 0x3b | 0x3f | 0x47)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_opcodes_are_basic_reads_without_pops() {
        assert!(opcode_rewind_safe(0x31));
        assert!(opcode_rewind_safe(0x47));
        assert!(!opcode_rewind_safe(0xf1));
        assert!(!opcode_rewind_safe(0x3c));
    }
}
