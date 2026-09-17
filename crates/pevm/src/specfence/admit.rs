//! Admit seed — ReadyEdges + ProducerStage **before** satellite Execute.
//!
//! Detect uses real write locations: same-`from` Basic WAW, calldata RAW fan-out,
//! empty-`to` probe + intra-block write-set WAW on hidden locations (not
//! envelope `Basic(to)`). Avoid is a **predecessor chain** (wait-for
//! dependency) for WAW; RAW fan-out stays a star on the first producer.
//! Independents get no ReadyEdge / no PE.
//!
//! Plant SoT: `lab/notes/specfence-complete-architecture-v10-raw-mixed.md`
//! plus WAW-spine ordered admission (3356896). Soft=0. No Basic→Storage PE clone.

use alloy_primitives::Address;

use super::AccountHints;
use super::bayes::BayesMap;
use super::feeder::{seed_known_stars, top_is_known_star};
use super::learner::{InterBlockPrior, LiveLearner};
use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::wave::WaveParkTable;
use crate::{MemoryLocation, MemoryLocationHash, TxIdx, hash_deterministic};

/// RAW fan-out star (v10): ≥16 calldata calls to the same `to`.
const RAW_FANOUT_FLOOR: usize = 16;
/// Same-`from` is always a real Basic write (nonce / balance).
const FROM_WAW_FLOOR: usize = 2;
/// Short calldata WAW (storage trio, short call chains) — chain, no Basic(to) PE.
const CALL_WAW_FLOOR: usize = 2;
/// Empty-calldata `to` ≥2: probe the first tx only. Later txs wait-for that
/// prefix until its write-set Detects hidden WAW (then predecessor-chain) or
/// lazy-only (then release). Do **not** serialize the whole payee group at
/// begin-block (3356896 0x209c wall / independent tax).
const EMPTY_TO_PROBE_FLOOR: usize = 2;
/// Fan-star access class (k≈6 → bucket 4–7). RAW stars only.
const FAN_STAR_K: u32 = 6;

/// Chain `ordered[i]` behind `ordered[i-1]`. WAW Avoid — not a RAW star.
///
/// Do **not** ProducerStage-reserve every predecessor. `next_reserved()` is a
/// global min; reserving a long WAW spine serializes the whole block.
fn note_predecessor_chain(ready: &ReadyEdgeTable, ordered: &[TxIdx]) -> usize {
    let mut edges = 0;
    for pair in ordered.windows(2) {
        let (pred, succ) = (pair[0], pair[1]);
        if pred >= succ {
            continue;
        }
        ready.note_consumer(succ, pred);
        edges += 1;
    }
    edges
}

/// Park later txs behind the first (probe). Write-set then chains or releases.
fn note_probe_star(ready: &ReadyEdgeTable, ordered: &[TxIdx]) -> usize {
    if ordered.len() < 2 {
        return 0;
    }
    let probe = ordered[0];
    let mut edges = 0;
    for &succ in &ordered[1..] {
        if probe >= succ {
            continue;
        }
        ready.note_consumer(succ, probe);
        edges += 1;
    }
    edges
}

