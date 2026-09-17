//! Admit seed — ReadyEdges + ProducerStage **before** satellite Execute.
//!
//! Detect uses real write locations (D1) + envelope probes as **cold-start
//! only** (D2). Avoid is a predecessor chain (wait-for dependency) for
//! **effective** WAW; RAW fan-out stays a star on the first producer.
//! Independents get no ReadyEdge / no PE.
//!
//! Soft=0. A0 vs A1 is B3 cost-aware EV (PC-5 Lean gate). No Soft wait arms.
//! No Basic→Storage PE clone.

use alloy_primitives::Address;
use hashbrown::HashSet;

use super::AccountHints;
use super::bayes::BayesMap;
use super::feeder::{seed_known_stars, top_is_known_star};
use super::learner::{InterBlockPrior, LiveLearner};
use super::policy::{CohortKind, CostPolicy};
use super::producer_stage::ProducerStageTable;
use super::ready_edge::ReadyEdgeTable;
use super::wave::WaveParkTable;
use crate::{MemoryLocation, MemoryLocationHash, TxIdx, hash_deterministic};

/// RAW fan-out star (v10): ≥16 calldata calls to the same `to`.
const RAW_FANOUT_FLOOR: usize = 16;
/// Short calldata WAW (storage trio, short call chains) — chain, no Basic(to) PE.
const CALL_WAW_FLOOR: usize = 2;
/// Fan-star access class (k≈6 → bucket 4–7). RAW stars only.
const FAN_STAR_K: u32 = 6;

/// Sentinel location for an envelope probe (D2) — rebind on write-set (D1).
#[inline]
fn envelope_loc(addr: Address) -> MemoryLocationHash {
    hash_deterministic(MemoryLocation::Basic(addr))
}

/// Chain `ordered[i]` behind `ordered[i-1]` on one location queue (PC-3).
///
/// Do **not** ProducerStage-reserve every predecessor. `next_reserved()` is a
/// global min; reserving a long WAW spine serializes the whole block.
fn note_predecessor_chain(
    ready: &ReadyEdgeTable,
    ordered: &[TxIdx],
    location: MemoryLocationHash,
    queued: &mut HashSet<TxIdx>,
) -> usize {
    let mut edges = 0;
    for pair in ordered.windows(2) {
        let (pred, succ) = (pair[0], pair[1]);
        if pred >= succ {
            continue;
        }
        if !queued.insert(succ) && ready.was_queued(succ) {
            // PC-3: already on another queue — rebind onto this location.
        }
        ready.note_consumer_on(succ, pred, Some(location));
        queued.insert(succ);
        edges += 1;
    }
    edges
}

/// Park later txs behind the first (probe). Write-set then chains or releases.
fn note_probe_star(
    ready: &ReadyEdgeTable,
    ordered: &[TxIdx],
    location: MemoryLocationHash,
    queued: &mut HashSet<TxIdx>,
) -> usize {
    if ordered.len() < 2 {
        return 0;
    }
    let probe = ordered[0];
    queued.insert(probe);
    let mut edges = 0;
    for &succ in &ordered[1..] {
        if probe >= succ {
            continue;
        }
        if queued.contains(&succ) {
            continue;
        }
        ready.note_consumer_on(succ, probe, Some(location));
        queued.insert(succ);
        edges += 1;
    }
    edges
}

