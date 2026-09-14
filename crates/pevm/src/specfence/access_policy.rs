//! SpecFence Mode(a) — CC Avoid decide (WaitFor/lane primary; OrderedAdmit rare).
//!
//! Authoritative plant: `lab/notes/specfence-complete-architecture-v8-parallel-computer.md`.
//! π: `lab/notes/specfence-complete-architecture-v4-frozen-grain.md`.
//!
//! OptimisticRead ⇒ caller **must** invoke the shared OCC read helper. This module
//! never touches rem journal / FF. CC is a first-class control plane — not
//! an edge-annotation layer.

use crate::{MemoryLocationHash, TxIdx};

use super::bayes::BayesAccessQuery;
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
    /// Stale PE may optimistic_read when independence is certified (FM9 consume).
    pub independence_certified: bool,
    /// OrderedAdmit-rare: MV tip identity == predicted RAW producer for this \(\ell\).
    pub tip_is_conflict_producer: bool,
}

/// Live verb after the PredictedEssential / \(e_{\mathrm{vis}}\) gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessDecision {
    /// Compile to OCC `storage`/`basic` (no SpecFence body).
    OptimisticReadOcc { predicted: bool, roi_skip: bool },
    /// Pessimistic admit: OrderedAdmit published Data for this \(a\).
    OrderedAdmit,
    /// Pessimistic admit: WaitFor a **single** executing writer.
    WaitFor { writer: TxIdx },
    /// Pessimistic admit: serial-lane / ordered admit on multi-writer PE class.
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
    decide_queried(learner, location, access_k, vis, None)
}