/// begin_block admit: WAW predecessor chains + RAW fan-out stars.
///
/// Independents (no same-from WAW, no calldata/hot-`to` chain) stay
/// `may_execute` — zero ReadyEdge / PE tax.
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
    let hint_fan = hints
        .from_accounts()
        .any(|a| a != beneficiary && hints.from_txs(&a).len() >= RAW_FANOUT_FLOOR)
        || hints
            .call_to_accounts()
            .any(|a| a != beneficiary && hints.call_to_txs(&a).len() >= RAW_FANOUT_FLOOR);
    let star_edges = fan || prior_star || stars > 0 || hint_fan;
    for top in prior.top_locations() {
        if top.k_template > 0
            && (hint_fan || top_is_known_star(&top) || bayes.query_admit(top.location))
        {
            learner.seed_predicted_essential(top.location, top.k_template);
        }
    }
    bayes.for_each_admit_hit(|loc| {
        learner.seed_predicted_essential(loc, FAN_STAR_K);
    });

    let mut edges = 0;

    // Same-from ≥2: real Basic(from) WAW. Chain each tx behind its predecessor.
    // Do **not** plant PE — that would tax independents via has_any_predicted.
    for addr in hints.from_accounts() {
        if addr == beneficiary {
            continue;
        }
        let txs = hints.from_txs(&addr);
        if txs.len() < FROM_WAW_FLOOR {
            continue;
        }
        let basic = hash_deterministic(MemoryLocation::Basic(addr));
        ready.note_raw_producer(basic, txs[0]);
        edges += note_predecessor_chain(ready, txs);
    }

    // Calldata ≥16: RAW fan-out star (v10). Basic(to) refuse-covers the account
    // class; storage PE stays InterPrior / Bayes / abort true-k only.
    for addr in hints.call_to_accounts() {
        if addr == beneficiary {
            continue;
        }
        let txs = hints.call_to_txs(&addr);
        if txs.len() < RAW_FANOUT_FLOOR {
            continue;
        }
        let producer = txs[0];
        stages.reserve(producer);
        let basic = hash_deterministic(MemoryLocation::Basic(addr));
        if star_edges {
            learner.seed_predicted_essential(basic, FAN_STAR_K);
        }
        ready.note_raw_producer(basic, producer);
        for &t in &txs[1..] {
            ready.note_consumer(t, producer);
            edges += 1;
        }
    }

    // Calldata 2..15: short contract WAW. Predecessor chain, no Basic(to) PE.
    for addr in hints.call_to_accounts() {
        if addr == beneficiary {
            continue;
        }
        let txs = hints.call_to_txs(&addr);
        if txs.len() < CALL_WAW_FLOOR || txs.len() >= RAW_FANOUT_FLOOR {
            continue;
        }
        edges += note_predecessor_chain(ready, txs);
    }

    // Empty-calldata `to` ≥2: probe-then-Detect. Independents with a unique
    // `to` are not in this loop. Popular lazy EOAs release after one write-set.
    for addr in hints.to_accounts() {
        if addr == beneficiary {
            continue;
        }
        if hints.call_to_txs(&addr).len() >= CALL_WAW_FLOOR {
            continue;
        }
        let txs = hints.to_txs(&addr);
        if txs.len() < EMPTY_TO_PROBE_FLOOR {
            continue;
        }
        edges += note_probe_star(ready, txs);
    }

    edges
}

/// Abort strengthen: known consumer ← discovered RAW/WAW producer + ProducerStage.
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