/// begin_block admit: WAW predecessor chains + RAW fan-out stars.
///
/// Independents (no same-from WAW, no calldata/hot-`to` chain) stay
/// `may_execute` — zero ReadyEdge / PE tax.
///
/// Static floors (from≥3, empty-to≥8) are gone: B3 EV + effective-WAW
/// (PC-2/D3) decide A1. `contracts` is the pre-state code-hash set.
pub(crate) fn admit_seed_begin_block(
    ready: &ReadyEdgeTable,
    stages: &ProducerStageTable,
    learner: &LiveLearner,
    bayes: &BayesMap,
    prior: &InterBlockPrior,
    hints: &AccountHints,
    beneficiary: Address,
    policy: &CostPolicy,
    contracts: &HashSet<Address>,
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
    let mut queued: HashSet<TxIdx> = HashSet::new();

    // Calldata ≥16: RAW fan-out star (v10). First — PC-3 so later same-from
    // / empty-to cannot dual-tax the same txs.
    for addr in hints.call_to_accounts() {
        if addr == beneficiary {
            continue;
        }
        let txs = hints.call_to_txs(&addr);
        if txs.len() < RAW_FANOUT_FLOOR {
            continue;
        }
        let p_beta = bayes.account_wait_probability(&addr);
        if !policy.choose_a1(CohortKind::RawFan, addr, txs.len(), true, p_beta) {
            continue;
        }
        let producer = txs[0];
        stages.reserve(producer);
        let basic = hash_deterministic(MemoryLocation::Basic(addr));
        if star_edges {
            learner.seed_predicted_essential(basic, FAN_STAR_K);
        }
        ready.note_raw_producer(basic, producer);
        queued.insert(producer);
        for &t in &txs[1..] {
            if queued.contains(&t) {
                continue;
            }
            ready.note_consumer_on(t, producer, Some(basic));
            queued.insert(t);
            edges += 1;
        }
    }

    // Calldata 2..15: short contract WAW. Predecessor chain, no Basic(to) PE.
    // Keep PR15 wins on storage 14–17 when EV says A1.
    for addr in hints.call_to_accounts() {
        if addr == beneficiary {
            continue;
        }
        let txs = hints.call_to_txs(&addr);
        if txs.len() < CALL_WAW_FLOOR || txs.len() >= RAW_FANOUT_FLOOR {
            continue;
        }
        let p_beta = bayes.account_wait_probability(&addr);
        if !policy.choose_a1(CohortKind::CallWaw, addr, txs.len(), true, p_beta) {
            continue;
        }
        let loc = envelope_loc(addr);
        edges += note_predecessor_chain(ready, txs, loc, &mut queued);
    }

    // Empty-calldata `to`: D2 cold-start probe **only** when B3 says A1
    // (contract hot-payee). Pure lazy EOA spines stay A0 (PC-2).
    for addr in hints.to_accounts() {
        if addr == beneficiary {
            continue;
        }
        if hints.call_to_txs(&addr).len() >= CALL_WAW_FLOOR {
            continue;
        }
        let txs = hints.to_txs(&addr);
        if txs.len() < 2 {
            continue;
        }
        let is_contract = contracts.contains(&addr);
        let p_beta = bayes.account_wait_probability(&addr);
        if !policy.choose_a1(CohortKind::EmptyTo, addr, txs.len(), is_contract, p_beta) {
            continue;
        }
        let loc = envelope_loc(addr);
        edges += note_probe_star(ready, txs, loc, &mut queued);
    }

    // Same-from: PC-2 — basic_lazy / empty-calldata spines default A0.
    // Calldata senders may A1 if EV wins (not the 3356896 lazy miner spines).
    for addr in hints.from_accounts() {
        if addr == beneficiary {
            continue;
        }
        let txs = hints.from_txs(&addr);
        if txs.len() < 2 {
            continue;
        }
        if hints.cohort_all_empty(txs) {
            continue;
        }
        let p_beta = bayes.account_wait_probability(&addr);
        if !policy.choose_a1(CohortKind::SameFrom, addr, txs.len(), false, p_beta) {
            continue;
        }
        let basic = hash_deterministic(MemoryLocation::Basic(addr));
        ready.note_raw_producer(basic, txs[0]);
        edges += note_predecessor_chain(ready, txs, basic, &mut queued);
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

/// Intra-block Detect from a published write-set.
///
/// D1: every publish extends the location writer total order over **all**
/// earlier writers on that ℓ (fixes 4→31 on Basic(0x32be) — not only
/// `hints.to_txs(to)`).
/// D2: envelope probes upgrade to the real location or release if lazy-only.
/// D3: ReadyEdge A1 only on effective (non-lazy Data / Storage) locations.
pub(crate) fn admit_seed_on_write_set(
    ready: &ReadyEdgeTable,
    hints: &AccountHints,
    wave: &WaveParkTable,
    policy: Option<&CostPolicy>,
    writer: TxIdx,
    from: Address,
    to: Option<Address>,
    all_write_locs: &[MemoryLocationHash],
    effective_locs: &[MemoryLocationHash],
) {
    let from_loc = hash_deterministic(MemoryLocation::Basic(from));
    // D1: record every writer (lazy included) so 4→31 is visible in order.
    // PC-2 / D3: ReadyEdge A1 only on effective (non-lazy Data / Storage).
    for &loc in all_write_locs {
        ready.note_location_writer(loc, writer);
    }
    if effective_locs.iter().any(|&l| l == from_loc) {
        ready.note_raw_producer(from_loc, writer);
        // PC-2 / PC-5: 2-tx pairs and empty-calldata same-from stay A0 even
        // when nonce/balance is Data — refuse meta loses to one OCC abort.
        let from_txs = hints.from_txs(&from);
        if from_txs.len() >= 3 && !hints.cohort_all_empty(from_txs) {
            ready.note_immediate_pred(from_loc, writer);
        }
    }

    let Some(to) = to else {
        for &loc in effective_locs {
            if loc != from_loc {
                ready.note_raw_producer(loc, writer);
                ready.note_immediate_pred(loc, writer);
            }
        }
        return;
    };

    let hidden_eff: Vec<MemoryLocationHash> = effective_locs
        .iter()
        .copied()
        .filter(|&l| l != from_loc)
        .collect();

    // RAW fan-out star: keep all consumers behind the first producer.
    if hints.call_to_txs(&to).len() >= RAW_FANOUT_FLOOR {
        for loc in hidden_eff {
            ready.note_raw_producer(loc, writer);
            ready.note_location_writer(loc, writer);
            ready.note_immediate_pred(loc, writer);
        }
        if let Some(p) = policy {
            p.note_eff_waw(CohortKind::RawFan, to, true);
        }
        return;
    }

    if hidden_eff.is_empty() {
        // D2: lazy-only. Release + learn only when an A1 probe is live
        // (PC-5: independent empty transfers must not take deferred/B2 locks).
        let later = hints.to_txs(&to);
        let probed = later.iter().any(|&t| t > writer && ready.was_queued(t));
        if !probed {
            return;
        }
        if let Some(p) = policy {
            p.note_eff_waw(CohortKind::EmptyTo, to, false);
        }
        let from_later: Vec<TxIdx> = hints
            .from_txs(&from)
            .iter()
            .copied()
            .filter(|&t| t > writer)
            .collect();
        for &t in later {
            if t <= writer || from_later.contains(&t) {
                continue;
            }
            ready.release_consumer(t, wave);
        }
        return;
    }

    if let Some(p) = policy {
        let kind = if hints.call_to_txs(&to).len() >= CALL_WAW_FLOOR {
            CohortKind::CallWaw
        } else {
            CohortKind::EmptyTo
        };
        p.note_eff_waw(kind, to, true);
    }

    let later: Vec<TxIdx> = hints
        .to_txs(&to)
        .iter()
        .copied()
        .filter(|&t| t > writer)
        .collect();
    let mut queued = HashSet::new();
    for loc in hidden_eff {
        ready.note_raw_producer(loc, writer);
        ready.note_location_writer(loc, writer);
        // D1: 4→31 when 4 already published this ℓ (any envelope).
        ready.note_immediate_pred(loc, writer);
        if later.is_empty() {
            continue;
        }
        let pred = ready
            .writers_of(loc)
            .into_iter()
            .rev()
            .find(|&w| w <= writer)
            .unwrap_or(writer);
        let mut ordered = Vec::with_capacity(later.len() + 1);
        ordered.push(pred);
        ordered.extend(later.iter().copied());
        let _ = note_predecessor_chain(ready, &ordered, loc, &mut queued);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specfence::AccountHints;
    use crate::specfence::learner::MorphWeights;
    use crate::{MemoryLocation, hash_deterministic};
    use alloy_primitives::Address;

    fn policy_for(n: usize) -> CostPolicy {
        let p = CostPolicy::new();
        p.begin_block(n);
        p
    }

    fn seed(
        ready: &ReadyEdgeTable,
        stages: &ProducerStageTable,
        learner: &LiveLearner,
        bayes: &BayesMap,
        prior: &InterBlockPrior,
        hints: &AccountHints,
        contracts: &HashSet<Address>,
    ) -> usize {
        let n = hints.n_txs().max(32);
        let policy = policy_for(n);
        admit_seed_begin_block(
            ready,
            stages,
            learner,
            bayes,
            prior,
            hints,
            Address::ZERO,
            &policy,
            contracts,
        )
    }

    #[test]
    fn lazy_same_from_is_not_seeded() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let addr = Address::repeat_byte(0x11);
        let hints = AccountHints::from_account_txs(addr, (0..20).collect());
        let n = seed(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            &HashSet::new(),
        );
        assert_eq!(n, 0, "PC-2: basic_lazy same-from must default A0, got {n}");
        assert!(ready.may_execute(19), "lazy spine stays independent");
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
        let mut contracts = HashSet::new();
        contracts.insert(payee);
        let n = seed(
            &ready, &stages, &learner, &bayes, &prior, &hints, &contracts,
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
    fn two_tx_empty_to_is_not_seeded() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let payee = Address::repeat_byte(0x55);
        let hints = AccountHints::from_to_txs(payee, vec![3, 9]);
        let n = seed(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            &HashSet::new(),
        );
        assert_eq!(n, 0, "2-tx lazy payee must not tax independents");
        assert!(ready.may_execute(9));
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
        let n = seed(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            &HashSet::new(),
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
        let n = seed(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            &HashSet::new(),
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
        let hints = AccountHints::from_to_txs(to, vec![31, 66, 67, 69, 70, 93, 96, 103, 115]);
        let mut contracts = HashSet::new();
        contracts.insert(to);
        let n = seed(
            &ready, &stages, &learner, &bayes, &prior, &hints, &contracts,
        );
        assert_eq!(n, 8, "probe-star: later empty-to wait on tx 31");
        assert!(ready.may_execute(31));
        assert_eq!(ready.blocking_producer(66), Some(31));
        assert_eq!(ready.blocking_producer(67), Some(31));
        assert_eq!(ready.blocking_producer(115), Some(31));
        assert!(
            !stages.is_reserved(31) && !stages.is_reserved(66),
            "WAW probe must not ProducerStage-reserve the spine"
        );
        let hidden = 0x32be_u64;
        let wave = WaveParkTable::new();
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            None,
            31,
            from,
            Some(to),
            &[hidden],
            &[hidden],
        );
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
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            None,
            31,
            from,
            Some(to),
            &[hidden],
            &[hidden],
        );
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
        let loc = envelope_loc(to);
        let mut queued = HashSet::new();
        let n = note_probe_star(&ready, hints.to_txs(&to), loc, &mut queued);
        assert!(n >= 15);
        assert!(!ready.may_execute(32));
        assert_eq!(ready.blocking_producer(32), Some(31));
        // Writer 31 published only Basic(from) — no hidden WAW.
        let from_loc = hash_deterministic(MemoryLocation::Basic(from));
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            None,
            31,
            from,
            Some(to),
            &[from_loc],
            &[],
        );
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
        let n = seed(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &AccountHints::default(),
            &HashSet::new(),
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
        let n = seed(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            &HashSet::new(),
        );
        assert_eq!(n, 0, "single-tx sender is not a WAW chain");
        assert!(ready.may_execute(2));
    }

    #[test]
    fn two_tx_same_from_is_not_seeded() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let addr = Address::repeat_byte(0x56);
        let hints = AccountHints::from_account_txs(addr, vec![56, 57]);
        let n = seed(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            &HashSet::new(),
        );
        assert_eq!(n, 0, "2-tx same-from stays OCC-cost (no refuse tax)");
        assert!(ready.may_execute(57));
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
        let n = seed(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints_star,
            &HashSet::new(),
        );
        assert!(n >= 15);
        let ready_side = ReadyEdgeTable::new();
        let stages_side = ProducerStageTable::new();
        let side_hints = AccountHints::from_account_txs(side, vec![1, 5, 8]);
        let n_side = seed(
            &ready_side,
            &stages_side,
            &learner,
            &bayes,
            &prior,
            &side_hints,
            &HashSet::new(),
        );
        assert_eq!(
            n_side, 0,
            "PC-2: lazy same-from satellite stays A0 (not a star tax)"
        );
        assert!(ready_side.may_execute(8));
        assert!(
            learner.predicted_essential(storage_loc, 6),
            "true-k: hint-fan plants storage RAW PE from InterPrior, not Basic→Storage clone"
        );
    }

    #[test]
    fn write_set_d1_extends_all_earlier_writers() {
        // 3356896: tx4 writes Basic(0x32be); tx31 later writes the same ℓ via 0x209c.
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let to = Address::repeat_byte(0x20);
        let hot = Address::repeat_byte(0x32);
        let other = Address::repeat_byte(0x99);
        let hints = AccountHints::from_to_txs(to, vec![31, 66, 67]);
        let loc = 0x32be_u64;
        // tx4 (from=hot, to=other) publishes Basic(hot) — not in hints.to_txs(to).
        admit_seed_on_write_set(
            &ready,
            &AccountHints::from_to_txs(other, vec![4]),
            &wave,
            None,
            4,
            hot,
            Some(other),
            &[loc],
            &[loc],
        );
        assert_eq!(ready.writers_of(loc), vec![4]);
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            None,
            31,
            Address::repeat_byte(0x31),
            Some(to),
            &[loc],
            &[loc],
        );
        assert_eq!(
            ready.writers_of(loc),
            vec![4, 31],
            "D1 writer order must list 4 then 31"
        );
        assert_eq!(
            ready.blocking_producer(31),
            Some(4),
            "4→31 must be a ReadyEdge after write-set, not scheduler luck"
        );
        assert_eq!(ready.blocking_producer(66), Some(31));
    }

    #[test]
    fn write_set_d1_lazy_then_effective_still_chains_4_to_31() {
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let to = Address::repeat_byte(0x20);
        let hot = Address::repeat_byte(0x32);
        let other = Address::repeat_byte(0x99);
        let hints = AccountHints::from_to_txs(to, vec![31, 66, 67]);
        let loc = 0x32be_u64;
        // tx4 publishes lazy-only on the hot account — record, do not A1-fence.
        admit_seed_on_write_set(
            &ready,
            &AccountHints::from_to_txs(other, vec![4]),
            &wave,
            None,
            4,
            hot,
            Some(other),
            &[loc],
            &[],
        );
        assert_eq!(ready.writers_of(loc), vec![4]);
        assert!(
            ready.may_execute(31),
            "lazy-only publish must not refuse the next writer before Detect"
        );
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            None,
            31,
            Address::repeat_byte(0x31),
            Some(to),
            &[loc],
            &[loc],
        );
        assert_eq!(ready.writers_of(loc), vec![4, 31]);
        assert_eq!(
            ready.blocking_producer(31),
            Some(4),
            "effective publish must extend order over the earlier lazy writer"
        );
        assert_eq!(ready.blocking_producer(66), Some(31));
    }

    #[test]
    fn write_set_lazy_from_does_not_a1_same_from_spine() {
        // PC-2: LazySender on Basic(from) is not effective WAW — no ReadyEdge tax.
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let from = Address::repeat_byte(0x2a);
        let to = Address::repeat_byte(0x11);
        let from_loc = hash_deterministic(MemoryLocation::Basic(from));
        let hints = AccountHints::from_from_and_to(from, to, vec![5, 6, 7, 8]);
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            None,
            5,
            from,
            Some(to),
            &[from_loc],
            &[],
        );
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            None,
            6,
            from,
            Some(to),
            &[from_loc],
            &[],
        );
        assert_eq!(ready.writers_of(from_loc), vec![5, 6]);
        assert!(
            ready.may_execute(6) && ready.may_execute(7) && ready.may_execute(8),
            "lazy same-from must stay A0 after write-set (no mid-block refuse tax)"
        );
    }

    #[test]
    fn write_set_two_tx_effective_from_stays_a0() {
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let from = Address::repeat_byte(0x56);
        let to = Address::repeat_byte(0x11);
        let from_loc = hash_deterministic(MemoryLocation::Basic(from));
        let hints = AccountHints::from_from_and_to(from, to, vec![56, 57]);
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            None,
            56,
            from,
            Some(to),
            &[from_loc],
            &[from_loc],
        );
        assert!(
            ready.may_execute(57),
            "2-tx same-from Data nonce must stay OCC-cost"
        );
    }

    #[test]
    fn pc3_empty_to_does_not_dual_queue_same_from() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        // 0x9535∩0x9e0b shape: same txs in same-from and empty-to.
        let from = Address::repeat_byte(0x95);
        let to = Address::repeat_byte(0x9e);
        let txs: Vec<TxIdx> = (75..87).collect();
        let hints = AccountHints::from_from_and_to(from, to, txs);
        let mut contracts = HashSet::new();
        contracts.insert(to);
        let n = seed(
            &ready, &stages, &learner, &bayes, &prior, &hints, &contracts,
        );
        // Same-from is lazy → not seeded. Empty-to EOA would be A0; we marked
        // `to` as contract so the probe may fire — but each tx is on one queue.
        let blocked = ready.blocked_consumers();
        let unique: HashSet<_> = blocked.iter().copied().collect();
        assert_eq!(
            unique.len(),
            blocked.len(),
            "PC-3: no dual same-from + empty-to tax"
        );
        let _ = n;
    }
}
