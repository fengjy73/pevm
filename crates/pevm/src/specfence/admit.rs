//! Admit seed — ReadyEdges + ProducerStage **before** satellite Execute.
//!
//! Detect uses real write locations (D1) + envelope probes as **cold-start
//! only** (D2). Avoid is a predecessor chain (wait-for dependency) for
//! **effective** WAW; RAW fan-out stays a star on the first producer.
//! Independents get no ReadyEdge / no PE.
//!
//! Soft=0. A0 vs A1 is ns-EV (K is a cap). No Soft wait arms. No second OCC engine.
//! No Basic→Storage PE clone.

use alloy_primitives::Address;
use hashbrown::{HashMap, HashSet};

use super::AccountHints;
use super::bayes::BayesMap;
use super::feeder::{seed_known_stars, top_is_known_star};
use super::learner::{InterBlockPrior, LiveLearner};
use super::metrics::MetricsInner;
use super::policy::{CohortKind, CostPolicy, FAT_N, ORDER_WINDOW_K, THIN_ORDERED_K};
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
/// A0-majority: only the first successor (short edge). Later writers stay A0.
fn note_predecessor_chain(
    ready: &ReadyEdgeTable,
    ordered: &[TxIdx],
    location: MemoryLocationHash,
    queued: &mut HashSet<TxIdx>,
    short_edge: bool,
) -> usize {
    let chain = if short_edge && ordered.len() >= 2 {
        &ordered[..2]
    } else {
        ordered
    };
    let mut edges = 0;
    for pair in chain.windows(2) {
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
/// A0-majority: first successor only — do not refuse the rest of the payee spine.
fn note_probe_star(
    ready: &ReadyEdgeTable,
    ordered: &[TxIdx],
    location: MemoryLocationHash,
    queued: &mut HashSet<TxIdx>,
    short_edge: bool,
) -> usize {
    if ordered.len() < 2 {
        return 0;
    }
    let probe = ordered[0];
    queued.insert(probe);
    let succs: &[TxIdx] = if short_edge {
        &ordered[1..2]
    } else {
        &ordered[1..]
    };
    let mut edges = 0;
    for &succ in succs {
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
    metrics: Option<&MetricsInner>,
) -> usize {
    // L1/C5: thin never plants an envelope A1=3 star. CallWaw spines get a
    // consecutive short-edge chain (14→16→17, 31→66→…). Empty-to stays A0.
    if policy.is_optimistic_majority_block() {
        let mut edges = 0;
        if policy.should_seed_thin_ordered() {
            edges += admit_seed_promoted_short_edges(ready, hints, policy, metrics);
        }
        edges += admit_seed_hint_short_edges(ready, hints, policy, metrics);
        let _ = (stages, learner, bayes, prior, beneficiary, contracts);
        return edges;
    }
    // C2: fat reuse still plants the real Basic/storage spine (not lazy).
    let mut edges = admit_seed_promoted_short_edges(ready, hints, policy, metrics);
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

    struct Cand {
        kind: CohortKind,
        addr: Address,
        is_contract: bool,
        score: i64,
        txs: Vec<TxIdx>,
    }
    let mut cands: Vec<Cand> = Vec::new();

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
        if !policy.choose_ordered(CohortKind::RawFan, addr, txs.len(), true, p_beta) {
            if let Some(m) = metrics {
                m.record_edge_optimistic_read();
            }
            continue;
        }
        cands.push(Cand {
            kind: CohortKind::RawFan,
            addr,
            is_contract: true,
            score: CostPolicy::ordered_score(CohortKind::RawFan, txs.len(), true),
            txs: txs.to_vec(),
        });
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
        if !policy.choose_ordered(CohortKind::CallWaw, addr, txs.len(), true, p_beta) {
            if let Some(m) = metrics {
                m.record_edge_optimistic_read();
            }
            continue;
        }
        cands.push(Cand {
            kind: CohortKind::CallWaw,
            addr,
            is_contract: true,
            score: CostPolicy::ordered_score(CohortKind::CallWaw, txs.len(), true),
            txs: txs.to_vec(),
        });
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
        if !policy.choose_ordered(CohortKind::EmptyTo, addr, txs.len(), is_contract, p_beta) {
            if let Some(m) = metrics {
                m.record_edge_optimistic_read();
            }
            continue;
        }
        cands.push(Cand {
            kind: CohortKind::EmptyTo,
            addr,
            is_contract,
            score: CostPolicy::ordered_score(CohortKind::EmptyTo, txs.len(), is_contract),
            txs: txs.to_vec(),
        });
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
        if !policy.choose_ordered(CohortKind::SameFrom, addr, txs.len(), false, p_beta) {
            if let Some(m) = metrics {
                m.record_edge_optimistic_read();
            }
            continue;
        }
        cands.push(Cand {
            kind: CohortKind::SameFrom,
            addr,
            is_contract: false,
            score: CostPolicy::ordered_score(CohortKind::SameFrom, txs.len(), false),
            txs: txs.to_vec(),
        });
    }

    cands.sort_by(|a, b| b.score.cmp(&a.score));
    let cap = policy.thin_ordered_k();
    if cands.len() > cap {
        for extra in cands.drain(cap..) {
            policy.note_k_cap_demote();
            if let Some(m) = metrics {
                m.record_edge_optimistic_read();
            }
            let _ = extra;
        }
    }

    let mut queued: HashSet<TxIdx> = HashSet::new();
    for c in cands {
        let loc = match c.kind {
            CohortKind::RawFan | CohortKind::SameFrom | CohortKind::EmptyTo => {
                hash_deterministic(MemoryLocation::Basic(c.addr))
            }
            CohortKind::CallWaw => envelope_loc(c.addr),
        };
        // C1: never plant a lazy / near_independent object.
        if policy.loc_forbids_ordered(loc) {
            continue;
        }
        if let Some(m) = metrics {
            m.record_edge_ordered_admit();
        }
        match c.kind {
            CohortKind::RawFan => {
                let producer = c.txs[0];
                stages.reserve(producer);
                if star_edges {
                    learner.seed_predicted_essential(loc, FAN_STAR_K);
                }
                ready.note_raw_producer(loc, producer);
                queued.insert(producer);
                // Probe the first successor only. A full-envelope star
                // (ERC-20 independent) serializes the block and livelocks
                // dual-path skip — OrderedAdmit is an edge, not a fan.
                if let Some(&succ) = c.txs.get(1)
                    && !queued.contains(&succ)
                {
                    ready.note_consumer_on(succ, producer, Some(loc));
                    queued.insert(succ);
                    edges += 1;
                }
            }
            CohortKind::CallWaw => {
                // C2: fat CallWaw is light-cover (short hop), not a full chain.
                let short = policy.block_n() >= FAT_N;
                edges += note_predecessor_chain(ready, &c.txs, loc, &mut queued, short);
            }
            CohortKind::EmptyTo => {
                // choose() already A0 on fat; keep thin-path dead.
                edges += note_probe_star(ready, &c.txs, loc, &mut queued, false);
            }
            CohortKind::SameFrom => {
                ready.note_raw_producer(loc, c.txs[0]);
                // Short hop only — never a full nonce spine (ERC-20 clusters).
                edges += note_predecessor_chain(ready, &c.txs, loc, &mut queued, true);
            }
        }
        let _ = c.is_contract;
    }

    soft_cap_begin_blocked(ready, policy);
    edges
}

/// P2: fat n≥512 soft-cap begin holes. Keep earliest (real-spine prefix).
fn soft_cap_begin_blocked(ready: &ReadyEdgeTable, policy: &CostPolicy) {
    let cap = policy.fat_begin_hole_cap();
    if cap == usize::MAX {
        return;
    }
    let mut blocked = ready.blocked_consumers();
    if blocked.len() <= cap {
        return;
    }
    blocked.sort_unstable();
    for &tx in &blocked[cap..] {
        ready.ungate(tx);
    }
}

/// C1/C5: thin begin — storage-trio CallWaw only (`3..=4`).
/// Wide ERC-20 / RAW fans stay A0 (same `to`, different slots). Empty-to A0.
/// L5: each short CallWaw ℓ picks independently via `select_hint_arm`.
/// DeferPlant / Opt plant nothing; Full / Win_w plant that arm's pairs.
fn admit_seed_hint_short_edges(
    ready: &ReadyEdgeTable,
    hints: &AccountHints,
    policy: &CostPolicy,
    metrics: Option<&MetricsInner>,
) -> usize {
    let mut addrs: Vec<(Address, usize, TxIdx)> = hints
        .call_to_accounts()
        .map(|a| {
            let txs = hints.call_to_txs(&a);
            (a, txs.len(), txs.first().copied().unwrap_or(TxIdx::MAX))
        })
        .filter(|&(_, n, _)| (3..=4).contains(&n))
        .collect();
    addrs.sort_unstable_by(|a, b| a.2.cmp(&b.2).then_with(|| b.1.cmp(&a.1)));
    let mut edges = 0;
    for (addr, n, _) in addrs {
        let loc = envelope_loc(addr);
        if policy.is_promoted(loc) {
            continue;
        }
        let n_pairs = n.saturating_sub(1);
        let txs = hints.call_to_txs(&addr);
        let mut pairs: Vec<(TxIdx, TxIdx)> = Vec::new();
        for pair in txs.windows(2) {
            let (pred, succ) = (pair[0], pair[1]);
            if pred < succ {
                pairs.push((pred, succ));
                policy.note_short_pair(loc, pred, succ);
            }
        }
        let arm = policy.select_hint_arm(loc, n_pairs.max(pairs.len()));
        policy.remember_arm(loc, arm);
        if !arm.is_ordered() {
            continue;
        }
        if !policy.should_gate_short_after_write(loc, false, n.saturating_sub(1)) {
            continue;
        }
        let plant = policy.plant_pairs(arm, &pairs);
        let mut planted = 0usize;
        for (pred, succ) in plant {
            ready.note_consumer_on(succ, pred, Some(loc));
            policy.note_short_edge_admit();
            if let Some(m) = metrics {
                m.record_edge_ordered_admit();
            }
            planted += 1;
        }
        if planted > 0 {
            policy.promote_short_edge(loc, 0);
            policy.note_hops_decision(loc, n_pairs.max(pairs.len()));
            edges += planted;
            // O2: cold begin plants one CallWaw trio (14→16→17). The second
            // (15→19→20) waits for a measured abort — two extra holes are
            // prepaid width with no Detect win on 3356896.
            if policy.is_optimistic_majority_block() {
                break;
            }
        }
    }
    edges
}

/// L4/C5: reuse / measured prior → ReadyEdges on promoted ℓ (not a cohort).
/// Thin: ≤`THIN_ORDERED_K` locations, longest first. O1: storage 14→16→17 stays
/// fully ordered. Long Basic WAW is A0 at begin when leftover-aware EV
/// loses (prefix-window + tail abort lost PRIMARY); O3 strengthens one hop.
fn admit_seed_promoted_short_edges(
    ready: &ReadyEdgeTable,
    hints: &AccountHints,
    policy: &CostPolicy,
    metrics: Option<&MetricsInner>,
) -> usize {
    let mut by_loc: HashMap<MemoryLocationHash, Vec<(TxIdx, TxIdx)>> = HashMap::new();
    for (loc, pred, succ) in policy.promoted_short_pairs() {
        if pred >= succ || !policy.is_promoted(loc) {
            continue;
        }
        by_loc.entry(loc).or_default().push((pred, succ));
    }
    by_loc.retain(|_, pairs| {
        let mut txs: Vec<TxIdx> = pairs.iter().flat_map(|(a, b)| [*a, *b]).collect();
        txs.sort_unstable();
        txs.dedup();
        !is_wide_envelope_writer_set(hints, &txs)
    });
    let mut locs: Vec<(MemoryLocationHash, Vec<(TxIdx, TxIdx)>)> = by_loc.into_iter().collect();
    locs.sort_unstable_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    // Drop zero-hop (demoted / leftover-lose) locs *before* the K-cap so a
    // 16-writer Basic spine cannot evict storage 14→16→17.
    locs.retain(|(loc, pairs)| policy.hops_to_plant(*loc, pairs.len()) > 0);
    if policy.is_optimistic_majority_block() && locs.len() > THIN_ORDERED_K {
        locs.truncate(THIN_ORDERED_K);
    }
    let mut edges = 0;
    for (loc, mut pairs) in locs {
        pairs.sort_unstable_by_key(|(pred, _)| *pred);
        let n_pairs = pairs.len();
        if policy.hops_to_plant(loc, n_pairs) == 0 {
            continue;
        }
        policy.note_hops_decision(loc, n_pairs);
        let strategy = policy.loc_strategy(loc, n_pairs);
        let plant = policy.plant_pairs(strategy, &pairs);
        for (pred, succ) in plant {
            ready.note_consumer_on(succ, pred, Some(loc));
            policy.note_short_edge_admit();
            if let Some(m) = metrics {
                m.record_edge_ordered_admit();
            }
            edges += 1;
        }
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
    // D1 tip only. `note_consumer` + ProducerStage reserve here is a
    // mid-execute ReadyEdge plant — banned (ERC-20 / iter11 livelock).
    // The aborted consumer OCC-reexecs; persist/queue is next-begin.
    let _ = (stages, consumer);
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
    // C1/C2: thin A0 publish is OCC-identical unless this is a short
    // storage-shaped CallWaw (14→16→17). Long Basic WAW (0x32be) is A0 —
    // write-set plant rebuilt the prepaid wall and lost PRIMARY.
    if policy.is_some_and(|p| p.is_optimistic_majority_block()) && !ready.was_queued(writer) {
        let storage_like = to.is_some_and(|t| {
            let n = hints.call_to_txs(&t).len();
            (3..=4).contains(&n)
        });
        if storage_like {
            seed_short_edges_after_publish(
                ready,
                hints,
                policy,
                writer,
                from,
                to,
                from_loc,
                all_write_locs,
                effective_locs,
            );
        }
        return;
    }
    // D1: record every writer (lazy included) so 4→31 is visible in order.
    // PC-2 / D3: ReadyEdge A1 only on effective (non-lazy Data / Storage).
    for &loc in all_write_locs {
        ready.note_location_writer(loc, writer);
        if let Some(p) = policy {
            let lazy = !effective_locs.iter().any(|&l| l == loc);
            p.note_loc_write(loc, lazy);
        }
    }
    if effective_locs.iter().any(|&l| l == from_loc)
        && !policy.is_some_and(|p| p.loc_forbids_ordered(from_loc))
    {
        ready.note_raw_producer(from_loc, writer);
        // PC-2 / PC-5: 2-tx pairs and empty-calldata same-from stay A0 even
        // when nonce/balance is Data — refuse meta loses to one OCC abort.
        let from_txs = hints.from_txs(&from);
        // Wide nonce chains stay OptimisticRead (ERC-20 clusters).
        if (3..8).contains(&from_txs.len()) && !hints.cohort_all_empty(from_txs) {
            ready.note_immediate_pred(from_loc, writer);
        }
    }

    let Some(to) = to else {
        for &loc in effective_locs {
            if loc != from_loc && !policy.is_some_and(|p| p.loc_forbids_ordered(loc)) {
                ready.note_raw_producer(loc, writer);
                if policy.is_none_or(|p| p.hops_to_plant(loc, p.pairs_of(loc).len().max(1)) > 0) {
                    ready.note_immediate_pred(loc, writer);
                }
            }
        }
        return;
    };

    let hidden_eff: Vec<MemoryLocationHash> = effective_locs
        .iter()
        .copied()
        .filter(|&l| l != from_loc)
        .collect();

    // Wide RAW fan (ERC-20): D1 only. Mid-execute ReadyEdge on a
    // thousands-wide same-`to` livelocks dual-path skip (P1 ban).
    if hints.call_to_txs(&to).len() >= RAW_FANOUT_FLOOR {
        for loc in hidden_eff {
            ready.note_raw_producer(loc, writer);
            ready.note_location_writer(loc, writer);
        }
        if let Some(p) = policy {
            p.note_eff_waw(CohortKind::RawFan, to, true);
        }
        return;
    }

    if hidden_eff.is_empty() {
        // D2: lazy-only. Release + learn only when an A1 probe is live
        // (PC-5: independent empty transfers must not take deferred/B2 locks).
        // P0-B: write-set proves no effective waiters — same as cost-EV demote
        // (`should_release_probe_star`) when commute absorbed the star.
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
    // Production: only upgrade an existing A1 probe (begin-block EV).
    // Tests pass `policy=None` and still expect write-set to seed the chain.
    let chain_later =
        !later.is_empty() && (later.iter().any(|&t| ready.was_queued(t)) || policy.is_none());
    let mut queued = HashSet::new();
    for &loc in &hidden_eff {
        ready.note_raw_producer(loc, writer);
        ready.note_location_writer(loc, writer);
        let n_writers = ready.writers_of(loc).len();
        let n_pairs = n_writers
            .saturating_sub(1)
            .max(policy.map(|p| p.pairs_of(loc).len()).unwrap_or(0));
        if policy.is_some_and(|p| p.loc_forbids_n(loc, n_pairs)) {
            continue;
        }
        // C2: real spine only. hops=0 (Opt/Defer / lazy) leaves OCC.
        let plant = policy.is_none_or(|p| p.hops_to_plant(loc, n_pairs.max(1)) > 0);
        if plant {
            // D1: 4→31 when 4 already published this ℓ (any envelope).
            ready.note_immediate_pred(loc, writer);
        }
        if !chain_later || !plant {
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
        // C2: production plants a light hop, not the full envelope (K8
        // 47-hole stars). Tests pass policy=None and still expect D1 chain.
        let short = policy.is_some();
        let _ = note_predecessor_chain(ready, &ordered, loc, &mut queued, short);
    }
    // P0-B: cost-EV demote / commute-absorbed star — release later envelope
    // waiters that D1 did **not** keep as WAW. Never release when the keep-set
    // is empty (that dropped RawFan waiters). Do not re-A1 lazy same-from.
    if policy.is_some_and(|p| p.should_release_probe_star()) && !queued.is_empty() {
        for &t in &later {
            if t > writer && !queued.contains(&t) {
                ready.release_consumer(t, wave);
            }
        }
    }
    if let Some(p) = policy {
        soft_cap_begin_blocked(ready, p);
    }
}

/// C1/C2: after an effective publish, record D1 and raise at most one
/// successor ReadyEdge per ℓ (location total order, not a lazy same-from spine).
fn seed_short_edges_after_publish(
    ready: &ReadyEdgeTable,
    hints: &AccountHints,
    policy: Option<&CostPolicy>,
    writer: TxIdx,
    from: Address,
    to: Option<Address>,
    from_loc: MemoryLocationHash,
    all_write_locs: &[MemoryLocationHash],
    effective_locs: &[MemoryLocationHash],
) {
    if effective_locs.is_empty() {
        return;
    }
    let envelope_later = envelope_successors(hints, to, writer);
    let from_later = from_successors(hints, from, writer, from_loc, effective_locs);
    // Envelope `to` is a same-ℓ proxy only for short CallWaw (storage trio).
    // Wide ERC-20 / RAW fans write different slots — do not chain them.
    let envelope_is_loc = to.is_some_and(|t| {
        let call_n = hints.call_to_txs(&t).len();
        let pay_n = hints.to_txs(&t).len();
        (call_n >= CALL_WAW_FLOOR && call_n < 8)
            || (call_n < CALL_WAW_FLOOR && pay_n >= 2 && pay_n < 8)
    });
    // D1: record lazy + Data so 4→31 is visible even when tx4 is lazy-only.
    for &loc in all_write_locs {
        ready.note_location_writer(loc, writer);
        if let Some(p) = policy {
            let lazy = !effective_locs.iter().any(|&l| l == loc);
            p.note_loc_write(loc, lazy);
        }
    }
    for &loc in all_write_locs {
        if !effective_locs.iter().any(|&l| l == loc) {
            continue;
        }
        let has_earlier = ready.writers_of(loc).iter().any(|&w| w < writer);
        let later: &[TxIdx] = if loc == from_loc {
            &from_later
        } else if envelope_is_loc {
            &envelope_later
        } else {
            &[]
        };
        let hint_n = later.len();
        let gate = policy
            .map(|p| p.should_gate_short_after_write(loc, has_earlier, hint_n))
            .unwrap_or(has_earlier || hint_n >= 2);
        if !gate {
            continue;
        }
        // O1/O2: do not plant a long-spine hop the leftover EV already lost.
        if let Some(p) = policy {
            let n_pairs = p.pairs_of(loc).len().max(1);
            if p.hops_to_plant(loc, n_pairs) == 0 && n_pairs > ORDER_WINDOW_K {
                continue;
            }
        }
        if let Some(p) = policy {
            p.promote_short_edge(loc, 0);
            if let Some(&succ) = later.first() {
                p.note_short_pair(loc, writer, succ);
            }
            if has_earlier {
                if let Some(pred) = ready
                    .writers_of(loc)
                    .into_iter()
                    .rev()
                    .find(|&w| w < writer)
                {
                    p.note_short_pair(loc, pred, writer);
                }
            }
        }
        if has_earlier {
            if let Some(pred) = ready
                .writers_of(loc)
                .into_iter()
                .rev()
                .find(|&w| w < writer)
            {
                let _ = ready.note_consumer_on_if_idle(writer, pred, Some(loc));
            }
        }
        if let Some(&succ) = later.first() {
            let _ = ready.note_consumer_on_if_idle(succ, writer, Some(loc));
        }
    }
}

fn envelope_successors(hints: &AccountHints, to: Option<Address>, writer: TxIdx) -> Vec<TxIdx> {
    let Some(to) = to else {
        return Vec::new();
    };
    let call = hints.call_to_txs(&to);
    let payee = hints.to_txs(&to);
    let src = if call.len() >= CALL_WAW_FLOOR {
        call
    } else {
        payee
    };
    src.iter().copied().filter(|&t| t > writer).collect()
}

fn from_successors(
    hints: &AccountHints,
    from: Address,
    writer: TxIdx,
    from_loc: MemoryLocationHash,
    effective_locs: &[MemoryLocationHash],
) -> Vec<TxIdx> {
    if !effective_locs.iter().any(|&l| l == from_loc) {
        return Vec::new();
    }
    let from_txs = hints.from_txs(&from);
    if from_txs.len() < 3 || hints.cohort_all_empty(from_txs) {
        return Vec::new();
    }
    from_txs.iter().copied().filter(|&t| t > writer).collect()
}

/// Later writers of `location` that hints can name without a wide CallWaw star.
///
/// Account-location consecutive only:
/// - `ℓ = Basic(from)` → later same-from
/// - `ℓ = Basic(to)` → later same `to`
/// - hidden ℓ + short CallWaw (3..=7) → later calldata of that `to` (14→16→17)
/// Hidden empty-to (0x209c → Basic(0x32be)) is completed from D1 at end-block,
/// not by cloning the envelope onto every abort ℓ.
/// Wide CallWaw / RAW fans stay empty — different slots must not serialize.
fn location_successors(
    hints: &AccountHints,
    location: MemoryLocationHash,
    writer: TxIdx,
    from: Address,
    to: Option<Address>,
) -> Vec<TxIdx> {
    let from_loc = hash_deterministic(MemoryLocation::Basic(from));
    if location == from_loc {
        return hints
            .from_txs(&from)
            .iter()
            .copied()
            .filter(|&t| t > writer)
            .collect();
    }
    let Some(to) = to else {
        return Vec::new();
    };
    let to_loc = hash_deterministic(MemoryLocation::Basic(to));
    if location == to_loc {
        return hints
            .to_txs(&to)
            .iter()
            .copied()
            .filter(|&t| t > writer)
            .collect();
    }
    let call_n = hints.call_to_txs(&to).len();
    if call_n >= RAW_FANOUT_FLOOR || call_n >= 8 {
        return Vec::new();
    }
    if call_n >= CALL_WAW_FLOOR {
        return hints
            .call_to_txs(&to)
            .iter()
            .copied()
            .filter(|&t| t > writer)
            .collect();
    }
    // Hidden ℓ + empty-to: do **not** clone the envelope tail onto this ℓ.
    // 0x209c writers of Basic(0x32be) are completed from D1 at end-block.
    Vec::new()
}

/// True when `writers` is a wide empty-to / CallWaw envelope (not a hidden Basic).
pub(crate) fn is_wide_envelope_writer_set(hints: &AccountHints, writers: &[TxIdx]) -> bool {
    if writers.len() < 8 {
        return false;
    }
    hints.to_accounts().any(|a| {
        let t = hints.to_txs(&a);
        t.len() >= 8 && writers.iter().all(|w| t.contains(w))
    }) || hints.call_to_accounts().any(|a| {
        let t = hints.call_to_txs(&a);
        t.len() >= RAW_FANOUT_FLOOR && writers.iter().all(|w| t.contains(w))
    })
}

/// L2: first EffectiveWAW abort → persist consecutive pairs on this ℓ.
///
/// Do **not** insert ReadyEdges on in-flight OptimisticRead successors
/// (done-stamp race). CC-L1/L2 queue idle pairs; `flush_pending_idle_edges`
/// plants them only via `note_consumer_on_if_idle`.
pub(crate) fn persist_short_chain_after_abort(
    hints: &AccountHints,
    policy: &CostPolicy,
    consumer: TxIdx,
    producer: TxIdx,
    location: MemoryLocationHash,
) {
    if producer < consumer {
        policy.note_short_pair(location, producer, consumer);
        // C1: hops=0 (Opt/Defer) must not queue idle. Covering ordered after
        // systematic reexec has hops>0 and plants via the same mouth.
        let n_pairs = policy.pairs_of(location).len();
        if policy.hops_to_plant(location, n_pairs) > 0 {
            policy.queue_idle_edge(location, producer, consumer);
        }
    }
    // Long spine already persisted — skip envelope walk on the abort path.
    if policy.pairs_of(location).len() > ORDER_WINDOW_K {
        return;
    }
    let from = hints.from_of(consumer);
    let to = hints.to_of(consumer);
    let later = location_successors(hints, location, consumer, from, to);
    let mut pred = consumer;
    for succ in later {
        policy.note_short_pair(location, pred, succ);
        pred = succ;
    }
}

/// T3: queue the next `w` idle hops on `ℓ` after a published pred.
///
/// Never inserts ReadyEdges here (validate-time plant livelocks ERC-20).
/// `flush_pending_idle_edges` at the next pick quantum plants idle hops only.
pub(crate) fn queue_nearest_unfinished_successor(
    ready: &ReadyEdgeTable,
    policy: &CostPolicy,
    hints: &AccountHints,
    location: MemoryLocationHash,
    published: TxIdx,
    from: Address,
    to: Option<Address>,
) {
    let n_pairs = policy.pairs_of(location).len();
    if policy.hops_to_plant(location, n_pairs) == 0 {
        return;
    }
    let w = policy
        .window_w_of(location, n_pairs)
        .clamp(1, policy.w_cap_for(location, n_pairs).max(1));
    let mut hops: Vec<(TxIdx, TxIdx)> = policy
        .pairs_of(location)
        .into_iter()
        .filter(|&(pred, succ)| pred >= published && pred < succ)
        .collect();
    let later = location_successors(hints, location, published, from, to);
    if hops.is_empty() {
        if let Some(&succ) = later
            .iter()
            .find(|&&s| s > published && !ready.is_started(s))
        {
            hops.push((published, succ));
        }
    }
    hops.sort_unstable();
    hops.dedup();
    // O1: leftover past the begin prefix is one cheap continuation hop,
    // not a w-wide (or full-spine) plant. Proven cover (O/S) leaves
    // leftover OCC — do not T3-slide extra Detect hops.
    let prefix_end = policy
        .pairs_of(location)
        .into_iter()
        .take(w)
        .last()
        .map(|(_, succ)| succ);
    let leftover = prefix_end.is_some_and(|end| published >= end);
    if leftover && !policy.leftover_slide_ok(location) {
        return;
    }
    hops.truncate(if leftover { 1 } else { w });
    for (pred, succ) in hops {
        if succ > published && !ready.is_started(succ) && !ready.is_writer_done(pred) {
            policy.queue_idle_edge(location, pred, succ);
        }
    }
}

/// T3: plant queued idle hops at the next pick quantum / next incarnation.
///
/// Started successors stay OptimisticRead this incarnation. Preds that are
/// already done-stamped are skipped (done-stamp race / ERC-20 livelock).
/// Cap is the learned Win_w — never a full-spine prepaid list.
pub(crate) fn flush_pending_idle_edges(ready: &ReadyEdgeTable, policy: &CostPolicy) -> usize {
    let pending = policy.take_pending_idle();
    if pending.is_empty() {
        return 0;
    }
    let mut by_loc: HashMap<MemoryLocationHash, Vec<(TxIdx, TxIdx)>> = HashMap::new();
    for (loc, pred, succ) in pending {
        if pred < succ {
            by_loc.entry(loc).or_default().push((pred, succ));
        }
    }
    let mut planted = 0;
    for (loc, mut pairs) in by_loc {
        pairs.sort_unstable();
        pairs.dedup();
        let n_pairs = policy.pairs_of(loc).len().max(pairs.len());
        if policy.hops_to_plant(loc, n_pairs) == 0 {
            continue;
        }
        // O1: next-quantum flush is leftover continuation — 1 hop, never
        // a begin-width or full-spine prepaid list.
        if pairs.len() > 1 {
            pairs.truncate(1);
        }
        for (pred, succ) in pairs {
            if pred >= succ || ready.is_started(succ) || ready.is_writer_done(pred) {
                continue;
            }
            if ready.note_consumer_on_if_idle(succ, pred, Some(loc)) {
                policy.note_short_edge_admit();
                policy.note_loc_ordered_ns(loc, 1);
                planted += 1;
            }
        }
    }
    planted
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
        // Structural A1 tests use a full shell (n > THIN_N_MAX) that is
        // still below FAT_N — fat EmptyTo is A0 (C1/P2).
        let policy = policy_for(400);
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
            None,
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
    fn calldata_fanout_stays_optimistic_read() {
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
        assert_eq!(n, 0, "wide RAW fan stays OptimisticRead (ERC-20 / P1): {n}");
        assert!(
            ready.may_execute(15),
            "same-to calldata independents must not wait on tx 0"
        );
        assert!(
            ready.blocking_producer(15).is_none(),
            "must not plant a 16-wide probe star"
        );
        let _ = (learner, token);
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
        let n = note_probe_star(&ready, hints.to_txs(&to), loc, &mut queued, false);
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
        assert_eq!(n, 0, "2-tx same-from stays A0 (no refuse tax)");
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
        assert_eq!(n, 0, "wide RAW calldata fan stays OptimisticRead, got {n}");
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
    fn a0_unique_write_records_d1_without_edge() {
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let policy = policy_for(176);
        assert!(policy.is_optimistic_majority_block());
        let to = Address::repeat_byte(0x99);
        let hot = Address::repeat_byte(0x32);
        let loc = 0x32be_u64;
        admit_seed_on_write_set(
            &ready,
            &AccountHints::from_to_txs(to, vec![4]),
            &wave,
            Some(&policy),
            4,
            hot,
            Some(to),
            &[loc],
            &[loc],
        );
        assert!(
            ready.writers_of(loc).is_empty(),
            "O1: thin A0 unique write is OCC — D1 comes from end-block MV"
        );
        assert!(
            ready.may_execute(31),
            "unique writer with no hint successor must not refuse 31"
        );
    }

    #[test]
    fn thin_write_set_promotes_14_to_16() {
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let policy = policy_for(176);
        let contract = Address::repeat_byte(0xed);
        let from = Address::repeat_byte(0x14);
        let loc = 0xedba_u64;
        let hints = AccountHints::from_call_to_txs(contract, vec![14, 16, 17]);
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            Some(&policy),
            14,
            from,
            Some(contract),
            &[loc],
            &[loc],
        );
        assert_eq!(
            ready.blocking_producer(16),
            Some(14),
            "C1/L1: first effective storage write must raise 14→16"
        );
        assert!(
            ready.may_execute(17),
            "C5: short edge only — 17 stays A0 until 16 publishes"
        );
    }

    #[test]
    fn thin_write_set_promotes_4_to_31() {
        let ready = ReadyEdgeTable::new();
        let wave = WaveParkTable::new();
        let policy = policy_for(176);
        let to = Address::repeat_byte(0x20);
        let hot = Address::repeat_byte(0x32);
        let other = Address::repeat_byte(0x99);
        let loc = 0x32be_u64;
        admit_seed_on_write_set(
            &ready,
            &AccountHints::from_to_txs(other, vec![4]),
            &wave,
            Some(&policy),
            4,
            hot,
            Some(other),
            &[loc],
            &[loc],
        );
        // Thin A0 write-set does not record the long Basic spine (OCC path).
        assert!(ready.writers_of(loc).is_empty() || ready.writers_of(loc) == vec![4]);
        let hints = AccountHints::from_to_txs(to, vec![31, 66, 67]);
        admit_seed_on_write_set(
            &ready,
            &hints,
            &wave,
            Some(&policy),
            31,
            Address::repeat_byte(0x31),
            Some(to),
            &[loc],
            &[loc],
        );
        assert!(
            ready.may_execute(31) && ready.may_execute(66) && ready.may_execute(67),
            "O1/O2: thin write-set must not prepaid-serialize the long Basic spine"
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
            "2-tx same-from Data nonce must stay A0"
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

    #[test]
    fn thin_cold_start_seeds_no_a1() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let payee = Address::repeat_byte(0x20);
        let hints = AccountHints::from_to_txs(payee, (0..16).collect());
        let mut contracts = HashSet::new();
        contracts.insert(payee);
        let policy = policy_for(176);
        assert!(policy.is_optimistic_majority_block());
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
            &policy,
            &contracts,
            None,
        );
        assert_eq!(n, 0, "C4: empty-to payee spine stays A0 on thin, got {n}");
        assert!(ready.may_execute(0), "probe head stays runnable");
        assert!(ready.may_execute(8), "later empty-to stays OCC-runnable");
    }

    #[test]
    fn thin_begin_seeds_call_waw_chain() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let storage = Address::repeat_byte(0xed);
        let hints = AccountHints::from_call_to_txs(storage, vec![14, 16, 17]);
        let policy = policy_for(176);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
            &policy,
            &HashSet::new(),
            None,
        );
        assert_eq!(n, 2, "C1: 14→16→17 is two short edges, got {n}");
        assert_eq!(ready.blocking_producer(16), Some(14));
        assert_eq!(ready.blocking_producer(17), Some(16));
        assert!(ready.may_execute(14), "storage head stays runnable");
    }

    #[test]
    fn thin_begin_each_call_waw_picks_independently() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let storage = Address::repeat_byte(0xed);
        let second = Address::repeat_byte(0xe9);
        let hints =
            AccountHints::from_two_call_to(storage, vec![14, 16, 17], second, vec![15, 19, 20]);
        let policy = policy_for(176);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
            &policy,
            &HashSet::new(),
            None,
        );
        // O2: cold begin plants the first trio only (14→16→17). 15→19→20
        // waits for a measured abort so we do not pay two extra holes.
        assert_eq!(n, 2, "O2: cold hint plants one CallWaw trio, got {n}");
        assert_eq!(ready.blocking_producer(16), Some(14));
        assert_eq!(ready.blocking_producer(17), Some(16));
        assert!(
            ready.may_execute(19) && ready.may_execute(20),
            "O2: second trio stays unfenced at cold begin"
        );
    }

    #[test]
    fn thin_begin_call_waw_chain_not_star() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let main = Address::repeat_byte(0x20);
        let hints = AccountHints::from_call_to_txs(main, vec![31, 66, 67, 69, 70, 93, 96, 103]);
        let policy = policy_for(176);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
            &policy,
            &HashSet::new(),
            None,
        );
        assert_eq!(n, 0, "C5: wide CallWaw stays A0 at thin begin, got {n}");
        assert!(ready.may_execute(31), "wide head stays runnable");
        assert!(ready.may_execute(66), "wide tail stays runnable");
    }

    #[test]
    fn wide_envelope_writer_set_skips_empty_to_not_hidden_basic() {
        let to = Address::repeat_byte(0x20);
        let writers: Vec<TxIdx> = vec![
            31, 66, 67, 69, 70, 93, 96, 103, 115, 131, 132, 135, 138, 141, 166, 171,
        ];
        let hints = AccountHints::from_to_txs(to, writers.clone());
        assert!(
            is_wide_envelope_writer_set(&hints, &writers),
            "pure 0x209c envelope must not be persisted as a location chain"
        );
        let mut with_hot = writers.clone();
        with_hot.insert(0, 4);
        assert!(
            !is_wide_envelope_writer_set(&hints, &with_hot),
            "Basic(0x32be) writers 4∪0x209c are not a wide envelope"
        );
    }

    #[test]
    fn abort_persists_hidden_basic_chain() {
        let policy = policy_for(176);
        let to = Address::repeat_byte(0x20);
        let loc = 0x32be_u64;
        let hints = AccountHints::from_to_txs(to, vec![31, 66, 67, 69]);
        policy.promote_short_edge(loc, 0);
        persist_short_chain_after_abort(&hints, &policy, 31, 4, loc);
        let pairs = policy.promoted_short_pairs();
        assert!(
            pairs.iter().any(|&(l, a, b)| l == loc && a == 4 && b == 31),
            "abort must persist the proven 4→31 pair: {pairs:?}"
        );
        assert!(
            !pairs
                .iter()
                .any(|&(l, a, b)| l == loc && a == 31 && b == 66),
            "empty-to envelope must not be cloned onto a hidden ℓ: {pairs:?}"
        );
    }

    #[test]
    fn abort_does_not_chain_wide_callwaw() {
        let policy = policy_for(176);
        let token = Address::repeat_byte(0xaa);
        let loc = 0xabc_u64;
        let hints = AccountHints::from_call_to_txs(token, (0..16).collect());
        policy.promote_short_edge(loc, 0);
        persist_short_chain_after_abort(&hints, &policy, 3, 1, loc);
        let pairs = policy.promoted_short_pairs();
        assert!(
            pairs.iter().any(|&(l, a, b)| l == loc && a == 1 && b == 3),
            "producer→consumer pair is kept: {pairs:?}"
        );
        assert!(
            !pairs.iter().any(|&(l, a, b)| l == loc && a == 3 && b == 4)
                && !pairs.iter().any(|&(l, _, s)| l == loc && s == 15),
            "C5: wide CallWaw must not persist ERC-20 slot stars: {pairs:?}"
        );
    }

    #[test]
    fn thin_begin_reuses_main_chain_pairs() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let policy = policy_for(176);
        policy.promote_short_edge(0x32be, 40_000);
        policy.note_short_pair(0x32be, 4, 31);
        policy.note_short_pair(0x32be, 31, 66);
        policy.note_short_pair(0x32be, 66, 67);
        policy.end_block_learn();
        policy.begin_block(176);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &AccountHints::default(),
            Address::ZERO,
            &policy,
            &HashSet::new(),
            None,
        );
        assert!(
            n >= 1 && n < 4,
            "T1: reuse plants OrderedWindow hops, not the full 4-pair list, got {n}"
        );
        assert_eq!(
            ready.blocking_producer(31),
            Some(4),
            "T1: head hop 4→31 is always planted"
        );
        assert!(ready.may_execute(4), "chain head stays runnable");
        // Win_1 leaves 66/67 OptimisticRead; F7 may plant Win_2/3. Never FullChain.
        assert!(
            n < 4,
            "T1: must not plant the full 4→31→66→67 prepaid list at begin, got {n}"
        );
    }

    #[test]
    fn thin_begin_k_caps_promoted_locations() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let policy = policy_for(176);
        policy.promote_short_edge(0x32be, 40_000);
        policy.note_short_pair(0x32be, 4, 31);
        policy.note_short_pair(0x32be, 31, 66);
        policy.note_short_pair(0x32be, 66, 67);
        policy.promote_short_edge(0xedba, 20_000);
        policy.note_short_pair(0xedba, 14, 16);
        policy.note_short_pair(0xedba, 16, 17);
        policy.promote_short_edge(0xa1, 10_000);
        policy.note_short_pair(0xa1, 10, 11);
        policy.promote_short_edge(0xa2, 10_000);
        policy.note_short_pair(0xa2, 12, 13);
        policy.promote_short_edge(0xa3, 10_000);
        policy.note_short_pair(0xa3, 18, 19);
        policy.end_block_learn();
        policy.begin_block(176);
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &AccountHints::default(),
            Address::ZERO,
            &policy,
            &HashSet::new(),
            None,
        );
        assert_eq!(
            ready.blocking_producer(31),
            Some(4),
            "T1: long Basic spine always plants 4→31"
        );
        assert!(ready.may_execute(4), "chain head stays runnable");
        assert_eq!(ready.blocking_producer(16), Some(14));
        let extra_blocked = [11usize, 13, 19]
            .iter()
            .filter(|&&t| !ready.may_execute(t))
            .count();
        assert!(
            extra_blocked <= 2 && n <= policy.w_cap_of(3) + 2 + 1,
            "C5: thin reuse plants ≤K locations; long spine is Win_w (extras_blocked={extra_blocked} edges={n})"
        );
    }

    #[test]
    fn flush_pending_idle_plants_window_not_started() {
        let ready = ReadyEdgeTable::new();
        let policy = policy_for(176);
        policy.promote_short_edge(0x32be, 40_000);
        persist_short_chain_after_abort(&AccountHints::default(), &policy, 31, 4, 0x32be);
        policy.note_short_pair(0x32be, 31, 66);
        policy.note_short_pair(0x32be, 66, 67);
        ready.note_started(67);
        let n = flush_pending_idle_edges(&ready, &policy);
        assert!(n >= 1, "O3: idle 4→31 must plant, got {n}");
        assert_eq!(ready.blocking_producer(31), Some(4));
        assert!(
            ready.may_execute(67),
            "O3: started 67 must stay A0 this incarnation"
        );
    }

    #[test]
    fn leftover_opt_abort_does_not_queue_idle() {
        let policy = policy_for(176);
        policy.promote_short_edge(0x32be, 40_000);
        for pair in [
            (4, 31),
            (31, 66),
            (66, 67),
            (67, 69),
            (69, 70),
            (70, 93),
            (93, 96),
            (96, 103),
        ] {
            policy.note_short_pair(0x32be, pair.0, pair.1);
        }
        policy.remember_arm(
            0x32be,
            crate::specfence::policy::LocStrategy::OptimisticRead,
        );
        persist_short_chain_after_abort(&AccountHints::default(), &policy, 166, 141, 0x32be);
        assert_eq!(
            policy.hops_to_plant(0x32be, 8),
            0,
            "C1: leftover Opt plants zero hops"
        );
        assert!(
            !policy.has_pending_idle(),
            "C1: hops=0 must not queue a pick-quantum hop the flush would drop"
        );
        assert!(
            policy
                .pairs_of(0x32be)
                .iter()
                .any(|&(a, b)| a == 141 && b == 166),
            "C1: proven pair is still persisted for the next begin"
        );
    }

    #[test]
    fn leftover_continuation_is_one_hop() {
        let ready = ReadyEdgeTable::new();
        let policy = policy_for(176);
        policy.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            policy.note_short_pair(0x32be, pair.0, pair.1);
        }
        policy.remember_arm(0x32be, crate::specfence::policy::LocStrategy::win(2));
        queue_nearest_unfinished_successor(
            &ready,
            &policy,
            &AccountHints::default(),
            0x32be,
            66,
            Address::ZERO,
            None,
        );
        let n = flush_pending_idle_edges(&ready, &policy);
        assert!(
            n <= 1,
            "O1: leftover continuation past Win_2 prefix is ≤1 hop, got {n}"
        );
    }

    #[test]
    fn cover_ok_does_not_queue_leftover_slide() {
        let ready = ReadyEdgeTable::new();
        let policy = policy_for(176);
        policy.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            policy.note_short_pair(0x32be, pair.0, pair.1);
        }
        policy.remember_arm(0x32be, crate::specfence::policy::LocStrategy::win(2));
        policy.test_set_cover_flags(0x32be, true, false, false, false);
        queue_nearest_unfinished_successor(
            &ready,
            &policy,
            &AccountHints::default(),
            0x32be,
            66,
            Address::ZERO,
            None,
        );
        assert!(
            !policy.has_pending_idle(),
            "O: proven cover must not queue leftover Detect hops"
        );
        let n = flush_pending_idle_edges(&ready, &policy);
        assert_eq!(n, 0, "O: no leftover slide after cover_ok, got {n}");
    }

    #[test]
    fn flush_skips_done_pred_and_started_succ() {
        let ready = ReadyEdgeTable::new();
        let policy = policy_for(176);
        policy.promote_short_edge(0x32be, 40_000);
        policy.note_short_pair(0x32be, 4, 31);
        policy.note_short_pair(0x32be, 31, 66);
        policy.queue_idle_edge(0x32be, 4, 31);
        policy.queue_idle_edge(0x32be, 31, 66);
        ready.note_producer_done_stamp(4);
        ready.note_started(66);
        let n = flush_pending_idle_edges(&ready, &policy);
        assert_eq!(
            n, 0,
            "T3: done pred and started succ must not plant, got {n}"
        );
        assert!(
            ready.may_execute(31) && ready.may_execute(66),
            "T3: skipped hops stay OptimisticRead"
        );
    }

    #[test]
    fn fat_empty_to_does_not_plant() {
        let ready = ReadyEdgeTable::new();
        let stages = ProducerStageTable::new();
        let learner = LiveLearner::new();
        learner.begin_block(MorphWeights::default());
        let bayes = BayesMap::new();
        let prior = InterBlockPrior::new();
        let payee = Address::repeat_byte(0x20);
        let hints = AccountHints::from_to_txs(payee, (0..64).collect());
        let mut contracts = HashSet::new();
        contracts.insert(payee);
        let policy = policy_for(800);
        assert!(!policy.is_optimistic_majority_block());
        let n = admit_seed_begin_block(
            &ready,
            &stages,
            &learner,
            &bayes,
            &prior,
            &hints,
            Address::ZERO,
            &policy,
            &contracts,
            None,
        );
        assert_eq!(
            n, 0,
            "C1/P2: fat EmptyTo must not plant a lazy star, got {n}"
        );
        assert!(ready.may_execute(1) && ready.may_execute(32));
    }
}
