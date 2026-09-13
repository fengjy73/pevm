//! SpecFence Mode(a) — decide + cost-aware prior (v6 `mode.rs`).
//!
//! Authoritative plant: `lab/notes/specfence-complete-architecture-v6-essence.md`.
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
    /// Stale PE may Unfence when independence is certified (FM9 consume).
    pub independence_certified: bool,
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
    let predicted = learner.predicted_essential(location, access_k);
    if !predicted {
        return AccessDecision::UnfencedOcc {
            predicted: false,
            roi_skip: false,
        };
    }
    let Some(vis) = vis else {
        // Prior PE with empty visibility stays Spec (quiet Bind-tax protection).
        return AccessDecision::UnfencedOcc {
            predicted: true,
            roi_skip: true,
        };
    };

    // FM9: independence Unfences a stale prior PE (not a Wait OR-door).
    if vis.independence_certified
        && vis.unfinished == 0
        && !learner.predicted_essential_intra(location, access_k)
    {
        return AccessDecision::UnfencedOcc {
            predicted: true,
            roi_skip: true,
        };
    }

    // SoT §3.2 — event-driven + cost-aware prior-PE Fire.
    // HotSet / WŜ are posterior / ready-edge priors, not SerialLane OR-doors.
    // Multi-writer PE class first — do **not** Bind stale Data (theater).
    let intra = learner.predicted_essential_intra(location, access_k);
    // Fence only on fan_out (EV win). Spine/quiet intra-Fence is tax
    // (19807137 0.30; 2179522 Bind livelock). Isolated prior is empty.
    let fan = learner.morph_weights().dominant_fan_out();
    let park_ok = fan && !learner.quiet_fence_off() && (intra || learner.prior_pe_fire_wins(vis));
    if vis.unfinished > 1 || (vis.in_serial_lane && vis.unfinished > 0) {
        // SerialLane parks only the executing head. Ready-head park
        // serializes satellites before they plant ESTIMATE and inflates
        // B0 (14689597 abort 229 vs OCC 37).
        if park_ok && vis.writer_executing && let Some(w) = vis.writer {
            return AccessDecision::SerialLane { writer: w };
        }
    }
    if vis.unfinished == 1 && vis.writer_executing {
        if park_ok && let Some(w) = vis.writer {
            return AccessDecision::WaitFor { writer: w };
        }
    }
    // Bind-on-Data is stale when later writers are not yet in MV (14689597
    // 550 vs OCC 66). WaitFor(executing) is the only timely Fence.
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
            independence_certified: false,
        }
    }

    fn data_plus_multi() -> AccessVis {
        AccessVis {
            published_data: true,
            writer: Some(0),
            writer_executing: true,
            unfinished: 3,
            in_serial_lane: false,
            hot: true,
            ws_hat: true,
            independence_certified: false,
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
            independence_certified: false,
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
            independence_certified: false,
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
    fn prior_pe_plus_data_is_not_stale_bind() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        assert_eq!(
            decide(&live, 7, 6, Some(&data_vis())),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            },
            "Bind-on-Data is stale-writer theater; WaitFor(executing) only"
        );
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
        assert_eq!(
            decide(&live, 7, 6, Some(&data_plus_multi())),
            AccessDecision::SerialLane { writer: 0 },
            "published Data must not Bind-theater a multi-writer PE class"
        );
        let mut ready_multi = data_plus_multi();
        ready_multi.writer_executing = false;
        assert_eq!(
            decide(&live, 7, 6, Some(&ready_multi)),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            },
            "Ready multi-writer must not SerialLane-park (ESTIMATE cascade)"
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
    fn quiet_intra_pe_stays_spec_until_heat() {
        // 2179522: one abort must not Bind-tax / livelock the quiet cohort.
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
        assert_eq!(
            decide(&live, 7, 6, Some(&data_vis())),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            }
        );
    }

    #[test]
    fn hot_is_not_a_serial_lane_door() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        let mut vis = exec_vis(2);
        vis.hot = true;
        vis.unfinished = 1;
        assert_eq!(
            decide(&live, 7, 6, Some(&vis)),
            AccessDecision::WaitFor { writer: 2 },
            "HotSet must not promote a single writer to SerialLane"
        );
    }

    #[test]
    fn prior_only_executing_on_quiet_is_roi_skip() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        live.seed_predicted_essential(7, 6);
        assert_eq!(
            decide(&live, 7, 6, Some(&exec_vis(2))),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            },
            "T3: prior-PE WaitFor without EV win is Fence tax"
        );
        assert_eq!(
            decide(&live, 7, 6, Some(&data_vis())),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            },
            "T3: quiet prior-PE Bind-on-Data is Fence tax"
        );
    }

    #[test]
    fn independence_unfences_stale_prior() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        let mut vis = data_vis();
        vis.published_data = false;
        vis.independence_certified = true;
        vis.ws_hat = false;
        assert_eq!(
            decide(&live, 7, 6, Some(&vis)),
            AccessDecision::UnfencedOcc {
                predicted: true,
                roi_skip: true
            }
        );
    }
}
