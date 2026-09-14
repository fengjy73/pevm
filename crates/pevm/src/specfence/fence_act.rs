//! Fence-act policy — extracted from the EVM host (`vm.rs`).
//!
//! File-SRP: interpreter host ≠ Fence verb body. `vm` calls these helpers;
//! WaitFor is **PinWithoutThrow** (PinHold), not Aborting+steal-without-park.
//! Bind-after-Done is **not** a Bind verb.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` §2.4.

use crate::TxIdx;
use crate::scheduler::Scheduler;

/// Policy outcome for an admitted Fence verb (CC act).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FenceAct {
    /// Writer already Done — OCC read; **not** Bind-after-Done theater.
    DoneUnfenced { cert: bool },
    /// Writer Ready/Validated/Aborting — canary Spec; edges already reserved.
    ReadyCanary,
    /// Writer Executing — PinHold (park without steal-convert).
    PinHold { writer: TxIdx },
}

/// WaitFor / SerialLane writer act. Never Bind-counts a Done fallthrough.
#[inline]
pub(crate) fn act_wait_for(scheduler: &Scheduler, writer: TxIdx) -> FenceAct {
    if scheduler.is_done(writer) {
        return FenceAct::DoneUnfenced { cert: true };
    }
    if scheduler.is_executing(writer) {
        return FenceAct::PinHold { writer };
    }
    FenceAct::ReadyCanary
}

/// SerialLane exclusive: Done → unfenced cert; Executing → pin; else canary.
#[inline]
pub(crate) fn act_serial_lane(scheduler: &Scheduler, writer: TxIdx) -> FenceAct {
    act_wait_for(scheduler, writer)
}

/// Bind-rare: published Data present. Caller already checked tip+EV.
#[inline]
pub(crate) fn act_bind_has_data(has_data: bool) -> bool {
    has_data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_requires_data() {
        assert!(act_bind_has_data(true));
        assert!(!act_bind_has_data(false));
    }
}
