//! SpecFence access policy — frozen-π gate, not a `Vm` intercept.
//!
//! Authoritative plant: `lab/notes/specfence-clean-slate-architecture.md`.
//! π: `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`.
//!
//! Unfenced ⇒ caller **must** invoke the shared OCC read helper. This module
//! never touches Edge / rem journal / process DashMap / FF.

use crate::MemoryLocationHash;

use super::learner::LiveLearner;

/// Live verb after the PredictedEssential / ROI gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessDecision {
    /// Compile to OCC `storage`/`basic` (no SpecFence body).
    UnfencedOcc { predicted: bool, roi_skip: bool },
    /// Open the thin PCC overlay (Bind / WaitFor using \(e_{\mathrm{vis}}\)).
    TryPcc { access_k: u32 },
}

/// Frozen-π gate for **this** access \(a=(t,k,\mathrm{depth},\ell)\).
///
/// `inc` is not an Avoid key. `force_prefix` / canary / H-OR / morph actuator
/// are not consulted.
#[inline]
pub(crate) fn decide(
    learner: &LiveLearner,
    location: MemoryLocationHash,
    access_k: u32,
) -> AccessDecision {
    if !learner.has_any_predicted() {
        return AccessDecision::UnfencedOcc {
            predicted: false,
            roi_skip: false,
        };
    }
    let mut predicted = learner.predicted_essential(location, access_k);
    if predicted
        && learner.quiet_fence_off()
        && !learner.predicted_essential_intra(location, access_k)
    {
        predicted = false;
    }
    if !predicted {
        return AccessDecision::UnfencedOcc {
            predicted: false,
            roi_skip: false,
        };
    }
    if !learner.pcc_makespan_win(location, access_k) {
        return AccessDecision::UnfencedOcc {
            predicted: true,
            roi_skip: true,
        };
    }
    AccessDecision::TryPcc { access_k }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specfence::learner::{LiveLearner, MorphWeights};

    #[test]
    fn empty_pe_is_unfenced_occ() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        assert_eq!(
            decide(&live, 7, 6),
            AccessDecision::UnfencedOcc {
                predicted: false,
                roi_skip: false
            }
        );
    }

    #[test]
    fn prior_only_pe_is_unfenced_roi_skip() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights {
            fan_out: 0.70,
            mixed: 0.15,
            waw_spine: 0.10,
            quiet: 0.05,
        });
        live.seed_predicted_essential(7, 6);
        assert_eq!(
            decide(&live, 7, 6),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            }
        );
    }

    #[test]
    fn intra_abort_pe_opens_pcc() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights {
            fan_out: 0.70,
            mixed: 0.15,
            waw_spine: 0.10,
            quiet: 0.05,
        });
        live.note_abort_access(7, 2, Some(6));
        assert_eq!(decide(&live, 7, 6), AccessDecision::TryPcc { access_k: 6 });
        assert_eq!(
            decide(&live, 7, 12),
            AccessDecision::UnfencedOcc {
                predicted: false,
                roi_skip: false
            }
        );
    }

    #[test]
    fn quiet_lone_abort_stays_unfenced() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        live.note_abort_access(7, 2, Some(6));
        let d = decide(&live, 7, 6);
        assert!(
            matches!(d, AccessDecision::UnfencedOcc { .. }),
            "quiet_fence_off must not open PCC: {d:?}"
        );
    }
}