/// Intra-block Detect from a published non-lazy write-set.
///
/// Hidden writes (not Basic(from)) associate envelope `to` with the real
/// locations and upgrade a probe-star to a predecessor chain (wait-for the
/// immediate writer). Lazy-only publish **releases** the empty-`to` probe so
/// independent payees are not serialized.
pub(crate) fn admit_seed_on_write_set(
    ready: &ReadyEdgeTable,
    hints: &AccountHints,
    wave: &WaveParkTable,
    writer: TxIdx,
    from: Address,
    to: Option<Address>,
    write_locs: &[MemoryLocationHash],
) {
    let from_loc = hash_deterministic(MemoryLocation::Basic(from));
    if write_locs.iter().any(|&l| l == from_loc) {
        ready.note_raw_producer(from_loc, writer);
    }
    let Some(to) = to else {
        return;
    };
    let hidden: Vec<MemoryLocationHash> = write_locs
        .iter()
        .copied()
        .filter(|&l| l != from_loc)
        .collect();
    let later: Vec<TxIdx> = hints
        .to_txs(&to)
        .iter()
        .copied()
        .filter(|&t| t > writer)
        .collect();
    if later.is_empty() {
        return;
    }

    // RAW fan-out star: keep all consumers behind the first producer.
    if hints.call_to_txs(&to).len() >= RAW_FANOUT_FLOOR {
        for loc in hidden {
            ready.note_raw_producer(loc, writer);
        }
        return;
    }

    if hidden.is_empty() {
        let from_later: Vec<TxIdx> = hints
            .from_txs(&from)
            .iter()
            .copied()
            .filter(|&t| t > writer)
            .collect();
        for t in later {
            if from_later.contains(&t) {
                continue;
            }
            ready.release_consumer(t, wave);
        }
        return;
    }

    for loc in hidden {
        ready.note_raw_producer(loc, writer);
    }
    let mut ordered = Vec::with_capacity(later.len() + 1);
    ordered.push(writer);
    ordered.extend(later);
    let _ = note_predecessor_chain(ready, &ordered);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specfence::AccountHints;
    use crate::specfence::learner::MorphWeights;
    use crate::{MemoryLocation, hash_deterministic};
    use alloy_primitives::Address;

    #[test]
    fn same_from_chain_seeds_predecessor_not_star() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
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
        assert!(n >= 19, "same-from WAW must chain 19 edges, got {n}");
        assert!(ready.may_execute(0), "head of the WAW chain must run");
        assert!(!ready.may_execute(19));
        assert_eq!(
            ready.blocking_producer(19),
            Some(18),
            "WAW Avoid is wait-for the immediate predecessor, not a star on tx 0"
        );
        assert_eq!(ready.blocking_producer(1), Some(0));
        assert!(
            !stages.is_reserved(18),
            "WAW chain must not ProducerStage-reserve the whole spine"
        );
        let loc = hash_deterministic(MemoryLocation::Basic(addr));
        assert!(
            !learner.predicted_essential(loc, FAN_STAR_K),
            "same-from WAW must not plant PE (independent tax)"
        );
        assert_eq!(ready.predicted_producer(loc), Some(0));
    }

    #[test]
    fn empty_to_fanout_does_not_tax_unrelated_from_chain() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let payee = Address::repeat_byte(0x33);
        // 16 empty-calldata `to` (like 0x209c) — provisional chain, no Basic(to) PE.
        let hints = AccountHints::from_to_txs(payee, (0..16).collect());
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
        );
        assert!(n >= 15, "hot empty-to must probe-star, got {n}");
        assert!(ready.may_execute(0), "probe head of empty-to must run");
        assert!(!ready.may_execute(8), "empty-to later waits on the probe");
        assert_eq!(
            ready.blocking_producer(8),
            Some(0),
            "probe-star: later empty-to wait on the first, not a full begin-block chain"
        );
        let basic = hash_deterministic(MemoryLocation::Basic(payee));
        assert!(
            !learner.predicted_essential(basic, FAN_STAR_K),
            "must not plant Basic(to) PE on empty-calldata payee"
        );
        // Unrelated independent (not on the payee chain) stays runnable.
        assert!(
            ready.may_execute(17),
            "tx outside the WAW chain must stay independent"
        );
    }

    #[test]
    fn two_tx_empty_to_probes_first_only() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let payee = Address::repeat_byte(0x55);
        let hints = AccountHints::from_to_txs(payee, vec![3, 9]);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
        );
        assert_eq!(n, 1, "2-tx empty-to probes the first; later waits");
        assert!(ready.may_execute(3));
        assert_eq!(ready.blocking_producer(9), Some(3));
        assert!(
            ready.may_execute(11),
            "tx outside the probe must stay independent"
        );
        assert!(
            !stages.is_reserved(3),
            "empty-to probe is not a Stage reserve"
        );
    }

    #[test]
    fn short_calldata_to_chains_without_basic_pe() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let contract = Address::repeat_byte(0xed);
        let hints = AccountHints::from_call_to_txs(contract, vec![14, 16, 17]);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
        );
        assert_eq!(n, 2, "14→16→17 wait-for chain");
        assert!(ready.may_execute(14));
        assert_eq!(ready.blocking_producer(16), Some(14));
        assert_eq!(ready.blocking_producer(17), Some(16));
        let basic = hash_deterministic(MemoryLocation::Basic(contract));
        assert!(
            !learner.predicted_essential(basic, FAN_STAR_K),
            "short calldata WAW must not fake Basic(to) PE / storage clone"
        );
    }

    #[test]
    fn calldata_fanout_star_keeps_raw_cover() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(crate::specfence::learner::MorphWeights {
            fan_out: 0.70,
            mixed: 0.15,
            waw_spine: 0.10,
            quiet: 0.05,
        });
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let token = Address::repeat_byte(0xaa);
        let hints = AccountHints::from_call_to_txs(token, (0..16).collect());
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
        );
        assert!(n >= 15);
        assert_eq!(
            ready.blocking_producer(15),
            Some(0),
            "RAW fan-out stays a star on the first producer"
        );
        let basic = hash_deterministic(MemoryLocation::Basic(token));
        assert!(
            learner.predicted_essential(basic, FAN_STAR_K),
            "RAW star still plants Basic PE (not a storage clone)"
        );
    }

    #[test]
    fn empty_to_probe_then_hidden_write_set_chains_waw() {
        // 3356896 shape: envelope `to` is the contract; real WAW is Basic(hot wallet).
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let to = Address::repeat_byte(0x20);
        let from = Address::repeat_byte(0x31);
        let hints = AccountHints::from_to_txs(to, vec![31, 66, 67, 69]);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
        );
        assert_eq!(n, 3, "probe-star 31←66,67,69");
        assert!(ready.may_execute(31));
        assert_eq!(ready.blocking_producer(66), Some(31));
        assert_eq!(ready.blocking_producer(67), Some(31));
        assert_eq!(ready.blocking_producer(69), Some(31));
        assert!(
            !stages.is_reserved(31) && !stages.is_reserved(66),
            "WAW probe must not ProducerStage-reserve the spine"
        );
        let hidden = 0x32be_u64;
        let wave = WaveParkTable::new();
        admit_seed_on_write_set(&ready, &hints, &wave, 31, from, Some(to), &[hidden]);
        assert_eq!(ready.blocking_producer(66), Some(31));
        assert_eq!(
            ready.blocking_producer(67),
            Some(66),
            "write-set Detect upgrades probe-star to predecessor chain"
        );
        assert_eq!(ready.blocking_producer(69), Some(67));
        assert_eq!(ready.predicted_producer(hidden), Some(31));
        assert!(ready.may_execute(0), "independents stay runnable");
        let basic_to = hash_deterministic(MemoryLocation::Basic(to));
        assert!(
            !learner.predicted_essential(basic_to, FAN_STAR_K),
            "must not fake Basic(to) PE / storage clone"
        );
    }

    #[test]
    fn write_set_hidden_location_chains_same_to() {
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let to = Address::repeat_byte(0x20);
        let from = Address::repeat_byte(0x31);
        let hints = AccountHints::from_to_txs(to, vec![31, 66, 67, 69]);
        let hidden = 0x32be_u64;
        admit_seed_on_write_set(&ready, &hints, &wave, 31, from, Some(to), &[hidden]);
        assert_eq!(ready.blocking_producer(66), Some(31));
        assert_eq!(ready.blocking_producer(67), Some(66));
        assert_eq!(ready.blocking_producer(69), Some(67));
        assert_eq!(ready.predicted_producer(hidden), Some(31));
        assert!(ready.may_execute(0), "independents stay runnable");
    }

    #[test]
    fn write_set_lazy_only_releases_provisional_to_chain() {
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let to = Address::repeat_byte(0x20);
        let from = Address::repeat_byte(0x31);
        let hints = AccountHints::from_to_txs(to, (31..47).collect());
        let n = note_probe_star(&ready, hints.to_txs(&to));
        assert!(n >= 15);
        assert!(!ready.may_execute(32));
        assert_eq!(ready.blocking_producer(32), Some(31));
        // Writer 31 published only Basic(from) — no hidden WAW.
        let from_loc = hash_deterministic(MemoryLocation::Basic(from));
        admit_seed_on_write_set(&ready, &hints, &wave, 31, from, Some(to), &[from_loc]);
        assert!(
            ready.may_execute(32),
            "lazy payee chain must release after write-set shows no hidden WAW"
        );
        assert!(ready.may_execute(46));
    }

    #[test]
    fn bayes_admit_seeds_storage_without_hint_fan() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let storage_loc = 0xdef_u64;
        assert!(
            bayes.observe_conflict_location(storage_loc),
            "first conflict observation"
        );
        assert!(
            bayes.query_admit(storage_loc),
            "triple-conflict seed must clear admit τ"
        );
        let prior = InterBlockPrior::new();
        prior.end_block(
            MorphWeights::default(),
            vec![crate::specfence::learner::TopLocPrior {
                location: storage_loc,
                fanout_ema: 1.0,
                abort_rate: 0.0,
                chain_len_ema: 1.0,
                k_template: 6,
            }],
        );
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &AccountHints::default(),
            Address::ZERO,
        );
        assert_eq!(n, 0, "no hint accounts → no ReadyEdges");
        assert!(
            learner.predicted_essential(storage_loc, 6),
            "Bayes query_admit must plant storage PE on a cold hint set"
        );
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
        let hints = AccountHints::from_account_txs(addr, vec![0]);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
        );
        assert_eq!(n, 0, "single-tx sender is not a WAW chain");
        assert!(ready.may_execute(2));
    }

    #[test]
    fn hint_fan_does_not_drop_floor_onto_unrelated_accounts() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let storage_loc = 0xabc_u64;
        prior.end_block(
            MorphWeights::default(),
            vec![crate::specfence::learner::TopLocPrior {
                location: storage_loc,
                fanout_ema: 20.0,
                abort_rate: 0.05,
                chain_len_ema: 2.0,
                k_template: 6,
            }],
        );
        let star = Address::repeat_byte(0x33);
        let side = Address::repeat_byte(0x44);
        // RAW calldata star + a 3-tx same-from satellite. Floor=2-because-star is banned.
        let hints_star = AccountHints::from_call_to_txs(star, (0..16).collect());
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints_star,
            Address::ZERO,
        );
        assert!(n >= 15);
        let ready_side = ReadyEdgeTable::new();
        let stages_side = ProducerStageTable::new();
        let side_hints = AccountHints::from_account_txs(side, vec![1, 5, 8]);
        let n_side = admit_seed_begin_block(
            &ready_side,
            &stages_side,
            &learner,
            &bayes,
            &prior,
            &side_hints,
            Address::ZERO,
        );
        assert_eq!(
            n_side, 2,
            "satellite same-from gets its own 2-edge chain, not a star tax"
        );
        assert_eq!(ready_side.blocking_producer(8), Some(5));
        assert!(
            learner.predicted_essential(storage_loc, 6),
            "true-k: hint-fan plants storage RAW PE from InterPrior, not Basic→Storage clone"
        );
    }
}
