//! In-block learning consult trace. Off unless `SPECFENCE_INBLOCK_TRACE=1`.
//!
//! Flag-off call sites return before any counter update. No scheduler,
//! Avoid, or Admit decision reads these counters.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        matches!(
            std::env::var("SPECFENCE_INBLOCK_TRACE").ok().as_deref(),
            Some("1") | Some("true") | Some("TRUE")
        )
    })
}

static ARM_N: AtomicUsize = AtomicUsize::new(0);
static FIRST_ARM_TX: AtomicUsize = AtomicUsize::new(usize::MAX);
static FIRST_ARM_NS: AtomicU64 = AtomicU64::new(0);
static CONSULT_NO_PRED: AtomicUsize = AtomicUsize::new(0);
static CONSULT_OPT: AtomicUsize = AtomicUsize::new(0);

/// Drop counters from the previous block. No-op when the flag is off.
pub fn reset() {
    if !enabled() {
        return;
    }
    ARM_N.store(0, Ordering::Relaxed);
    FIRST_ARM_TX.store(usize::MAX, Ordering::Relaxed);
    FIRST_ARM_NS.store(0, Ordering::Relaxed);
    CONSULT_NO_PRED.store(0, Ordering::Relaxed);
    CONSULT_OPT.store(0, Ordering::Relaxed);
}

/// `protect_hot` installed WaitOnce for a location this block.
#[inline]
pub fn note_arm(tx: usize, origin: Instant) {
    if !enabled() {
        return;
    }
    let n = ARM_N.fetch_add(1, Ordering::Relaxed);
    if n == 0 {
        FIRST_ARM_TX.store(tx, Ordering::Relaxed);
        FIRST_ARM_NS.store(origin.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

/// Protected location was consulted and no predecessor writer was visible.
#[inline]
pub fn note_consult_no_pred() {
    if enabled() {
        CONSULT_NO_PRED.fetch_add(1, Ordering::Relaxed);
    }
}

/// Protected location was consulted and the read still proceeded optimistically.
#[inline]
pub fn note_consult_opt() {
    if enabled() {
        CONSULT_OPT.fetch_add(1, Ordering::Relaxed);
    }
}

/// Counters for one SpecFence block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InblockSnap {
    /// Locations whose first hot conflict armed WaitOnce.
    pub arm_n: usize,
    /// Transaction that armed the first location. `usize::MAX` if none.
    pub first_arm_tx: usize,
    /// Nanoseconds from the block origin to that first arm.
    pub first_arm_ns: u64,
    /// Protected reads that found no writer to wait for.
    pub consult_no_pred: usize,
    /// Protected reads that returned to the optimistic path.
    pub consult_opt: usize,
}

/// Snapshot. Zeros when the flag is off.
pub fn snapshot() -> InblockSnap {
    if !enabled() {
        return InblockSnap {
            arm_n: 0,
            first_arm_tx: usize::MAX,
            first_arm_ns: 0,
            consult_no_pred: 0,
            consult_opt: 0,
        };
    }
    InblockSnap {
        arm_n: ARM_N.load(Ordering::Relaxed),
        first_arm_tx: FIRST_ARM_TX.load(Ordering::Relaxed),
        first_arm_ns: FIRST_ARM_NS.load(Ordering::Relaxed),
        consult_no_pred: CONSULT_NO_PRED.load(Ordering::Relaxed),
        consult_opt: CONSULT_OPT.load(Ordering::Relaxed),
    }
}
