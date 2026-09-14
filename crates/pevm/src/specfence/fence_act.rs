//! Fence-act policy — extracted from the EVM host (`vm.rs`).
//!
//! File-SRP: interpreter host ≠ Fence verb body. `vm` calls these helpers;
//! WaitFor is **PinWithoutThrow** (PinHold), not Aborting+steal-without-park.
//! Bind-after-Done is **not** a Bind verb.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` §2.4.

use super::wave::ParkKind;
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
/// Ready producer is PinHold (not ReadyCanary Spec) — known-edge canary is
/// the leak that forces mid-tx Wait + OCC abort. Aborting stays canary
/// (PinHold behind Aborting deadlocks — no progress path).
#[inline]
pub(crate) fn act_wait_for(scheduler: &Scheduler, writer: TxIdx) -> FenceAct {
    if scheduler.is_done(writer) {
        // DoneUnfenced cert is R1 bait (sibling Spec stays uncertified).
        return FenceAct::DoneUnfenced { cert: false };
    }
    if scheduler.is_executing(writer) || scheduler.is_ready(writer) {
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

/// Bind EV from the Bayes port. `known_star` is a **pin / WaitFor** signal,
/// not a Bind door — OR-ing it in was the volume bug (N=1 Bind ≈ WaitFor).
/// Quiet-off still holds Bind unless the star posterior is on (2179522).
#[inline]
pub(crate) fn bind_ev_from_query(
    ev_bind_beats_b0: bool,
    quiet_off: bool,
    known_star: bool,
) -> bool {
    ev_bind_beats_b0 && (!quiet_off || known_star)
}

/// ESTIMATE Avoid: PE-known RAW is PinHold (not BlockingOther Aborting default).
/// Unknown ESTIMATE stays BlockingOther (true OCC).
#[inline]
pub(crate) fn estimate_park_kind(pe_known: bool) -> ParkKind {
    if pe_known {
        ParkKind::PinHold
    } else {
        ParkKind::BlockingOther
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_requires_data() {
        assert!(act_bind_has_data(true));
        assert!(!act_bind_has_data(false));
    }

    #[test]
    fn estimate_pe_known_is_pinhold() {
        assert_eq!(estimate_park_kind(true), ParkKind::PinHold);
        assert_eq!(estimate_park_kind(false), ParkKind::BlockingOther);
    }

    #[test]
    fn ready_producer_is_pinhold_not_canary() {
        let s = Scheduler::new(3);
        assert!(s.is_ready(0));
        assert_eq!(
            act_wait_for(&s, 0),
            FenceAct::PinHold { writer: 0 },
            "known Ready producer must PinHold, not ReadyCanary Spec"
        );
        let v = s.try_execute_producer(0).unwrap();
        assert!(s.is_executing(0));
        assert_eq!(act_wait_for(&s, 0), FenceAct::PinHold { writer: 0 });
        let _ = s.finish_execution(
            crate::TxVersion {
                tx_idx: v.tx_idx,
                tx_incarnation: v.tx_incarnation,
            },
            crate::FinishExecFlags::empty(),
        );
        assert!(
            matches!(act_wait_for(&s, 0), FenceAct::DoneUnfenced { cert: false }),
            "DoneUnfenced must not write R1-bait cert"
        );
    }

    #[test]
    fn known_star_is_not_bind_ev() {
        assert!(
            !bind_ev_from_query(false, false, true),
            "star without Bind EV must not Bind"
        );
        assert!(bind_ev_from_query(true, false, false));
        assert!(
            !bind_ev_from_query(true, true, false),
            "quiet-off holds Bind unless star posterior"
        );
        assert!(bind_ev_from_query(true, true, true));
    }
}
