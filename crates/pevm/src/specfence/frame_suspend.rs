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
//! 1. `Vm` arms the DB cell only for a known protected toucher while no
//!    frame is already parked. The spare `Evm` is built on the first hold.
//!    Execution stays on `run_plain`.
//! 2. `VmDb::basic` requests a suspend when that read returns `Blocking`.
//! 3. After `run_plain` halts, the handler rewinds a safe opcode
//!    (`BALANCE` / `SELFBALANCE` / `EXTCODESIZE` / `EXTCODEHASH`): PC −1 and
//!    static gas. `CALL` is put back and dropped. `catch_error` is skipped
//!    only when the frame is held.
//! 4. `Vm` swaps the live `Evm` into the spare slot. The next transaction's
//!    `set_tx` / `journal.clear()` hits the other `Evm`.
//! 5. When the predecessor is `is_validated`, `resume_pevm_tx` continues the
//!    same frame loop. It does not call `Handler::run`.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use revm::interpreter::InitialAndFloorGas;

static SUSPENDS: AtomicU64 = AtomicU64::new(0);
static RESUMES: AtomicU64 = AtomicU64::new(0);
static REQUESTS: AtomicU64 = AtomicU64::new(0);
static UNSAFE_OP: AtomicU64 = AtomicU64::new(0);
static PRE_FRAME: AtomicU64 = AtomicU64::new(0);
static OP_HITS: [AtomicU64; 256] = [const { AtomicU64::new(0) }; 256];

thread_local! {
    static REQUESTED: Cell<Option<usize>> = const { Cell::new(None) };
    static HELD_PRED: Cell<Option<usize>> = const { Cell::new(None) };
    static GAS: Cell<Option<(InitialAndFloorGas, i64)>> = const { Cell::new(None) };
}

/// `(suspends, resumes, basic requests, unsafe-opcode rejects)` since process start.
#[must_use]
pub fn frame_suspend_counts() -> (u64, u64, u64, u64, u64) {
    (
        SUSPENDS.load(Ordering::Relaxed),
        RESUMES.load(Ordering::Relaxed),
        REQUESTS.load(Ordering::Relaxed),
        UNSAFE_OP.load(Ordering::Relaxed),
        PRE_FRAME.load(Ordering::Relaxed),
    )
}

/// Non-zero opcode hits among rewind rejects. `(opcode, count)`.
#[must_use]
pub fn frame_suspend_unsafe_ops() -> Vec<(u8, u64)> {
    OP_HITS
        .iter()
        .enumerate()
        .filter_map(|(op, n)| {
            let n = n.load(Ordering::Relaxed);
            (n > 0).then_some((op as u8, n))
        })
        .collect()
}

/// Drop a request the handler did not consume. Returns whether one was pending.
pub(crate) fn clear_request() -> bool {
    let pending = REQUESTED.take().is_some();
    if pending {
        PRE_FRAME.fetch_add(1, Ordering::Relaxed);
    }
    pending
}

/// `basic` wants this read to suspend. The handler confirms the opcode.
pub(crate) fn request(pred: usize) {
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    REQUESTED.set(Some(pred));
}

pub(crate) fn note_unsafe_opcode(op: u8) {
    UNSAFE_OP.fetch_add(1, Ordering::Relaxed);
    OP_HITS[op as usize].fetch_add(1, Ordering::Relaxed);
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
