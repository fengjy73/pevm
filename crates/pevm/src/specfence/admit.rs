//! PC admit_seed — ReadyEdges + ProducerStage **before** satellite Execute.
//!
//! Call order (v9.1): Bayes → admit_seed → Execute(only if admitted).
//! Mid-tx `access_vis` may refresh edges; it must not be the first insert.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v9.1-cc-pc-bayes.md`.

use alloy_primitives::Address;

use super::AccountHints;
use super::bayes::BayesMap;
use super::feeder::{seed_known_stars, top_is_known_star};
use super::learner::{InterBlockPrior, LiveLearner};
use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;

/// High-fanout account: OrderedAdmit later hinted txs behind the earliest.
const HINT_FANOUT_FLOOR: usize = 16;

/// begin_block admit: seed PE for known stars; optionally OrderedAdmit
/// high-fanout hinted accounts when fan_out / star evidence is present.
///
/// Truly cold (empty PE ∧ no stars ∧ Bayes.cold) seeds nothing — Mode(a)=Spec
/// on the **same** spine (v9.3). Does **not** flip to an OCC computer.
pub(crate) fn admit_seed_begin_block(
    ready: &ReadyEdgeTable,
    stages: &ProducerStageTable,
    learner: &LiveLearner,
    bayes: &BayesMap,
    prior: &InterBlockPrior,
    hints: &AccountHints,
    beneficiary: Address,
) -> usize {
    let stars = seed_known_stars(learner, bayes, prior);
    let fan = learner.morph_weights().dominant_fan_out();
    let prior_star = prior.top_locations().iter().any(top_is_known_star);
    if stars == 0 && !fan && !prior_star {
        return 0;
    }
    let mut edges = 0;
    for addr in hints.accounts() {
        if addr == beneficiary {
            continue;
        }
        let txs = hints.txs(&addr);
        if txs.len() < HINT_FANOUT_FLOOR {
            continue;
        }
        let producer = txs[0];
        stages.reserve(producer);
        for &t in &txs[1..] {
            ready.note_consumer(t, producer);
            edges += 1;
        }
    }
    edges
}

/// Abort strengthen: known consumer ← discovered RAW producer + ProducerStage.
#[inline]
pub(crate) fn admit_seed_on_abort(
    ready: &ReadyEdgeTable,
    stages: &ProducerStageTable,
    consumer: crate::TxIdx,
    producer: crate::TxIdx,
    location: crate::MemoryLocationHash,
) {
    if producer >= consumer {
        return;
    }
    ready.note_raw_producer(location, producer);
    ready.note_consumer(consumer, producer);
    stages.reserve(producer);
}
