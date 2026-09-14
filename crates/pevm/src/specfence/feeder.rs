//! Learner **feeder** — PE / morph / tax observations into Bayes ports.
//!
//! File-SRP (v9.4): this is **not** Mode(a) decide. Decide lives in
//! [`super::access_policy`] and **queries** Bayes. `LiveLearner` remains the
//! PE store; this module is the seed / observe API used at begin_block and
//! abort.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v9.4-file-srp.md`.

use crate::MemoryLocationHash;

use super::bayes::BayesMap;
use super::learner::{InterBlockPrior, LiveLearner, TopLocPrior};

/// Known-star prior: abort or fan evidence that must seed PE even on quiet morph.
#[inline]
pub(crate) fn top_is_known_star(top: &TopLocPrior) -> bool {
    top.k_template > 0 && (top.abort_rate >= 0.15 || top.fanout_ema >= 16.0)
}

/// Feed InterPrior stars into PE + Bayes. Quiet morph must **not** skip stars (M4).
pub(crate) fn seed_known_stars(
    learner: &LiveLearner,
    bayes: &BayesMap,
    prior: &InterBlockPrior,
) -> usize {
    let mut n = 0;
    for top in prior.top_locations() {
        if !top_is_known_star(&top) {
            continue;
        }
        learner.seed_predicted_essential(top.location, top.k_template);
        let _ = bayes.observe_conflict_location(top.location);
        n += 1;
    }
    n
}

/// True when feeder has no PE and Bayes is cold (optimistic_read cost class, same spine).
#[inline]
pub(crate) fn feeder_is_cold(learner: &LiveLearner, bayes: &BayesMap) -> bool {
    !learner.has_any_predicted() && bayes.is_cold()
}

/// Abort observation — PE(true-k) + Bayes conflict. Never sprays templates.
#[inline]
pub(crate) fn observe_abort(
    learner: &LiveLearner,
    bayes: &BayesMap,
    location: MemoryLocationHash,
    cascade_hint: usize,
    loc_k: Option<u32>,
) {
    learner.note_abort_access(location, cascade_hint, loc_k);
    bayes.observe_conflict_location_always(location);
}
