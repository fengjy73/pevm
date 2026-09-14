//! Pessimistic-admit policy — extracted from the EVM host (`vm.rs`).
//!
//! File-SRP: interpreter host ≠ admit-verb body. `vm` calls these helpers;
//! WaitFor is **wait_for_dependency** (`WaitForDependency`), not Aborting+steal-without-park.
//! Ordered admit after Done is **not** an `OrderedAdmit` verb.
//!
//! Vocabulary: `lab/notes/specfence-cc-glossary.md`.
//! Plant SoT: `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md` §2.4.

use super::wave::ParkKind;
use crate::TxIdx;
use crate::scheduler::Scheduler;

/// Policy outcome for an admitted Fence verb (CC act).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FenceAct {
    /// Writer already Done — OCC read; **not** OrderedAdmit-after-Done theater.
    DoneOptimisticRead { cert: bool },
    /// Writer Ready/Validated/Aborting — canary optimistic_read; edges already reserved.
    ReadyCanary,
    /// Writer Executing — WaitForDependency (park without steal-convert).
    WaitForDependency { writer: TxIdx },
}

/// WaitFor / SerialLane writer act. Never OrderedAdmit-counts a Done fallthrough.
/// Ready producer is WaitForDependency (not ReadyCanary optimistic_read) — known-edge canary is
/// the leak that forces mid-tx Wait + OCC abort. Aborting stays canary
/// (WaitForDependency behind Aborting deadlocks — no progress path).
#[inline]
pub(crate) fn act_wait_for(scheduler: &Scheduler, writer: TxIdx) -> FenceAct {
    if scheduler.is_done(writer) {
        // DoneOptimisticRead cert is partial_abort bait (sibling optimistic_read stays uncertified).
        return FenceAct::DoneOptimisticRead { cert: false };
    }
    if scheduler.is_executing(writer) || scheduler.is_ready(writer) {
        return FenceAct::WaitForDependency { writer };
    }
    FenceAct::ReadyCanary
}

/// SerialLane exclusive: Done → optimistic_read cert; Executing → wait_for_dependency; else canary.
#[inline]
pub(crate) fn act_serial_lane(scheduler: &Scheduler, writer: TxIdx) -> FenceAct {
    act_wait_for(scheduler, writer)
}

/// OrderedAdmit-rare: published Data present. Caller already checked tip+EV.
#[inline]
pub(crate) fn act_ordered_admit_has_data(has_data: bool) -> bool {
    has_data
}

/// OrderedAdmit EV from the Bayes port. `known_star` is a **wait_for_dependency / WaitFor** signal,
/// not a OrderedAdmit door — OR-ing it in was the volume bug (N=1 OrderedAdmit ≈ WaitFor).
/// Quiet-off still holds OrderedAdmit unless the star posterior is on (2179522).
#[inline]
pub(crate) fn ordered_admit_ev_from_query(
    ev_ordered_admit_beats_full_abort: bool,
    quiet_off: bool,
    known_star: bool,
) -> bool {
    ev_ordered_admit_beats_full_abort && (!quiet_off || known_star)
}

/// ESTIMATE Avoid: PE-known RAW is WaitForDependency (not BlockingOther Aborting default).
/// Unknown ESTIMATE stays BlockingOther (true OCC).
#[inline]
pub(crate) fn estimate_park_kind(pe_known: bool) -> ParkKind {
    if pe_known {
        ParkKind::WaitForDependency
    } else {
        ParkKind::BlockingOther
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_admit_requires_data() {
        assert!(act_ordered_admit_has_data(true));
        assert!(!act_ordered_admit_has_data(false));
    }

    #[test]
    fn estimate_pe_known_is_wait_for_dependency() {
        assert_eq!(estimate_park_kind(true), ParkKind::WaitForDependency);
        assert_eq!(estimate_park_kind(false), ParkKind::BlockingOther);
    }

    #[test]
    fn ready_producer_is_wait_for_dependency_not_canary() {
        let s = Scheduler::new(3);
        assert!(s.is_ready(0));
        assert_eq!(
            act_wait_for(&s, 0),
            FenceAct::WaitForDependency { writer: 0 },
            "known Ready producer must WaitForDependency, not ReadyCanary optimistic_read"
        );
        let v = s.try_execute_producer(0).unwrap();
        assert!(s.is_executing(0));
        assert_eq!(
            act_wait_for(&s, 0),
            FenceAct::WaitForDependency { writer: 0 }
        );
        let _ = s.finish_execution(
            crate::TxVersion {
                tx_idx: v.tx_idx,
                tx_incarnation: v.tx_incarnation,
            },
            crate::FinishExecFlags::empty(),
        );
        assert!(
            matches!(
                act_wait_for(&s, 0),
                FenceAct::DoneOptimisticRead { cert: false }
            ),
            "DoneOptimisticRead must not write partial_abort bait cert"
        );
    }

    #[test]
    fn known_star_is_not_ordered_admit_ev() {
        assert!(
            !ordered_admit_ev_from_query(false, false, true),
            "star without OrderedAdmit EV must not OrderedAdmit"
        );
        assert!(ordered_admit_ev_from_query(true, false, false));
        assert!(
            !ordered_admit_ev_from_query(true, true, false),
            "quiet-off holds OrderedAdmit unless star posterior"
        );
        assert!(ordered_admit_ev_from_query(true, true, true));
    }
}
