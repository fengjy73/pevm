//! SpecFence access policy — Mode(a) from \(a + e_{\mathrm{vis}} + \mathrm{PE} + learning\).
//!
//! Authoritative plant: `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md`.
//! π: `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`.
//!
//! Unfenced ⇒ caller **must** invoke the shared OCC read helper. This module
//! never touches rem journal / FF.

use crate::{MemoryLocationHash, TxIdx};

use super::learner::LiveLearner;

/// Visibility + learning features gathered **only** on a PE hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AccessVis {
    pub published_data: bool,
    pub writer: Option<TxIdx>,
    pub writer_executing: bool,
    pub unfinished: usize,
    pub in_serial_lane: bool,
    pub hot: bool,
    pub ws_hat: bool,
}

/// Live verb after the PredictedEssential / \(e_{\mathrm{vis}}\) gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessDecision {
    /// Compile to OCC `storage`/`basic` (no SpecFence body).
    UnfencedOcc { predicted: bool, roi_skip: bool },
    /// Fence: Bind published Data for this \(a\).
    Bind,
    /// Fence: WaitFor a **single** executing writer.
    WaitFor { writer: TxIdx },
    /// Fence: serial-lane / ordered-admit on multi-writer PE class.
    SerialLane { writer: TxIdx },
}

/// Frozen-π + v5 fusion gate for **this** access \(a=(t,k,\mathrm{depth},\ell)\).
///
/// `vis` is `None` when the caller has not gathered \(e_{\mathrm{vis}}\)
/// (empty PE / ¬PE). Prior PE **may** Fence when `vis` says Data or a
/// single executing writer — that is the fusion hinge, not `mark_pcc(tx)`.
#[inline]
pub(crate) fn decide(
    learner: &LiveLearner,
    location: MemoryLocationHash,
    access_k: u32,
    vis: Option<&AccessVis>,
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
        // Quiet + prior-only: still allow Bind-on-Data below; other verbs stay Spec.
        if vis.is_none_or(|v| !v.published_data) {
            predicted = false;
        }
    }
    if !predicted {
        return AccessDecision::UnfencedOcc {
            predicted: false,
            roi_skip: false,
        };
    }
    let Some(vis) = vis else {
        return AccessDecision::UnfencedOcc {
            predicted: true,
            roi_skip: true,
        };
    };

    // A3: PE ∧ published Data → Bind. Prior PE may take this path.
    // writer_validated is not a Bind gate. WŜ is a feature, not an OR-door.
    if vis.published_data {
        return AccessDecision::Bind;
    }

    // Quiet: Bind-on-Data only (2179522). No WaitFor / lane.
    if learner.quiet_fence_off() {
        return AccessDecision::UnfencedOcc {
            predicted: true,
            roi_skip: true,
        };
    }
    if learner.park_storm() {
        return AccessDecision::UnfencedOcc {
            predicted: true,
            roi_skip: true,
        };
    }

    if vis.unfinished == 1 && vis.writer_executing {
        if let Some(w) = vis.writer {
            return AccessDecision::WaitFor { writer: w };
        }
    }
    // Multi-writer PE class / HotSet / already-laned ℓ → serial-lane, not
    // Bind-only theater and not fleet WaitFor.
    if vis.unfinished > 1 || vis.in_serial_lane || vis.hot {
        if let Some(w) = vis.writer {
            return AccessDecision::SerialLane { writer: w };
        }
    }
    AccessDecision::UnfencedOcc {
        predicted: true,
        roi_skip: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specfence::learner::{LiveLearner, MorphWeights};

    fn fan_out_learner() -> LiveLearner {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights {
            fan_out: 0.70,
            mixed: 0.15,
            waw_spine: 0.10,
            quiet: 0.05,
        });
        live
    }

    fn data_vis() -> AccessVis {
        AccessVis {
            published_data: true,
            writer: Some(1),
            writer_executing: false,
            unfinished: 0,
            in_serial_lane: false,
            hot: false,
            ws_hat: true,
        }
    }

    fn exec_vis(w: usize) -> AccessVis {
        AccessVis {
            published_data: false,
            writer: Some(w),
            writer_executing: true,
            unfinished: 1,
            in_serial_lane: false,
            hot: false,
            ws_hat: false,
        }
    }

    fn multi_vis(w: usize) -> AccessVis {
        AccessVis {
            published_data: false,
            writer: Some(w),
            writer_executing: true,
            unfinished: 3,
            in_serial_lane: false,
            hot: true,
            ws_hat: false,
        }
    }

    #[test]
    fn empty_pe_is_unfenced_occ() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        assert_eq!(
            decide(&live, 7, 6, None),
            AccessDecision::UnfencedOcc {
                predicted: false,
                roi_skip: false
            }
        );
    }

    #[test]
    fn prior_pe_without_vis_is_roi_skip() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        assert_eq!(
            decide(&live, 7, 6, None),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            }
        );
    }

    #[test]
    fn prior_pe_plus_data_is_bind() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        assert_eq!(decide(&live, 7, 6, Some(&data_vis())), AccessDecision::Bind);
    }

    #[test]
    fn prior_pe_plus_executing_writer_is_waitfor() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        assert_eq!(
            decide(&live, 7, 6, Some(&exec_vis(2))),
            AccessDecision::WaitFor { writer: 2 }
        );
    }

    #[test]
    fn multi_writer_pe_is_serial_lane() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        assert_eq!(
            decide(&live, 7, 6, Some(&multi_vis(0))),
            AccessDecision::SerialLane { writer: 0 }
        );
    }

    #[test]
    fn intra_abort_pe_opens_pcc() {
        let live = fan_out_learner();
        live.note_abort_access(7, 2, Some(6));
        assert_eq!(
            decide(&live, 7, 6, Some(&exec_vis(1))),
            AccessDecision::WaitFor { writer: 1 }
        );
        assert_eq!(
            decide(&live, 7, 12, None),
            AccessDecision::UnfencedOcc {
                predicted: false,
                roi_skip: false
            }
        );
    }

    #[test]
    fn quiet_lone_abort_stays_unfenced_unless_data() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        live.note_abort_access(7, 2, Some(6));
        assert_eq!(
            decide(&live, 7, 6, Some(&exec_vis(1))),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            }
        );
        assert_eq!(decide(&live, 7, 6, Some(&data_vis())), AccessDecision::Bind);
    }
}