/// Live decide ← Bayes query ports (v9.1). Tests use [`decide`] (no query).
#[inline]
pub(crate) fn decide_queried(
    learner: &LiveLearner,
    location: MemoryLocationHash,
    access_k: u32,
    vis: Option<&AccessVis>,
    bayes: Option<BayesAccessQuery>,
) -> AccessDecision {
    if !learner.has_any_predicted() {
        return AccessDecision::OptimisticReadOcc {
            predicted: false,
            roi_skip: false,
        };
    }
    let predicted = learner.predicted_essential(location, access_k);
    if !predicted {
        return AccessDecision::OptimisticReadOcc {
            predicted: false,
            roi_skip: false,
        };
    }
    let Some(vis) = vis else {
        // Prior PE with empty visibility stays Spec (quiet OrderedAdmit-tax protection).
        return AccessDecision::OptimisticReadOcc {
            predicted: true,
            roi_skip: true,
        };
    };

    // FM9: independence takes optimistic_read on a stale prior PE (not a Wait OR-door).
    if vis.independence_certified
        && vis.unfinished == 0
        && !learner.predicted_essential_intra(location, access_k)
    {
        return AccessDecision::OptimisticReadOcc {
            predicted: true,
            roi_skip: true,
        };
    }

    // SoT §3.2 — WaitFor/lane primary; OrderedAdmit only conflict-tip ∧ EV.
    // HotSet / WŜ are posterior / ready-edge priors, not SerialLane OR-doors.
    let intra = learner.predicted_essential_intra(location, access_k);
    let fan = learner.morph_weights().dominant_fan_out();
    let quiet_off = learner.quiet_pessimistic_off();
    // Known-star Bayes posterior opens WaitFor even on quiet morph (M4).
    // Lone intra abort on quiet stays OptimisticRead (2179522).
    let known_star = bayes.is_some_and(|q| q.known_star && !q.quiet_cold);
    // OR-bool adapter only when no Bayes query (tests / empty ports).
    // Live π is ev_wait_for_dependency_beats_abort / depth_frac — not ev_win as decide spine.
    let ev_adapter = !quiet_off && (intra || (fan && learner.prior_pe_fire_wins(vis)));
    let ev_wait_for_dependency = if let Some(q) = bayes {
        if quiet_off && !known_star {
            false
        } else {
            q.ev_wait_for_dependency_beats_abort || q.depth_frac >= 0.50 || known_star
        }
    } else {
        known_star || ev_adapter
    };
    // OrderedAdmit rare: tip==conflict ∧ OrderedAdmit EV only. `known_star` opens WaitFor,
    // never OrderedAdmit (fan 14689597 OrderedAdmit 472 vs Wait 47 was this OR).
    let ordered_admit_ev = if let Some(q) = bayes {
        crate::specfence::fence_act::ordered_admit_ev_from_query(
            q.ev_ordered_admit_beats_full_abort,
            quiet_off,
            known_star,
        )
    } else {
        ev_adapter
    };
    // WaitFor pins one *executing* producer. AbortingThrow last: low
    // depth_frac ∧ ¬ev_wait_for_dependency_beats_abort (already folded into ev_wait_for_dependency).
    if vis.unfinished == 1
        && vis.writer_executing
        && let Some(w) = vis.writer
        && ev_wait_for_dependency
    {
        return AccessDecision::WaitFor { writer: w };
    }
    // SerialLane: multi-writer PE class. Never OrderedAdmit while unfinished>0
    // (later writers not yet in MV — stale last_data theater).
    if vis.unfinished > 1 || (vis.in_serial_lane && vis.unfinished > 0) {
        if ev_wait_for_dependency
            && vis.writer_executing
            && let Some(w) = vis.writer
        {
            return AccessDecision::SerialLane { writer: w };
        }
    }
    // OrderedAdmit RARE: published tip must be the predicted RAW producer, EV win,
    // and OrderedAdmit must not be a tax (OrderedAdmit↑ ∧ abort not↓).
    if vis.unfinished == 0
        && vis.published_data
        && vis.tip_is_conflict_producer
        && ordered_admit_ev
        && !learner.ordered_admit_tax_losing()
    {
        return AccessDecision::OrderedAdmit;
    }
    AccessDecision::OptimisticReadOcc {
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
            tip_is_conflict_producer: true,
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
            tip_is_conflict_producer: false,
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
            tip_is_conflict_producer: false,
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
            tip_is_conflict_producer: false,
        }
    }

    #[test]
    fn empty_pe_is_optimistic_read_occ() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        assert_eq!(
            decide(&live, 7, 6, None),
            AccessDecision::OptimisticReadOcc {
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
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            }
        );
    }

    #[test]
    fn prior_pe_plus_data_and_unfinished0_is_ordered_admit() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        assert_eq!(
            decide(&live, 7, 6, Some(&data_vis())),
            AccessDecision::OrderedAdmit,
            "v8: Data ∧ unfinished=0 ∧ tip==conflict producer ∧ EV → OrderedAdmit"
        );
        let mut mere_data = data_vis();
        mere_data.tip_is_conflict_producer = false;
        assert_eq!(
            decide(&live, 7, 6, Some(&mere_data)),
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            },
            "v8: OrderedAdmit-on-any-Data is tax — tip must be the RAW producer"
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
            "published Data must not OrderedAdmit-theater a multi-writer PE class"
        );
        let mut ready_multi = data_plus_multi();
        ready_multi.writer_executing = false;
        assert_eq!(
            decide(&live, 7, 6, Some(&ready_multi)),
            AccessDecision::OptimisticReadOcc {
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
            AccessDecision::OptimisticReadOcc {
                predicted: false,
                roi_skip: false
            }
        );
    }

    #[test]
    fn quiet_intra_pe_waitfor_executing_but_does_not_ordered_admit() {
        // 2179522: one abort must not OrderedAdmit-tax the quiet cohort.
        // quiet_pessimistic_off also holds WaitFor until heat lifts.
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        live.note_abort_access(7, 2, Some(6));
        assert_eq!(
            decide(&live, 7, 6, Some(&exec_vis(1))),
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            }
        );
        assert_eq!(
            decide(&live, 7, 6, Some(&data_vis())),
            AccessDecision::OptimisticReadOcc {
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
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            },
            "T3: prior-PE WaitFor without EV win is Fence tax"
        );
        assert_eq!(
            decide(&live, 7, 6, Some(&data_vis())),
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            },
            "T3: quiet prior-PE OrderedAdmit-on-Data is Fence tax"
        );
    }

    #[test]
    fn ordered_admit_tax_trips_only_when_aborts_match_binds() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        for _ in 0..16 {
            live.note_ordered_admit_success(7);
        }
        live.note_abort_access(7, 2, Some(6));
        assert_eq!(
            decide(&live, 7, 6, Some(&data_vis())),
            AccessDecision::OrderedAdmit,
            "loose abort>0 trip lost 14689597 (965 aborts); residual abort alone is not enough"
        );
        for _ in 0..16 {
            live.note_abort_access(7, 2, Some(6));
        }
        assert_eq!(
            decide(&live, 7, 6, Some(&data_vis())),
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            },
            "aborts >= binds still trips OrderedAdmit"
        );
    }

    #[test]
    fn bayes_low_depth_does_not_waitfor() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        let q = crate::specfence::BayesAccessQuery {
            p_raw: 0.4,
            p_ordered_admit: 0.1,
            quiet_cold: false,
            depth_frac: 0.15,
            ev_wait_for_dependency_beats_abort: false,
            ev_ordered_admit_beats_full_abort: false,
            known_star: false,
        };
        assert_eq!(
            decide_queried(&live, 7, 6, Some(&exec_vis(2)), Some(q)),
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            },
            "decide←Bayes: low depth_frac ∧ ¬ev_wait_for_dependency_beats_abort → AbortingThrow last"
        );
    }

    #[test]
    fn bayes_wait_for_dependency_ev_opens_waitfor() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        let q = crate::specfence::BayesAccessQuery {
            p_raw: 0.5,
            p_ordered_admit: 0.1,
            quiet_cold: false,
            depth_frac: 0.85,
            ev_wait_for_dependency_beats_abort: true,
            ev_ordered_admit_beats_full_abort: false,
            known_star: false,
        };
        assert_eq!(
            decide_queried(&live, 7, 6, Some(&exec_vis(2)), Some(q)),
            AccessDecision::WaitFor { writer: 2 },
            "decide←Bayes: ev_wait_for_dependency_beats_abort shapes WaitFor"
        );
    }

    #[test]
    fn known_star_is_not_ordered_admit_ev() {
        // Star + OrderedAdmit EV off + published conflict tip → OptimisticRead, never OrderedAdmit.
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        let q = crate::specfence::BayesAccessQuery {
            p_raw: 0.5,
            p_ordered_admit: 0.1,
            quiet_cold: false,
            depth_frac: 0.10,
            ev_wait_for_dependency_beats_abort: false,
            ev_ordered_admit_beats_full_abort: false,
            known_star: true,
        };
        assert_eq!(
            decide_queried(&live, 7, 6, Some(&data_vis()), Some(q)),
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            },
            "OrderedAdmit rare: known_star is WaitFor/wait_for_dependency, not OrderedAdmit EV"
        );
    }

    #[test]
    fn ordered_admit_ev_opens_ordered_admit_on_conflict_tip() {
        let live = fan_out_learner();
        live.seed_predicted_essential(7, 6);
        let q = crate::specfence::BayesAccessQuery {
            p_raw: 0.4,
            p_ordered_admit: 0.4,
            quiet_cold: false,
            depth_frac: 0.15,
            ev_wait_for_dependency_beats_abort: false,
            ev_ordered_admit_beats_full_abort: true,
            known_star: false,
        };
        assert_eq!(
            decide_queried(&live, 7, 6, Some(&data_vis()), Some(q)),
            AccessDecision::OrderedAdmit,
            "OrderedAdmit rare: tip==conflict ∧ ev_ordered_admit_beats_full_abort"
        );
    }

    #[test]
    fn known_star_bayes_opens_waitfor_on_quiet() {
        let live = LiveLearner::new();
        live.begin_block(MorphWeights::default());
        live.seed_predicted_essential(7, 6);
        let q = crate::specfence::BayesAccessQuery {
            p_raw: 0.4,
            p_ordered_admit: 0.2,
            quiet_cold: false,
            depth_frac: 0.85,
            ev_wait_for_dependency_beats_abort: true,
            ev_ordered_admit_beats_full_abort: true,
            known_star: true,
        };
        assert_eq!(
            decide_queried(&live, 7, 6, Some(&exec_vis(2)), Some(q)),
            AccessDecision::WaitFor { writer: 2 },
            "M4: known-star posterior opens WaitFor despite quiet morph"
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
            AccessDecision::OptimisticReadOcc {
                predicted: true,
                roi_skip: true
            }
        );
    }
}
