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
/// When fan / InterPrior / seeded stars are present, OrderedAdmit from 2 txs.
const HINT_STAR_FLOOR: usize = 2;

/// begin_block admit: seed PE for known stars; OrderedAdmit hinted accounts.
///
/// Block-local ≥16-tx accounts are star evidence even on quiet-biased cold
/// start / empty InterPrior (hints≥16-only **after** a morph/prior gate was
/// too weak — first-wave satellites executed before edges existed).
/// Truly empty hints + no stars seeds nothing — Mode(a)=Spec on the same spine.
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
    let star_edges = fan || prior_star || stars > 0;
    let floor = if star_edges {
        HINT_STAR_FLOOR
    } else {
        HINT_FANOUT_FLOOR
    };
    let mut edges = 0;
    for addr in hints.accounts() {
        if addr == beneficiary {
            continue;
        }
        let txs = hints.txs(&addr);
        if txs.len() < floor {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specfence::AccountHints;
    use alloy_primitives::Address;

    #[test]
    fn hints_fanout_seeds_without_prior() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(crate::specfence::learner::MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let addr = Address::repeat_byte(0x11);
        let hints = AccountHints::from_account_txs(addr, (0..20).collect());
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
        );
        assert!(
            n >= 19,
            "quiet-biased cold start must still OrderedAdmit ≥16-tx stars, got {n}"
        );
        assert!(!ready.may_execute(19));
        assert_eq!(ready.blocking_producer(19), Some(0));
        assert!(stages.is_reserved(0));
    }

    #[test]
    fn quiet_small_account_does_not_seed() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(crate::specfence::learner::MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let addr = Address::repeat_byte(0x22);
        let hints = AccountHints::from_account_txs(addr, vec![0, 1, 2]);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
        );
        assert_eq!(n, 0, "quiet 3-tx account is not a star");
        assert!(ready.may_execute(2));
    }
}
