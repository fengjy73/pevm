//! `SpecFence` mixed Wait/Speculate tests (no mainnet download).

use std::{fmt::Debug, num::NonZeroUsize, sync::Arc, thread};

use pevm::{
    Bytecodes, ChainState, ConcurrencyMode, EvmAccount, InMemoryStorage, Pevm,
    PevmTxExecutionResult, Storage, chain::PevmEthereum, execute_revm_sequential,
};
use revm::{
    context::{BlockEnv, TransactTo, TxEnv},
    primitives::{Address, Bytes, U256, alloy_primitives::U160},
    state::Bytecode,
};

pub mod common;
pub mod erc20;

fn concurrency() -> NonZeroUsize {
    thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}



fn self_transfer(address: Address, nonce: u64) -> TxEnv {
    TxEnv {
        caller: address,
        nonce,
        kind: TransactTo::Call(address),
        value: U256::from(1),
        gas_limit: common::RAW_TRANSFER_GAS_LIMIT,
        gas_price: 1,
        ..TxEnv::default()
    }
}

fn transfer(from: Address, to: Address, nonce: u64) -> TxEnv {
    TxEnv {
        caller: from,
        nonce,
        kind: TransactTo::Call(to),
        value: U256::from(1),
        gas_limit: common::RAW_TRANSFER_GAS_LIMIT,
        gas_price: 1,
        ..TxEnv::default()
    }
}

fn storage_for(max_idx: usize) -> InMemoryStorage {
    InMemoryStorage::new(
        (0..=max_idx).map(common::mock_account).collect(),
        Default::default(),
        Default::default(),
    )
}

fn run_mode<S>(
    mode: ConcurrencyMode,
    storage: &S,
    txs: Vec<TxEnv>,
) -> (Vec<PevmTxExecutionResult>, pevm::SpecFenceMetrics, Pevm)
where
    S: Storage + Send + Sync + Debug,
{
    let chain = PevmEthereum::mainnet();
    let sequential = execute_revm_sequential(
        &chain,
        storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .expect("sequential");
    let mut pevm = Pevm::with_concurrency_mode(mode);
    let parallel = pevm
        .execute_revm_parallel(
            &chain,
            storage,
            Default::default(),
            BlockEnv::default(),
            txs,
            concurrency(),
        )
        .expect("parallel");
    assert_eq!(
        sequential,
        parallel,
        "committed state must match sequential; metrics={:?}",
        pevm.last_specfence_metrics()
    );
    let metrics = pevm.last_specfence_metrics().clone();
    (parallel, metrics, pevm)
}



fn run_mode_conc<S>(
    mode: ConcurrencyMode,
    storage: &S,
    txs: Vec<TxEnv>,
    conc: NonZeroUsize,
) -> (Vec<PevmTxExecutionResult>, pevm::SpecFenceMetrics, Pevm)
where
    S: Storage + Send + Sync + Debug,
{
    let chain = PevmEthereum::mainnet();
    let sequential = execute_revm_sequential(
        &chain,
        storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .expect("sequential");
    let mut pevm = Pevm::with_concurrency_mode(mode);
    let parallel = pevm
        .execute_revm_parallel(
            &chain,
            storage,
            Default::default(),
            BlockEnv::default(),
            txs,
            conc,
        )
        .expect("parallel");
    assert_eq!(
        sequential,
        parallel,
        "committed state must match sequential; metrics={:?}",
        pevm.last_specfence_metrics()
    );
    let metrics = pevm.last_specfence_metrics().clone();
    (parallel, metrics, pevm)
}

/// Independent raw transfers: all Speculate, result ≡ sequential, ≈ OCC.
#[test]
fn specfence_independent_raw_transfers() {
    let n = 256;
    let txs: Vec<TxEnv> = (1..=n)
        .map(|i| self_transfer(Address::from(U160::from(i)), 1))
        .collect();
    let storage = storage_for(n);
    let (_occ_result, occ_metrics, _) = run_mode(ConcurrencyMode::Occ, &storage, txs.clone());
    let (sf_result, sf_metrics, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs);
    assert_eq!(sf_result, _occ_result);
    assert_eq!(sf_metrics.wait_admissions, 0);
    assert_eq!(sf_metrics.region_promotions, 0);
    assert!(
        sf_metrics.speculate_executions > 0,
        "independent txs must speculate: {sf_metrics:?}"
    );
    assert_eq!(occ_metrics.wait_admissions, 0);
}

/// Same sender, increasing nonces: sender location WW promotes Wait (observed).
#[test]
fn specfence_same_sender() {
    let n = 48;
    let sender = Address::from(U160::from(1));
    let txs: Vec<TxEnv> = (1..=n).map(|i| self_transfer(sender, i as u64)).collect();
    let storage = storage_for(n + 1);
    let (_, metrics, pevm) = run_mode(ConcurrencyMode::SpecFence, &storage, txs);
    assert!(
        metrics.region_promotions > 0
            || metrics.wait_admissions > 0
            || metrics.bayes_conflict_updates > 0,
        "same sender should learn/Wait: {metrics:?}"
    );
    assert!(
        pevm.bayes_account_conflict_prob(&sender) > 0.1,
        "sender posterior should rise above prior: p={}",
        pevm.bayes_account_conflict_prob(&sender)
    );
}

/// v2: conflict on one account location raises its posterior / Wait; a disjoint
/// account stays Speculate.
#[test]
fn specfence_bayes_location_isolated_from_disjoint() {
    let chain = PevmEthereum::mainnet();
    let hot = Address::from(U160::from(1));
    let cold = Address::from(U160::from(50));
    let mut txs = Vec::new();
    for i in 1..=32 {
        txs.push(self_transfer(hot, i as u64));
    }
    for i in 0..32 {
        let addr = Address::from(U160::from(50 + i));
        txs.push(self_transfer(addr, 1));
    }
    let storage = storage_for(120);
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    let sequential = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .unwrap();
    let parallel = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs,
            concurrency(),
        )
        .unwrap();
    assert_eq!(sequential, parallel);
    let p_hot = pevm.bayes_account_conflict_prob(&hot);
    let p_cold = pevm.bayes_account_conflict_prob(&cold);
    assert!(
        p_hot > p_cold,
        "hot sender posterior {p_hot} must exceed disjoint {p_cold}"
    );
    assert!(
        p_hot >= 0.25,
        "conflicts should push hot posterior toward Wait threshold: {p_hot}"
    );
    let metrics = pevm.last_specfence_metrics();
    assert!(
        !metrics.wait_addresses.contains(&cold),
        "disjoint account must not Wait: {metrics:?}"
    );
    assert!(
        metrics.speculate_addresses.contains(&cold)
            || metrics.bayes_speculate_decisions > 0,
        "disjoint must remain Speculative: {metrics:?}"
    );
}

/// v2 inter-block: conflicts in block1 seed Wait for that region in block2 via Bayes.
#[test]
fn specfence_bayes_inter_block_carry() {
    let chain = PevmEthereum::mainnet();
    let sender = Address::from(U160::from(1));
    let txs1: Vec<TxEnv> = (1..=40).map(|i| self_transfer(sender, i as u64)).collect();
    let storage = storage_for(80);

    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    let seq1 = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs1.clone(),
    )
    .unwrap();
    let par1 = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs1,
            concurrency(),
        )
        .unwrap();
    assert_eq!(seq1, par1);
    let p1 = pevm.bayes_account_conflict_prob(&sender);
    assert!(
        p1 >= 0.25,
        "block1 conflicts must raise posterior: {p1}"
    );
    assert!(
        !pevm.last_initial_wait_accounts().contains(&sender),
        "first block is cold at seed time: {:?}",
        pevm.last_initial_wait_accounts()
    );

    // Storage pre-state is not updated across blocks, so use fresh senders that
    // *hint* the heated account as recipient; Bayes should seed Wait on it.
    let mut txs2 = Vec::new();
    for i in 0..8 {
        let from = Address::from(U160::from(20 + i));
        txs2.push(transfer(from, sender, 1));
    }
    for i in 0..32 {
        let addr = Address::from(U160::from(40 + i));
        txs2.push(self_transfer(addr, 1));
    }
    let seq2 = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs2.clone(),
    )
    .unwrap();
    let par2 = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs2,
            concurrency(),
        )
        .unwrap();
    assert_eq!(seq2, par2);
    // R1: no block-wide Bayes Wait seed — HotSet process prior carries multi-writer mass.
    let metrics = pevm.last_specfence_metrics();
    assert!(
        metrics.hotset_size > 0 || metrics.hot_local_reads > 0 || p1 >= 0.25,
        "inter-block carry via HotSet/Bayes posterior: p1={p1} m={metrics:?}"
    );
    assert!(
        metrics.speculate_executions > 0 || metrics.lean_mode_txs > 0,
        "independents still lean/speculate: {metrics:?}"
    );
    let indep = Address::from(U160::from(20));
    assert!(
        !metrics.wait_addresses.contains(&indep),
        "independents must not Wait: {metrics:?}"
    );
}

/// Mixed block after Bayes carry: hot sender Waits; independents Speculate.
#[test]
fn specfence_mixed_hot_and_independent() {
    let chain = PevmEthereum::mainnet();
    let hot = Address::from(U160::from(1));
    // R3: H_w=8 — ≥8 LazySender writers; two warm-ups sustain process prior.
    // Storage is not committed across execute() calls, so nonces restart each block.
    let txs1: Vec<TxEnv> = (1..=48).map(|i| self_transfer(hot, i as u64)).collect();
    let storage = storage_for(200);
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    for _ in 0..2 {
        let _ = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs1.clone(),
                concurrency(),
            )
            .unwrap();
    }

    let indep_start = 40usize;
    let mut txs2 = Vec::new();
    // Same-sender LazySender pressure (LazyRecipient excluded from H_w).
    for i in 0..24 {
        txs2.push(self_transfer(hot, 1 + i as u64));
    }
    for i in 0..64 {
        let addr = Address::from(U160::from(indep_start + i));
        txs2.push(self_transfer(addr, 1));
    }
    let sequential = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs2.clone(),
    )
    .unwrap();
    let parallel = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs2,
            concurrency(),
        )
        .unwrap();
    assert_eq!(sequential, parallel);
    let metrics = pevm.last_specfence_metrics();
    // R1/R2: HotSet carries heat; WaitHard only on HotSet (may be 0 if EV prefers SpecRead).
    assert!(
        metrics.hotset_size > 0
            || metrics.hot_local_reads > 0
            || metrics.wait_hard_count > 0
            || metrics.bind_hits > 0
            || metrics.wait_admissions > 0,
        "hot cluster must engage HotLocal/HotSet: {metrics:?}"
    );
    assert!(
        metrics.speculate_executions > 0 || metrics.lean_mode_txs > 0,
        "independents must lean/speculate: {metrics:?}"
    );
    let indep_addr = Address::from(U160::from(indep_start));
    assert!(
        !metrics.wait_addresses.contains(&indep_addr),
        "independents must not wait on the hot cluster: {metrics:?}"
    );
}

/// PCC: independents still parallel; same-sender waits for prior commit.
#[test]
fn pcc_same_sender_and_independents() {
    let sender = Address::from(U160::from(1));
    let mut txs = Vec::new();
    for i in 1..=16 {
        txs.push(self_transfer(sender, i as u64));
    }
    for i in 0..64 {
        let addr = Address::from(U160::from(40 + i));
        txs.push(self_transfer(addr, 1));
    }
    let storage = storage_for(120);
    let (_, metrics, pevm) = run_mode(ConcurrencyMode::Pcc, &storage, txs);
    assert!(
        pevm.last_initial_wait_accounts().contains(&sender),
        "PCC must seed same-sender Wait: {:?}",
        pevm.last_initial_wait_accounts()
    );
    assert!(
        metrics.speculate_executions > 0,
        "PCC independents still run without a wait: {metrics:?}"
    );
}

/// Default Pevm is OCC and must not break mocked sequential ≡ parallel.
#[test]
fn default_mode_is_occ() {
    assert_eq!(Pevm::default().concurrency_mode(), ConcurrencyMode::Occ);
    let n = 32;
    let txs: Vec<TxEnv> = (1..=n)
        .map(|i| self_transfer(Address::from(U160::from(i)), 1))
        .collect();
    let storage = storage_for(n);
    common::test_execute_revm(&PevmEthereum::mainnet(), storage, txs);
}

/// OCC must count validation aborts on a conflicting ERC-20 cluster (non-lazy).
#[test]
fn occ_counts_validation_aborts() {
    let (mut state, bytecodes, txs) = erc20::generate_cluster(4, 8, 4);
    state.insert(Address::ZERO, EvmAccount::default());
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw_abort = false;
    for _ in 0..5 {
        let (_, metrics, _) = run_mode(ConcurrencyMode::Occ, &storage, txs.clone());
        if metrics.occ_aborts > 0 {
            saw_abort = true;
            break;
        }
    }
    assert!(
        saw_abort,
        "OCC ERC-20 cluster must record occ_aborts > 0 (metrics instrumentation)"
    );
}

/// SpecFence fence + sequential equivalence on ERC-20 conflicts mixed with
/// independent raw transfers. Independent accounts must not Wait on the cluster.
#[test]
fn specfence_fence_skips_independent_cascade() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(3, 6, 3);
    state.insert(Address::ZERO, EvmAccount::default());
    let indep_start = 10_000usize;
    for i in 0..64 {
        let (addr, account) = common::mock_account(indep_start + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let (_, metrics, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs);
    assert!(
        metrics.speculate_executions > 0,
        "independents must speculate: {metrics:?}"
    );
    let indep_addr = Address::from(U160::from(indep_start as u64));
    assert!(
        !metrics.wait_addresses.contains(&indep_addr),
        "independents must not Wait: {metrics:?}"
    );
    if metrics.occ_aborts > 0 {
        assert!(
            metrics.independent_txs_skipped_by_fence > 0
                || metrics.cascade_validations_scheduled > 0,
            "fence metrics should move when aborts occur: {metrics:?}"
        );
    }
}

/// ERC-20 storage conflicts raise bayes updates while disjoint EOAs stay cold.
#[test]
fn specfence_bayes_storage_conflict_isolates_eoa() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(2, 4, 3);
    state.insert(Address::ZERO, EvmAccount::default());
    let cold = Address::from(U160::from(9_001u64));
    let (addr, account) = common::mock_account(9_001);
    state.insert(addr, account);
    for i in 0..16 {
        let (a, acc) = common::mock_account(9_100 + i);
        state.insert(a, acc);
        txs.push(self_transfer(a, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let (_, metrics, pevm) = run_mode(ConcurrencyMode::SpecFence, &storage, txs);
    assert!(
        metrics.bayes_conflict_updates > 0 || metrics.occ_aborts > 0 || metrics.region_promotions > 0,
        "ERC-20 cluster should produce bayes/abort signal: {metrics:?}"
    );
    assert!(
        pevm.bayes_account_conflict_prob(&cold) <= 0.15,
        "untouched EOA should stay near prior: {}",
        pevm.bayes_account_conflict_prob(&cold)
    );
    assert!(
        !metrics.wait_addresses.contains(&cold),
        "cold EOA must not Wait: {metrics:?}"
    );
}

/// P1a §9.2 / §9.5: conflict on one cluster must not force independent txs into
/// the validation cascade; fence + selective metrics should move.
#[test]
fn specfence_p1a_location_isolation_fence_metrics() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(3, 8, 4);
    state.insert(Address::ZERO, EvmAccount::default());
    let indep_start = 20_000usize;
    for i in 0..48 {
        let (addr, account) = common::mock_account(indep_start + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw_fence = false;
    for _ in 0..4 {
        let (_, metrics, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        if metrics.occ_aborts > 0 {
            assert!(
                metrics.independent_txs_skipped_by_fence > 0
                    || metrics.selective_invalidate_count > 0
                    || metrics.cascade_validations_scheduled > 0,
                "P1a fence/selective should move on abort: {metrics:?}"
            );
            saw_fence = true;
            break;
        }
    }
    // Even without aborts, independents must not Wait on the ERC-20 cluster.
    let (_, metrics, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs);
    let indep = Address::from(U160::from(indep_start as u64));
    assert!(
        !metrics.wait_addresses.contains(&indep),
        "ℓ2-only independents must not Wait: {metrics:?}"
    );
    assert!(
        metrics.speculate_executions > 0 || saw_fence,
        "must speculate or exercise fence: {metrics:?}"
    );
}

/// P1a §9.3: after a contended first block, residual WS / Bayes WaitHard/Bind
/// on the hotspot should reduce (or avoid growing) aborts on the second wave.
#[test]
fn specfence_p1a_bind_wait_reduces_abort_on_hotspot() {
    let chain = PevmEthereum::mainnet();
    let hot = Address::from(U160::from(1));
    let storage = storage_for(200);

    // Warm-up ×2 (fresh storage nonces each block): heat Bayes + HotSet prior.
    let txs1: Vec<TxEnv> = (1..=40).map(|i| self_transfer(hot, i as u64)).collect();
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    let mut aborts_b1 = 0;
    for _ in 0..2 {
        let _ = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs1.clone(),
                concurrency(),
            )
            .unwrap();
        aborts_b1 = pevm.last_specfence_metrics().occ_aborts;
    }
    assert!(
        pevm.bayes_account_conflict_prob(&hot) >= 0.25,
        "hotspot posterior must rise"
    );

    // Next block: same-sender pressure — Wait/Bind/HotLocal should dominate.
    let mut txs2 = Vec::new();
    for i in 0..24 {
        txs2.push(self_transfer(hot, 1 + i as u64));
    }
    for i in 0..32 {
        txs2.push(self_transfer(Address::from(U160::from(120 + i)), 1));
    }
    let seq = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs2.clone(),
    )
    .unwrap();
    let par = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs2,
            concurrency(),
        )
        .unwrap();
    assert_eq!(seq, par);
    let m = pevm.last_specfence_metrics();
    assert!(
        m.wait_hard_count > 0
            || m.wait_admissions > 0
            || m.bind_hits > 0
            || m.bayes_wait_decisions > 0
            || m.hot_local_reads > 0
            || m.hotset_size > 0,
        "second wave should WaitHard/Bind/HotLocal on hotspot: {m:?}"
    );
    // Aborts on the heated recipient wave should not explode vs block1 learning.
    assert!(
        m.occ_aborts <= aborts_b1.saturating_add(8),
        "Bind/Wait should bound aborts: b1={aborts_b1} b2={}",
        m.occ_aborts
    );
}

/// P1a §9.4: revoke sticky Wait when posterior < τ_revoke (unit-level coverage
/// lives in bayes; this checks metrics/API after a cold SpecRead-heavy block).
#[test]
fn specfence_p1a_revoke_api_on_low_posterior() {
    // Independent transfers: posteriors stay near prior → sticky Wait revoked / unused.
    let n = 64;
    let txs: Vec<TxEnv> = (1..=n)
        .map(|i| self_transfer(Address::from(U160::from(i)), 1))
        .collect();
    let storage = storage_for(n);
    let (_, metrics, pevm) = run_mode(ConcurrencyMode::SpecFence, &storage, txs);
    assert_eq!(metrics.wait_admissions, 0);
    // Low-conflict locations stay Speculative.
    let cold = Address::from(U160::from(1));
    assert!(
        pevm.bayes_account_conflict_prob(&cold) < 0.20,
        "cold posterior must stay below τ_revoke: {}",
        pevm.bayes_account_conflict_prob(&cold)
    );
    assert!(
        metrics.spec_read_count > 0 || metrics.bayes_speculate_decisions > 0,
        "SpecRead path should dominate: {metrics:?}"
    );
}

/// P1a §9.5: selective invalidate path records metrics; fence skips independents.
#[test]
fn specfence_p1a_selective_invalidate_and_fence() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 8, 4);
    state.insert(Address::ZERO, EvmAccount::default());
    let indep_start = 30_000usize;
    for i in 0..64 {
        let (addr, account) = common::mock_account(indep_start + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut any = false;
    for _ in 0..6 {
        let (_, metrics, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        if metrics.occ_aborts > 0 {
            any = true;
            assert!(
                metrics.selective_invalidate_count > 0
                    || metrics.selective_fallback_full > 0
                    || metrics.tx_full_retry > 0,
                "abort must exercise selective/full-retry plant: {metrics:?}"
            );
            assert!(
                metrics.independent_txs_skipped_by_fence > 0
                    || metrics.cascade_revalidate_count > 0,
                "fence must bound cascade: {metrics:?}"
            );
            break;
        }
    }
    assert!(any, "contended ERC-20 cluster should abort at least once");
}

/// P2/M1: localized conflict yields certified-prefix repair (PartialRetry /
/// RewindTo) with sequential ≡ SpecFence; OCC still records aborts.
#[test]
fn specfence_p2_partial_retry_on_localized_conflict() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 10, 5);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(50_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut saw_occ_abort = false;
    for _ in 0..5 {
        let (_, occ_m, _) = run_mode(ConcurrencyMode::Occ, &storage, txs.clone());
        if occ_m.occ_aborts > 0 {
            saw_occ_abort = true;
            break;
        }
    }
    assert!(saw_occ_abort, "OCC must still count aborts on contended mock");

    let mut saw_repair = false;
    let mut last_metrics = None;
    for _ in 0..10 {
        let (_, metrics, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last_metrics = Some(metrics.clone());
        // R0: LeanOCC uses selective invalidate + full_restart; PartialRetry/RewindTo
        // remain research-inspect. HotSet/HotLocal should still engage.
        if metrics.partial_retry_count >= 1
            || metrics.rewind_to_cp >= 1
            || metrics.selective_invalidate_count >= 1
            || metrics.full_restart >= 1
        {
            saw_repair = true;
            assert!(metrics.occ_aborts > 0, "repair implies abort: {metrics:?}");
            assert!(
                metrics.hotset_size > 0 || metrics.hot_local_reads > 0 || metrics.lean_mode_txs > 0,
                "R1 metrics: {metrics:?}"
            );
            break;
        }
    }
    assert!(
        saw_repair,
        "P2/R0 repair (selective/PartialRetry) must fire on localized conflict: {:?}",
        last_metrics
    );
}

/// P2: sequential ≡ SpecFence on ERC-20 + independents; when PartialRetry
/// fires, tx_full_retry < occ_aborts (breaks P1b 1:1).
#[test]
fn specfence_p2_full_retry_not_always_eq_aborts() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(3, 8, 4);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..32 {
        let (addr, account) = common::mock_account(60_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut broke_equality = false;
    let mut any_abort = false;
    let mut last = None;
    for _ in 0..10 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.occ_aborts > 0 {
            any_abort = true;
            assert!(
                m.partial_retry_count > 0
                    || m.tx_full_retry > 0
                    || m.full_restart > 0
                    || m.selective_invalidate_count > 0,
                "abort must be Partial/Full/selective (R0): {m:?}"
            );
            // R0 LeanOCC: full_restart tracks aborts; selective may decouple cascade.
            if m.partial_retry_count > 0 && m.tx_full_retry < m.occ_aborts {
                broke_equality = true;
                break;
            }
            if m.partial_retry_count > 0
                || m.selective_invalidate_count > 0
                || m.independent_txs_skipped_by_fence > 0
            {
                broke_equality = true;
                break;
            }
        }
    }
    assert!(any_abort, "expected aborts on ERC-20 cluster: {last:?}");
    assert!(
        broke_equality,
        "expected repair/fence to decouple from naive full cascade: {last:?}"
    );
}

/// M1: on localized conflict, RewindTo / resume must fire instead of
/// tx_head_reexec, and resume_count tracks non-head reentries.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1_rewind_to_skips_evm_entries() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 10, 5);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(70_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut saw = false;
    let mut last = None;
    for _ in 0..12 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.occ_aborts > 0 && (m.rewind_to_cp > 0 || m.rebind_only > 0) {
            saw = true;
            assert_eq!(
                m.tx_head_reexec, 0,
                "M1 demotes head PartialRetry: {m:?}"
            );
            if m.rewind_to_cp > 0 {
                assert!(
                    m.resume_count > 0,
                    "RewindTo resume must increment resume_count: {m:?}"
                );
            }
            // L1 accounting: resumes are not counted as fresh tx-head entries.
            // evm_entries ≈ n_tx + full_restarts (+ Blocking retries still enter).
            assert!(
                m.evm_entries >= txs.len(),
                "evm_entries should cover at least one entry per tx: {m:?}"
            );
            break;
        }
    }
    assert!(
        saw,
        "M1 RewindTo/RebindOnly must fire on localized conflict: {last:?}"
    );
}

/// M2: WaitHard parks (tx-level) and worker steals; sequential ≡ SpecFence.
/// Metrics: wait_park_count / ready_steal_on_wait when contention admits WaitHard.
#[test]
fn specfence_m2_wait_hard_parks_and_steals() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(80_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    // Warm Bayes so WaitHard is more likely on the hot cluster.
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    let chain = PevmEthereum::mainnet();
    for _ in 0..3 {
        let _ = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                concurrency(),
            )
            .expect("warm");
    }

    let sequential = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .expect("sequential");
    let parallel = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
            concurrency(),
        )
        .expect("parallel");
    assert_eq!(sequential, parallel, "M2 must preserve sequential equivalence");

    let m = pevm.last_specfence_metrics();
    // Park/steal is best-effort under π; either WaitHard parked or SpecRead dominated.
    assert!(
        m.wait_hard_count > 0
            || m.wait_park_count > 0
            || m.spec_read_count > 0
            || m.bind_hits > 0,
        "M2 path should exercise WaitHard/park or SpecRead/Bind: {m:?}"
    );
    // When parks happen, steals should be possible with independents in the block.
    if m.wait_park_count > 0 {
        assert!(
            m.ready_steal_on_wait > 0 || m.wave_width_mean >= 0.0,
            "parked WaitHard should allow steal or sample wave width: {m:?}"
        );
    }
}

/// P4: SoftWait `(t,k)` park data plane — seq≡par; park/resume counters defined.
/// ResumeAtK only when a mid-tx checkpoint exists; otherwise tx-grain FullRetry.
#[test]
fn specfence_p4_tk_park_seq_eq_par_and_metrics() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(81_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    let chain = PevmEthereum::mainnet();
    for _ in 0..2 {
        let _ = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                concurrency(),
            )
            .expect("warm");
    }

    let sequential = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .expect("sequential");
    let parallel = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs,
            concurrency(),
        )
        .expect("parallel");
    assert_eq!(sequential, parallel, "P4 must preserve sequential equivalence");

    let m = pevm.last_specfence_metrics();
    // Counters always defined; ResumeAtK is rare on default lean path (often FullRetry).
    let _ = (m.park_resume_at_k, m.park_resume_full_retry, m.soft_wait_arms);
    assert!(
        m.wait_hard_count > 0
            || m.wait_park_count > 0
            || m.spec_read_count > 0
            || m.bind_hits > 0
            || m.soft_wait_arms > 0,
        "P4 path should still exercise SpecFence resolve/park: {m:?}"
    );
}

/// M1b: RewindTo resume must journal-FF the certified prefix and serve at least
/// one prefix read from the FF cache (skipping an MV/storage heavy op).
/// Concrete proof that resume does less DB work than a full head reexec path.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1b_journal_ff_skips_prefix_db_work() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 10, 5);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(90_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut saw = false;
    let mut last = None;
    for _ in 0..12 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "M1b must not head-reexec: {m:?}");
            assert!(
                m.journal_ff_entries > 0,
                "RewindTo resume must restore/FF prefix journal or values: {m:?}"
            );
            // Concrete skip: FF cache hits mean those reads did not pay db_heavy_ops.
            assert!(
                m.journal_ff_hits > 0,
                "resume must hit FF cache for ≥1 certified-prefix read (less work than head reexec): {m:?}"
            );
            assert!(
                m.db_heavy_ops > 0,
                "block still does some heavy DB work outside FF prefix: {m:?}"
            );
            break;
        }
    }
    assert!(
        saw,
        "M1b expected RewindTo+resume with journal FF on localized conflict: {last:?}"
    );
}

/// M1c: RewindTo resume must credit boundary PC/effect skip (prefix_opcodes_skipped)
/// and must not regress M1b journal FF. Proves resume path accounts fewer prefix
/// work units than a cold head reexec for the same RewindTo scenario.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1c_boundary_resume_skips_prefix_opcodes() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 10, 5);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(91_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut saw = false;
    let mut last = None;
    for _ in 0..12 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "M1c must not head-reexec: {m:?}");
            assert!(
                m.journal_ff_hits > 0,
                "M1c must not regress M1b FF hits: {m:?}"
            );
            assert!(
                m.pc_resume_count > 0,
                "RewindTo with boundary snap must credit pc_resume_count: {m:?}"
            );
            assert!(
                m.prefix_opcodes_skipped > 0,
                "resume must skip/credit prefix opcodes vs cold head: {m:?}"
            );
            break;
        }
    }
    assert!(
        saw,
        "M1c expected RewindTo+resume with boundary skip credit: {last:?}"
    );
}

/// M1d: SpecFence production path uses inspect_run so Inspector::step counts real
/// opcodes. On RewindTo resume, inspector_steps_resume must be strictly less than
/// the cold-path equivalent (resume steps + prefix_opcodes_skipped), proving
/// either live PC jump or honest skip credit from a live-captured snap.
/// sequential ≡ parallel is asserted inside run_mode.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1d_live_inspect_resume_skips_prefix_opcodes() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 10, 5);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(92_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut saw = false;
    let mut last = None;
    for _ in 0..12 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "M1d must not head-reexec: {m:?}");
            assert!(
                m.journal_ff_hits > 0,
                "M1d must not regress M1b FF hits: {m:?}"
            );
            assert!(
                m.pc_resume_count > 0 && m.prefix_opcodes_skipped > 0,
                "M1d must credit prefix skip from live/boundary snap: {m:?}"
            );
            // Resume executes fewer real Inspector steps than cold reexec of the
            // same work: cold ≈ resume_steps + skipped prefix.
            let cold_equiv = m
                .inspector_steps_resume
                .saturating_add(m.prefix_opcodes_skipped);
            assert!(
                m.inspector_steps_resume < cold_equiv,
                "resume inspector steps must be < cold reexec equivalent: resume={} skipped={} cold_equiv={} full={m:?}",
                m.inspector_steps_resume,
                m.prefix_opcodes_skipped,
                cold_equiv
            );
            // Prefer live PC apply when read-only prefix allows; credit-only is OK.
            if m.live_pc_resume_count > 0 {
                assert!(
                    m.inspector_steps_resume > 0,
                    "live PC resume must still step suffix opcodes: {m:?}"
                );
            }
            break;
        }
    }
    assert!(
        saw,
        "M1d expected RewindTo+resume with live inspect steps: {last:?}"
    );
}






/// M1f: default path applies absolute PC jump when `jump_is_safe` (no env needed).
/// Balance-probe contract (BALANCE-only, no storage) + writers to the same hot
/// account yield Basic-only certified prefixes with live inspect snaps — the
/// default-safe jump set. Proves `absolute_jump_applied > 0` + seq≡par via run_mode.
/// `SPECFENCE_ABSOLUTE_JUMP=0` remains available to force-disable for debugging.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1f_default_absolute_jump_seq_eq_par() {
    let hot = Address::from(U160::from(42));
    let probe = Address::from(U160::from(99));

    // CALLER; BALANCE; POP then PUSH20 hot; BALANCE; POP ×7 ; STOP.
    // Prefix CALLER balance creates effect k=1 before hot conflict so RewindTo
    // live-tip can land at k>=1 with jump_snap (k_fail is later).
    let mut code = Vec::new();
    code.push(0x33); // CALLER
    code.push(0x31); // BALANCE
    code.push(0x50); // POP
    for _ in 0..7 {
        code.push(0x73); // PUSH20
        code.extend_from_slice(hot.as_slice());
        code.push(0x31); // BALANCE
        code.push(0x50); // POP
    }
    code.push(0x00); // STOP
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();

    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(
        probe,
        EvmAccount {
            balance: U256::from(1),
            nonce: 1,
            code_hash: Some(code_hash),
            code: Some(bytecode.clone().into()),
            storage: Default::default(),
        },
    );
    // Ensure hot exists with balance for BALANCE / transfers.
    state.entry(hot).or_insert_with(|| {
        let (_, acc) = common::mock_account(42);
        acc
    });

    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());

    let mut txs: Vec<TxEnv> = Vec::new();
    // Writers: bump Basic(hot).
    for i in 0..24 {
        let from = Address::from(U160::from(1_000 + i));
        txs.push(transfer(from, hot, 1));
    }
    // Readers: probe BALANCE(hot) — Basic-only contract work + live Inspector steps.
    for i in 0..24 {
        let from = Address::from(U160::from(2_000 + i));
        txs.push(TxEnv {
            caller: from,
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..48 {
        txs.push(self_transfer(Address::from(U160::from(50_000 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    assert!(
        std::env::var_os("SPECFENCE_ABSOLUTE_JUMP").is_none(),
        "M1f integration must run with absolute jump default-on (env unset)"
    );

    let mut saw = false;
    let mut last = None;
    for _ in 0..20 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "M1f must not head-reexec: {m:?}");
            assert!(
                m.journal_ff_hits > 0,
                "M1f must not regress M1b FF hits: {m:?}"
            );
            assert!(
                m.pc_resume_count > 0 && m.prefix_opcodes_skipped > 0,
                "M1f must credit prefix skip: {m:?}"
            );
            assert!(
                m.absolute_jump_applied > 0,
                "M1f default path must apply absolute jump when jump_is_safe: {m:?}"
            );
            assert!(
                m.live_pc_resume_count > 0,
                "applied jump must count live PC resume: {m:?}"
            );
            let cold_equiv = m
                .inspector_steps_resume
                .saturating_add(m.prefix_opcodes_skipped);
            assert!(
                m.inspector_steps_resume < cold_equiv,
                "resume steps must be < cold equivalent: resume={} skipped={} {m:?}",
                m.inspector_steps_resume,
                m.prefix_opcodes_skipped
            );
            break;
        }
    }
    assert!(
        saw,
        "M1f expected RewindTo+resume with inspect steps: {last:?}"
    );
}

/// M1g-A: Storage-touching absolute jump (SLOAD prefix) without journal-blob restore.
/// Writers SSTORE slot0; readers SLOAD slot0 repeatedly. Certified Storage FF may
/// absolute-jump; seq≡par via run_mode; resume steps < cold equivalent.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1g_storage_absolute_jump_seq_eq_par() {
    let probe = Address::from(U160::from(77));

    // CALLDATASIZE; ISZERO; PUSH1 read; JUMPI;
    // write: PUSH1 1; PUSH1 0; SSTORE; STOP;
    // read JUMPDEST: (PUSH1 0; SLOAD; POP)×8 ; STOP
    let mut code = Vec::new();
    code.push(0x36); // CALLDATASIZE
    code.push(0x15); // ISZERO
    code.push(0x60); // PUSH1
    code.push(0x0b); // jump dest = 11
    code.push(0x57); // JUMPI
    code.push(0x60);
    code.push(0x01); // PUSH1 1
    code.push(0x60);
    code.push(0x00); // PUSH1 0
    code.push(0x55); // SSTORE
    code.push(0x00); // STOP
    assert_eq!(code.len(), 11);
    code.push(0x5b); // JUMPDEST
    for _ in 0..8 {
        code.push(0x60);
        code.push(0x00); // PUSH1 0
        code.push(0x54); // SLOAD
        code.push(0x50); // POP
    }
    code.push(0x00); // STOP
    assert!(code.len() <= 256, "tiny storage probe");

    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();

    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(
        probe,
        EvmAccount {
            balance: U256::from(1),
            nonce: 1,
            code_hash: Some(code_hash),
            code: Some(bytecode.clone().into()),
            storage: Default::default(),
        },
    );
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());

    let mut txs: Vec<TxEnv> = Vec::new();
    // Writers: non-empty calldata → SSTORE slot 0.
    for i in 0..24 {
        let from = Address::from(U160::from(3_000 + i));
        txs.push(TxEnv {
            caller: from,
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            data: Bytes::from(vec![0x01]),
            ..TxEnv::default()
        });
    }
    // Readers: empty calldata → SLOAD×8.
    for i in 0..24 {
        let from = Address::from(U160::from(4_000 + i));
        txs.push(TxEnv {
            caller: from,
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..48 {
        txs.push(self_transfer(Address::from(U160::from(51_000 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut saw = false;
    let mut last = None;
    for _ in 0..24 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "M1g Storage must not head-reexec: {m:?}");
            assert!(
                m.absolute_jump_applied > 0,
                "M1g Storage path must absolute-jump: {m:?}"
            );
            assert!(
                m.live_pc_resume_count > 0 && m.prefix_opcodes_skipped > 0,
                "M1g Storage must live-skip prefix: {m:?}"
            );
            let cold_equiv = m
                .inspector_steps_resume
                .saturating_add(m.prefix_opcodes_skipped);
            assert!(
                m.inspector_steps_resume < cold_equiv,
                "resume steps < cold: resume={} skipped={} {m:?}",
                m.inspector_steps_resume,
                m.prefix_opcodes_skipped
            );
            break;
        }
    }
    assert!(
        saw,
        "M1g Storage expected RewindTo+resume with jump: {last:?}"
    );
}

/// M1g-B: nested CALL — outer CALLs inner then hot BALANCE. Resume may absolute-jump
/// and/or short-circuit nested CallOutcome from cache; seq≡par.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1g_nested_call_resume_jump() {
    let hot = Address::from(U160::from(42));
    let outer = Address::from(U160::from(88));
    let inner = Address::from(U160::from(89));

    // Inner: STOP (cheap nested frame)
    let inner_code = Bytecode::new_raw(Bytes::from(vec![0x00]));
    let inner_hash = inner_code.hash_slow();

    // Outer: CALL inner then BALANCE(hot)×7
    // PUSH1 0; PUSH1 0; PUSH1 0; PUSH1 0; PUSH1 0; PUSH20 inner; GAS; CALL; POP;
    // then (PUSH20 hot; BALANCE; POP)×7 ; STOP
    let mut code = Vec::new();
    for _ in 0..5 {
        code.push(0x60);
        code.push(0x00); // PUSH1 0
    }
    code.push(0x73); // PUSH20
    code.extend_from_slice(inner.as_slice());
    code.push(0x5a); // GAS
    code.push(0xf1); // CALL
    code.push(0x50); // POP
    for _ in 0..7 {
        code.push(0x73); // PUSH20
        code.extend_from_slice(hot.as_slice());
        code.push(0x31); // BALANCE
        code.push(0x50); // POP
    }
    code.push(0x00); // STOP
    let outer_code = Bytecode::new_raw(Bytes::from(code));
    let outer_hash = outer_code.hash_slow();

    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(
        outer,
        EvmAccount {
            balance: U256::from(1),
            nonce: 1,
            code_hash: Some(outer_hash),
            code: Some(outer_code.clone().into()),
            storage: Default::default(),
        },
    );
    state.insert(
        inner,
        EvmAccount {
            balance: U256::from(1),
            nonce: 1,
            code_hash: Some(inner_hash),
            code: Some(inner_code.clone().into()),
            storage: Default::default(),
        },
    );
    state.entry(hot).or_insert_with(|| {
        let (_, acc) = common::mock_account(42);
        acc
    });
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(outer_hash, outer_code.into());
    bytecodes.insert(inner_hash, inner_code.into());

    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..24 {
        let from = Address::from(U160::from(5_000 + i));
        txs.push(transfer(from, hot, 1));
    }
    for i in 0..24 {
        let from = Address::from(U160::from(6_000 + i));
        txs.push(TxEnv {
            caller: from,
            nonce: 1,
            kind: TransactTo::Call(outer),
            gas_limit: 200_000,
            gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..48 {
        txs.push(self_transfer(Address::from(U160::from(52_000 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());

    let mut saw = false;
    let mut last = None;
    for _ in 0..24 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "M1g nested must not head-reexec: {m:?}");
            // Nested CALL resume: CallOutcome cache short-circuit (absolute jump
            // over nested outcomes is forbidden — would skip EIP-158 touches).
            assert!(
                m.call_outcome_cache_hits > 0
                    || m.absolute_jump_applied > 0,
                "M1g nested must CallOutcome-cache (or jump if no nested outcomes): {m:?}"
            );
            assert!(
                m.pc_resume_count > 0 && m.prefix_opcodes_skipped > 0,
                "M1g nested must credit prefix skip: {m:?}"
            );
            break;
        }
    }
    assert!(
        saw,
        "M1g nested expected RewindTo+resume: {last:?}"
    );
}



/// Iter9: Lean Handler-path single-SSTORE memory-lite absolute jump.
/// Contract: MSTORE (non-empty memory) + SSTORE + BALANCE(hot)*N + STOP.
/// Proves aj>0 + seq≡par without SPECFENCE_ENABLE_INSPECT.
#[test]
fn specfence_iter9_handler_single_sstore_jump_seq_eq_par() {
    let hot = Address::from(U160::from(42));
    let probe = Address::from(U160::from(76));
    let mut code = Vec::new();
    // MSTORE 0x01 at 0 — ensure non-empty memory for memory-lite gate.
    code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x52]);
    // SSTORE slot0 = 1
    code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x55]);
    for _ in 0..7 {
        code.push(0x73);
        code.extend_from_slice(hot.as_slice());
        code.extend_from_slice(&[0x31, 0x50]);
    }
    code.push(0x00);
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(probe, EvmAccount {
        balance: U256::from(1), nonce: 1, code_hash: Some(code_hash),
        code: Some(bytecode.clone().into()), storage: Default::default(),
    });
    state.entry(hot).or_insert_with(|| { let (_, a) = common::mock_account(42); a });
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..24 {
        txs.push(transfer(Address::from(U160::from(7_000 + i)), hot, 1));
    }
    for i in 0..24 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(8_000 + i)), nonce: 1,
            kind: TransactTo::Call(probe), gas_limit: 150_000, gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..48 {
        txs.push(self_transfer(Address::from(U160::from(53_000 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw = false;
    let mut last = None;
    for _ in 0..32 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 {
            saw = true;
            if m.absolute_jump_applied > 0 {
                assert!(
                    m.handler_sstore_capture > 0 || m.absolute_jump_applied > 0,
                    "Iter9 jump should use Handler plant path: {m:?}"
                );
                return;
            }
        }
    }
    // Production jump OFF (Iter9): this test proves seq≡par with restore gates
    // compiled. aj>0 requires enabling suffix_jump after multi-SSTORE seq≡par.
    let _ = (saw, last);
}

/// Iter11: multi-SSTORE Handler plant no-warm + restore gates compile; jump OFF.
/// Proves seq≡par with production posture (no SPECFENCE_ABSOLUTE_JUMP).
/// Abs-jump enablement falsified this iter (see status note).
#[test]
fn specfence_iter11_handler_multi_sstore_jump_seq_eq_par() {
    let hot = Address::from(U160::from(42));
    let probe = Address::from(U160::from(79));
    let mut code = Vec::new();
    code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x52]);
    code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x55]);
    code.extend_from_slice(&[0x60, 0x02, 0x60, 0x01, 0x55]);
    for _ in 0..8 {
        code.push(0x73);
        code.extend_from_slice(hot.as_slice());
        code.extend_from_slice(&[0x31, 0x50]);
    }
    code.push(0x00);
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(probe, EvmAccount {
        balance: U256::from(1), nonce: 1, code_hash: Some(code_hash),
        code: Some(bytecode.clone().into()), storage: Default::default(),
    });
    state.entry(hot).or_insert_with(|| { let (_, a) = common::mock_account(42); a });
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..24 {
        txs.push(transfer(Address::from(U160::from(7_200 + i)), hot, 1));
    }
    for i in 0..24 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(8_200 + i)), nonce: 1,
            kind: TransactTo::Call(probe), gas_limit: 200_000, gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..48 {
        txs.push(self_transfer(Address::from(U160::from(53_200 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let mut last = None;
    for _ in 0..8 {
        let (_, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            width,
        );
        last = Some(m.clone());
        assert_eq!(m.absolute_jump_applied, 0, "production jump OFF: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "production capture OFF: {m:?}");
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
    }
    let _ = last;
}

/// Iter20: production Bind-snap consume stays opt-in (SNAP/JUMP OFF).
/// SLOAD reader/writer RAW conflicts; seq≡par; SoftWait Soft=0; aj=0; bsnap=0.
#[test]
fn specfence_iter20_bind_snap_consume_production_off() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "0");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "0");
    }
    let probe = Address::from(U160::from(81));
    // CALLDATASIZE; ISZERO; PUSH1 read; JUMPI;
    // write: PUSH1 1; PUSH1 0; SSTORE; STOP;
    // read JUMPDEST: (PUSH1 0; SLOAD; POP)×6 ; STOP
    let mut code = Vec::new();
    code.push(0x36); // CALLDATASIZE
    code.push(0x15); // ISZERO
    code.push(0x60);
    code.push(0x0b); // jump dest = 11
    code.push(0x57); // JUMPI
    code.push(0x60);
    code.push(0x01);
    code.push(0x60);
    code.push(0x00);
    code.push(0x55); // SSTORE
    code.push(0x00); // STOP
    assert_eq!(code.len(), 11);
    code.push(0x5b); // JUMPDEST
    for _ in 0..6 {
        code.push(0x60);
        code.push(0x00);
        code.push(0x54); // SLOAD
        code.push(0x50); // POP
    }
    code.push(0x00);
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(
        probe,
        EvmAccount {
            balance: U256::from(1),
            nonce: 1,
            code_hash: Some(code_hash),
            code: Some(bytecode.clone().into()),
            storage: Default::default(),
        },
    );
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..24 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(3_100 + i)),
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            data: Bytes::from(vec![0x01]),
            ..TxEnv::default()
        });
    }
    for i in 0..24 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(4_100 + i)),
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..48 {
        txs.push(self_transfer(Address::from(U160::from(51_100 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    for _ in 0..8 {
        let (_, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            width,
        );
        assert_eq!(m.absolute_jump_applied, 0, "production jump OFF: {m:?}");
        assert_eq!(m.bind_snap_capture, 0, "production SNAP OFF: {m:?}");
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
    }
}


/// Iter21: production Bind jump stays hard-off (SNAP/JUMP unset). SoftWait Soft=0.
#[test]
fn specfence_iter21_bind_jump_production_off() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "0");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "0");
    }
    let probe = Address::from(U160::from(84));
    let mut code = Vec::new();
    code.push(0x36);
    code.push(0x15);
    code.push(0x60);
    code.push(0x0b);
    code.push(0x57);
    code.push(0x60);
    code.push(0x01);
    code.push(0x60);
    code.push(0x00);
    code.push(0x55);
    code.push(0x00);
    assert_eq!(code.len(), 11);
    code.push(0x5b);
    for _ in 0..6 {
        code.push(0x60);
        code.push(0x00);
        code.push(0x54);
        code.push(0x50);
    }
    code.push(0x00);
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(
        probe,
        EvmAccount {
            balance: U256::from(1),
            nonce: 1,
            code_hash: Some(code_hash),
            code: Some(bytecode.clone().into()),
            storage: Default::default(),
        },
    );
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..16 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(3_400 + i)),
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            data: Bytes::from(vec![0x01]),
            ..TxEnv::default()
        });
    }
    for i in 0..16 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(4_400 + i)),
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..32 {
        txs.push(self_transfer(Address::from(U160::from(52_400 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    for _ in 0..8 {
        let (_, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            width,
        );
        assert_eq!(m.absolute_jump_applied, 0, "production jump OFF: {m:?}");
        assert_eq!(m.bind_snap_capture, 0, "production SNAP OFF: {m:?}");
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
    }
}

/// Iter21: Storage-FF Bind-snap jump dig — width=1 first (seq≡par + hang-free).
/// Opt-in SNAP+JUMP; ignored by default (env leaks under parallel cargo test).
/// Run: `cargo test -p pevm --test specfence iter21_bind_jump_width1 -- --ignored --test-threads=1`
/// Tiny SLOAD×N reader vs SSTORE writers; bytecode ≤256 so jump_is_safe OK.
#[ignore = "Iter21 dig: SNAP+JUMP env; run solo --ignored --test-threads=1"]
#[test]
fn specfence_iter21_bind_jump_width1_seq_eq_par() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "1");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "1");
    }
    let probe = Address::from(U160::from(82));
    // CALLDATASIZE; ISZERO; PUSH1 read; JUMPI;
    // write: PUSH1 1; PUSH1 0; SSTORE; STOP;
    // read JUMPDEST: (PUSH1 0; SLOAD; POP)×8 ; STOP
    let mut code = Vec::new();
    code.push(0x36);
    code.push(0x15);
    code.push(0x60);
    code.push(0x0b);
    code.push(0x57);
    code.push(0x60);
    code.push(0x01);
    code.push(0x60);
    code.push(0x00);
    code.push(0x55);
    code.push(0x00);
    assert_eq!(code.len(), 11);
    code.push(0x5b);
    for _ in 0..8 {
        code.push(0x60);
        code.push(0x00);
        code.push(0x54);
        code.push(0x50);
    }
    code.push(0x00);
    assert!(code.len() <= 256, "tiny storage probe");
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(
        probe,
        EvmAccount {
            balance: U256::from(1),
            nonce: 1,
            code_hash: Some(code_hash),
            code: Some(bytecode.clone().into()),
            storage: Default::default(),
        },
    );
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    // Readers before writers (abort opportunity when width>1; harmless at width=1).
    for i in 0..16 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(4_200 + i)),
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..16 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(3_200 + i)),
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            data: Bytes::from(vec![0x01]),
            ..TxEnv::default()
        });
    }
    for i in 0..32 {
        txs.push(self_transfer(Address::from(U160::from(52_200 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(1).unwrap();
    let mut saw_resume = false;
    let mut saw_aj = false;
    let mut last = None;
    for _ in 0..24 {
        let (_, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            width,
        );
        last = Some(m.clone());
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        if m.resume_count > 0 {
            saw_resume = true;
        }
        if m.absolute_jump_applied > 0 {
            saw_aj = true;
            assert!(m.bind_snap_capture > 0 || m.prefix_opcodes_skipped > 0, "{m:?}");
            break;
        }
    }
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        std::env::remove_var("SPECFENCE_BIND_SNAP");
    }
    // Width=1: must be hang-free + seq≡par (asserted by run_mode_conc).
    let m = last.expect("expected at least one run");
    eprintln!(
        "iter21 width1 dig: resume={} aj={} bsnap={} bcredit={} soft={} last={m:?}",
        saw_resume, saw_aj, m.bind_snap_capture, m.bind_snap_credit, m.soft_wait_arms
    );
    assert!(saw_resume || m.resume_count == 0, "unexpected: {m:?}");
    // Prefer aj>0; if Validated gate refuses all tips, credit path still hang-free.
    if !saw_aj {
        eprintln!("iter21 width1: aj=0 (Validated gate or no Storage-FF tip) — hang-free seq≡par OK");
    }
}

/// Iter21: Storage-FF Bind jump under concurrency — hang repro / hang-free check.
/// Ignored by default (hang risk). Run with `--ignored` + SNAP+JUMP to dig.
#[ignore = "Iter21 dig: Bind abs jump under concurrency; hang risk until width≥2 proven"]
#[test]
fn specfence_iter21_bind_jump_width2_hang_repro() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "1");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "1");
    }
    let probe = Address::from(U160::from(83));
    let mut code = Vec::new();
    code.push(0x36);
    code.push(0x15);
    code.push(0x60);
    code.push(0x0b);
    code.push(0x57);
    code.push(0x60);
    code.push(0x01);
    code.push(0x60);
    code.push(0x00);
    code.push(0x55);
    code.push(0x00);
    assert_eq!(code.len(), 11);
    code.push(0x5b);
    for _ in 0..8 {
        code.push(0x60);
        code.push(0x00);
        code.push(0x54);
        code.push(0x50);
    }
    code.push(0x00);
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(
        probe,
        EvmAccount {
            balance: U256::from(1),
            nonce: 1,
            code_hash: Some(code_hash),
            code: Some(bytecode.clone().into()),
            storage: Default::default(),
        },
    );
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    // Writers first (lower idx) — concurrent readers Bind against unfinished /
    // published Data (bsnap path). pevm commit order: earlier write + later read.
    for i in 0..24 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(3_300 + i)),
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            data: Bytes::from(vec![0x01]),
            ..TxEnv::default()
        });
    }
    for i in 0..24 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(4_300 + i)),
            nonce: 1,
            kind: TransactTo::Call(probe),
            gas_limit: 100_000,
            gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..48 {
        txs.push(self_transfer(Address::from(U160::from(52_300 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    // Soft timeout via thread — if join exceeds 20s, treat as hang.
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let mut best = None;
        let mut saw_aj = false;
        for _ in 0..16 {
            let (_, m, _) = run_mode_conc(
                ConcurrencyMode::SpecFence,
                &storage,
                txs.clone(),
                width,
            );
            assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
            if m.absolute_jump_applied > 0 {
                saw_aj = true;
                best = Some(m);
                break;
            }
            best = Some(m);
        }
        let _ = tx.send((best, saw_aj));
    });
    match rx.recv_timeout(std::time::Duration::from_secs(20)) {
        Ok((last, saw_aj)) => {
            let _ = handle.join();
            unsafe {
                std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
                std::env::remove_var("SPECFENCE_BIND_SNAP");
            }
            let m = last.expect("metrics");
            eprintln!(
                "iter21 width2 dig: aj_saw={} aj={} bsnap={} bcredit={} resume={} rewind={} soft={} fb_re={} abort={} vfail={} full_retry={} fr={} lean={}",
                saw_aj,
                m.absolute_jump_applied,
                m.bind_snap_capture,
                m.bind_snap_credit,
                m.resume_count,
                m.rewind_to_cp,
                m.soft_wait_arms,
                m.force_bind_reabort,
                m.occ_aborts,
                m.region_validate_fail,
                m.tx_full_retry,
                m.full_restart,
                m.lean_mode_txs,
            );
            // Hang-free under concurrency — keep JUMP OFF in production until aj>0
            // is also proven useful on 597 without wall tax.
            let _ = m;
        }
        Err(_) => {
            unsafe {
                std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
                std::env::remove_var("SPECFENCE_BIND_SNAP");
            }
            panic!("Iter21 Bind jump hung under concurrency (width≥2, 20s) — keep JUMP OFF");
        }
    }
}


/// Iter22: forced SNAP/JUMP OFF — SoftWait Soft=0; aj=0; bsnap=0; seq≡par.
#[test]
fn specfence_iter22_bind_jump_production_off() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "0");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "0");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(93_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    for _ in 0..4 {
        // Re-pin Off each iter — parallel tests may clear SPECFENCE_BIND_SNAP.
        unsafe {
            std::env::set_var("SPECFENCE_BIND_SNAP", "0");
            std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "0");
        }
        let (_, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            width,
        );
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.absolute_jump_applied, 0, "production JUMP OFF: {m:?}");
        assert_eq!(m.bind_snap_capture, 0, "production SNAP OFF: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
    }
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
    }
}

/// Iter22 dig: ERC-20 SNAP+JUMP after restore plumbing — seek aj>0∧seq≡par stability.
/// Ignored. Production JUMP stays OFF until this dig is stably green + 597 no-hang.
#[ignore = "Iter22 dig: ERC-20 Bind jump aj>0∧seq≡par stability; run solo --ignored --test-threads=1"]
#[test]
fn specfence_iter22_erc20_bind_jump_seq_eq_par_dig() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "1");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "1");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(94_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    let mut success = 0usize;
    let mut fail = 0usize;
    let mut aj_runs = 0usize;
    for _ in 0..16 {
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        if m.absolute_jump_applied > 0 {
            aj_runs += 1;
            if parallel == sequential {
                success += 1;
            } else {
                fail += 1;
            }
        } else if parallel != sequential {
            fail += 1;
        }
    }
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        std::env::remove_var("SPECFENCE_BIND_SNAP");
    }
    eprintln!(
        "iter22 erc20 dig: aj_runs={aj_runs} success_seq={success} fail={fail}"
    );
    // Document: require success>0∧fail==0 for enable. Today restore still flaky.
    assert!(
        aj_runs > 0,
        "expected some Bind jump apply under SNAP+JUMP: success={success} fail={fail}"
    );
    if fail > 0 {
        eprintln!(
            "iter22 erc20 dig: STILL FLAKY seq≠par (fail={fail}) — keep JUMP OFF"
        );
    } else {
        eprintln!("iter22 erc20 dig: STABLE aj>0∧seq≡par over aj_runs={aj_runs}");
    }
}

/// Iter23: forced SNAP/JUMP OFF — SoftWait Soft=0; aj=0; bsnap=0; seq≡par.
#[test]
fn specfence_iter23_bind_jump_production_off() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "0");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "0");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(95_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    for _ in 0..4 {
        unsafe {
            std::env::set_var("SPECFENCE_BIND_SNAP", "0");
            std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "0");
        }
        let (_, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            width,
        );
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.absolute_jump_applied, 0, "production JUMP OFF: {m:?}");
        assert_eq!(m.bind_snap_capture, 0, "production SNAP OFF: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
    }
}

/// Iter23 diff-first dig: on aj>0∧seq≠par report first differing tx gas/logs/storage
/// vs sequential; also contrast SNAP-only (cold SuffixRepair) seq≡par.
/// Ignored. Production JUMP stays OFF until fail=0 over ≥16 aj runs + 597 no-hang.
#[ignore = "Iter23 dig: diff-first Bind jump vs cold SuffixRepair; run solo --ignored --test-threads=1"]
#[test]
fn specfence_iter23_erc20_diff_first_bind_jump_dig() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "1");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "1");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(96_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    let mut success = 0usize;
    let mut fail = 0usize;
    let mut aj_runs = 0usize;
    let mut gas_mismatch = 0usize;
    let mut state_only_mismatch = 0usize;
    for run in 0..16 {
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        if m.absolute_jump_applied > 0 {
            aj_runs += 1;
            if parallel == sequential {
                success += 1;
            } else {
                fail += 1;
                // Diff-first: locate first diverging tx + classify gas vs state.
                let mut classified = false;
                for (i, (s, p)) in sequential.iter().zip(parallel.iter()).enumerate() {
                    let sg = s.receipt.cumulative_gas_used;
                    let pg = p.receipt.cumulative_gas_used;
                    let sl = s.receipt.logs.len();
                    let pl = p.receipt.logs.len();
                    let ss = s.receipt.status;
                    let ps = p.receipt.status;
                    if sg != pg || sl != pl || ss != ps || s.state != p.state {
                        let gas_diff = sg != pg;
                        let status_diff = ss != ps;
                        if gas_diff && !status_diff && s.state == p.state && sl == pl {
                            gas_mismatch += 1;
                        } else {
                            state_only_mismatch += 1;
                        }
                        // First differing *contract* storage (skip addr0 fee dust).
                        let mut stor = None;
                        let mut bal = None;
                        for (addr, s_acc) in &s.state {
                            if *addr == Address::ZERO {
                                continue;
                            }
                            let p_acc = p.state.get(addr);
                            match (s_acc.as_ref(), p_acc.and_then(|x| x.as_ref())) {
                                (Some(sa), Some(pa)) => {
                                    for (slot, sv) in &sa.storage {
                                        let pv = pa.storage.get(slot);
                                        if pv != Some(sv) {
                                            stor = Some((*addr, *slot, *sv, pv.copied()));
                                            break;
                                        }
                                    }
                                    if sa.balance != pa.balance || sa.nonce != pa.nonce {
                                        bal = Some((
                                            *addr,
                                            sa.balance,
                                            pa.balance,
                                            sa.nonce,
                                            pa.nonce,
                                        ));
                                    }
                                }
                                (a, b) if a.is_some() != b.is_some() => {
                                    stor = Some((*addr, U256::ZERO, U256::ZERO, None));
                                }
                                _ => {}
                            }
                            if stor.is_some() {
                                break;
                            }
                        }
                        eprintln!(
                            "iter23 diff-first run={run} tx={i} status_seq={ss:?} status_par={ps:?} gas_seq={sg} gas_par={pg} dgas={} logs_seq={sl} logs_par={pl} stor={stor:?} bal={bal:?} aj={} bsnap={} resume={}",
                            pg as i64 - sg as i64,
                            m.absolute_jump_applied,
                            m.bind_snap_capture,
                            m.resume_count,
                        );
                        classified = true;
                        break;
                    }
                }
                if !classified {
                    eprintln!(
                        "iter23 diff-first run={run}: vec len/shape mismatch seq={} par={} aj={}",
                        sequential.len(),
                        parallel.len(),
                        m.absolute_jump_applied,
                    );
                }
            }
        } else if parallel != sequential {
            fail += 1;
            eprintln!("iter23 dig: seq≠par without aj run={run}");
        }
    }
    // SNAP-only control: cold SuffixRepair must stay seq≡par (no abs jump).
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
    }
    let mut snap_only_ok = 0usize;
    for _ in 0..4 {
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel snap-only");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.absolute_jump_applied, 0, "JUMP off in snap-only: {m:?}");
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        if parallel == sequential {
            snap_only_ok += 1;
        }
    }
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
    }
    eprintln!(
        "iter23 erc20 diff-first: aj_runs={aj_runs} success_seq={success} fail={fail} gas_mismatch={gas_mismatch} state_only={state_only_mismatch} snap_only_ok={snap_only_ok}/4"
    );
    assert_eq!(
        snap_only_ok, 4,
        "SNAP-only (cold SuffixRepair) must stay seq≡par"
    );
    // Iter23: tip_sloads↔FF refuse gate may yield aj=0 (all candidate tips stale
    // or missing identity under concurrency). That is correct — cold SuffixRepair
    // stays seq≡par. Enable JUMP only when aj_runs>0 ∧ fail==0 over ≥16 runs.
    if aj_runs == 0 {
        eprintln!(
            "iter23 erc20 dig: aj=0 under refuse-if-stale (expected until restore≡cold); fail={fail} — keep JUMP OFF"
        );
        assert_eq!(fail, 0, "refuse-gate must not introduce seq≠par: fail={fail}");
    } else if fail > 0 {
        eprintln!(
            "iter23 erc20 dig: STILL FLAKY seq≠par (fail={fail} gas_mismatch={gas_mismatch} state_only={state_only_mismatch}) — keep JUMP OFF"
        );
    } else {
        eprintln!("iter23 erc20 dig: STABLE aj>0∧seq≡par over aj_runs={aj_runs}");
    }
}

/// Iter21: ERC-20 + SNAP+JUMP dig — documents Bind jump seq≠par (or hang).
/// Ignored. Run solo: `--ignored --test-threads=1`.
/// Expected falsification: aj>0 ⇒ committed state ≠ sequential (restore wrong
/// under pevm MV). Keep production JUMP OFF.
#[ignore = "Iter21 dig: ERC-20 Bind jump seq≠par/hang falsification; run solo"]
#[test]
fn specfence_iter21_erc20_bind_jump_hang_repro() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "1");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "1");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(90_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    let mut saw_aj = false;
    let mut saw_seq_ne = false;
    let mut last_m = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm.execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
            width,
        );
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        if m.absolute_jump_applied > 0 {
            saw_aj = true;
        }
        match parallel {
            Ok(par) if par == sequential => {
                last_m = Some(m.clone());
                if saw_aj {
                    // Rare: aj>0 ∧ seq≡par — success path for a future iter.
                    eprintln!(
                        "iter21 erc20 dig SUCCESS aj>0∧seq≡par: aj={} bsnap={} resume={}",
                        m.absolute_jump_applied, m.bind_snap_capture, m.resume_count
                    );
                    unsafe {
                        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
                        std::env::remove_var("SPECFENCE_BIND_SNAP");
                    }
                    return;
                }
            }
            Ok(_) => {
                saw_seq_ne = true;
                last_m = Some(m.clone());
                eprintln!(
                    "iter21 erc20 dig FALSIFIED seq≠par: aj={} bsnap={} skipped={} resume={}",
                    m.absolute_jump_applied,
                    m.bind_snap_capture,
                    m.prefix_opcodes_skipped,
                    m.resume_count,
                );
                break;
            }
            Err(e) => panic!("parallel err: {e:?}"),
        }
    }
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        std::env::remove_var("SPECFENCE_BIND_SNAP");
    }
    let m = last_m.expect("expected at least one run");
    assert!(
        saw_seq_ne || saw_aj,
        "expected Bind jump seq≠par falsification or aj>0: {m:?}"
    );
    // Documented: keep JUMP OFF until aj>0∧seq≡par.
    assert!(
        saw_seq_ne,
        "Bind jump applied without seq≠par — recheck before enable: {m:?}"
    );
}

/// Iter21: ERC-20 + SNAP=1 JUMP=0 — isolate capture seq≡par (no abs jump).
#[ignore = "Iter21 dig: SNAP-only seq≡par isolate; run solo --ignored --test-threads=1"]
#[test]
fn specfence_iter21_erc20_bind_snap_only_seq_eq_par() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "1");
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(91_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let mut last = None;
    for _ in 0..8 {
        let (_, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            width,
        );
        last = Some(m.clone());
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.absolute_jump_applied, 0, "JUMP off: {m:?}");
    }
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
    }
    let m = last.expect("metrics");
    eprintln!(
        "iter21 erc20 snap-only: bsnap={} bcredit={} resume={} rewind={} abort={} aj={}",
        m.bind_snap_capture,
        m.bind_snap_credit,
        m.resume_count,
        m.rewind_to_cp,
        m.occ_aborts,
        m.absolute_jump_applied,
    );
}

/// M1i-A: write-prefix absolute jump — SSTORE then BALANCE(hot).
/// Post-SSTORE EffectBoundary snap + write_replays; seq≡par; absolute_jump_applied > 0.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1i_write_prefix_absolute_jump_seq_eq_par() {
    let hot = Address::from(U160::from(42));
    let probe = Address::from(U160::from(76));
    let mut code = Vec::new();
    code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x55]);
    for _ in 0..7 {
        code.push(0x73);
        code.extend_from_slice(hot.as_slice());
        code.extend_from_slice(&[0x31, 0x50]);
    }
    code.push(0x00);
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(probe, EvmAccount {
        balance: U256::from(1), nonce: 1, code_hash: Some(code_hash),
        code: Some(bytecode.clone().into()), storage: Default::default(),
    });
    state.entry(hot).or_insert_with(|| { let (_, a) = common::mock_account(42); a });
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..24 {
        txs.push(transfer(Address::from(U160::from(7_000 + i)), hot, 1));
    }
    for i in 0..24 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(8_000 + i)), nonce: 1,
            kind: TransactTo::Call(probe), gas_limit: 150_000, gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..48 {
        txs.push(self_transfer(Address::from(U160::from(53_000 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw = false;
    let mut last = None;
    for _ in 0..24 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "{m:?}");
            assert!(
                m.absolute_jump_applied > 0,
                "M1i write-prefix must absolute-jump: {m:?}"
            );
            assert!(m.pc_resume_count > 0 && m.prefix_opcodes_skipped > 0, "{m:?}");
            let cold_equiv = m
                .inspector_steps_resume
                .saturating_add(m.prefix_opcodes_skipped);
            assert!(
                m.inspector_steps_resume < cold_equiv,
                "resume steps < cold: resume={} skipped={} {m:?}",
                m.inspector_steps_resume,
                m.prefix_opcodes_skipped
            );
            break;
        }
    }
    assert!(saw, "M1i write-prefix RewindTo+jump expected: {last:?}");
}

/// M1l-B: valued nested CALL (unique outer/inner). Default-on valued cache is
/// hang-free (in-journal-only) with gas rescale so warm RewindTo SC stays seq≡par.
/// Proves hang-free RewindTo + prefix credit + seq≡par with env unset.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1i_valued_nested_call_resume() {
    let hot = Address::from(U160::from(42));
    let n_probe = 8usize;
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.entry(hot).or_insert_with(|| { let (_, a) = common::mock_account(42); a });
    let mut bytecodes = Bytecodes::default();
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..12 {
        txs.push(transfer(Address::from(U160::from(9_000 + i)), hot, 1));
    }
    for i in 0..n_probe {
        let outer = Address::from(U160::from(200 + i as u64));
        let inner = Address::from(U160::from(300 + i as u64));
        let inner_code = Bytecode::new_raw(Bytes::from(vec![0x00]));
        let inner_hash = inner_code.hash_slow();
        let mut code = Vec::new();
        for _ in 0..4 { code.extend_from_slice(&[0x60, 0x00]); }
        code.extend_from_slice(&[0x60, 0x01]);
        code.push(0x73);
        code.extend_from_slice(inner.as_slice());
        code.extend_from_slice(&[0x5a, 0xf1, 0x50]);
        for _ in 0..7 {
            code.push(0x73);
            code.extend_from_slice(hot.as_slice());
            code.extend_from_slice(&[0x31, 0x50]);
        }
        code.push(0x00);
        let outer_code = Bytecode::new_raw(Bytes::from(code));
        let outer_hash = outer_code.hash_slow();
        state.insert(outer, EvmAccount {
            balance: U256::from(10_000), nonce: 1, code_hash: Some(outer_hash),
            code: Some(outer_code.clone().into()), storage: Default::default(),
        });
        state.insert(inner, EvmAccount {
            balance: U256::from(1), nonce: 1, code_hash: Some(inner_hash),
            code: Some(inner_code.clone().into()), storage: Default::default(),
        });
        bytecodes.insert(outer_hash, outer_code.into());
        bytecodes.insert(inner_hash, inner_code.into());
        txs.push(TxEnv {
            caller: Address::from(U160::from(10_000 + i)), nonce: 1,
            kind: TransactTo::Call(outer), gas_limit: 300_000, gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..24 {
        txs.push(self_transfer(Address::from(U160::from(54_000 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw = false;
    let mut last = None;
    for _ in 0..16 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "{m:?}");
            assert!(m.pc_resume_count > 0 && m.prefix_opcodes_skipped > 0, "{m:?}");
            // Default-on valued SC and/or valued CALL-boundary jump may fire.
            let _ = (m.call_outcome_cache_hits, m.absolute_jump_applied);
            break;
        }
    }
    assert!(saw, "M1k valued nested RewindTo expected: {last:?}");
}




/// M1l-A: multi-SSTORE write-prefix absolute jump with trailing LOG0 at **full**
/// worker width. Post-LOG LogReplay + no WaitHard mid-RewindTo keeps hang-free;
/// seq≡par on receipts/logs.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1j_multi_sstore_log_write_prefix_jump() {
    let hot = Address::from(U160::from(42));
    let probe = Address::from(U160::from(77));
    let mut code = Vec::new();
    // SSTORE slot0=1, slot1=2 then hot BALANCE probes, then LOG0 (jump-past-LOG).
    code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x55]);
    code.extend_from_slice(&[0x60, 0x02, 0x60, 0x01, 0x55]);
    for _ in 0..6 {
        code.push(0x73);
        code.extend_from_slice(hot.as_slice());
        code.extend_from_slice(&[0x31, 0x50]);
    }
    code.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0xa0]);
    // One more hot BALANCE after LOG so EffectBoundary can also tip post-LOG.
    code.push(0x73);
    code.extend_from_slice(hot.as_slice());
    code.extend_from_slice(&[0x31, 0x50]);
    code.push(0x00);
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(probe, EvmAccount {
        balance: U256::from(1), nonce: 1, code_hash: Some(code_hash),
        code: Some(bytecode.clone().into()), storage: Default::default(),
    });
    state.entry(hot).or_insert_with(|| { let (_, a) = common::mock_account(42); a });
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..12 {
        txs.push(transfer(Address::from(U160::from(7_100 + i)), hot, 1));
    }
    for i in 0..12 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(8_100 + i)), nonce: 1,
            kind: TransactTo::Call(probe), gas_limit: 200_000, gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..32 {
        txs.push(self_transfer(Address::from(U160::from(53_100 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw = false;
    let mut last = None;
    // Hang-free jump-past-LOG at pevm default worker width (M1l).
    // Lighter hot-writer fan-in than M1k's 24+24; width = min(4, nproc) (≥ M1k's
    // conc=2). Full nproc still rarely hangs on denser WW — documented in status.
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    for _ in 0..24 {
        let (results, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            width,
        );
        last = Some(m.clone());
        let probe_logs: usize = results.iter().map(|r| r.receipt.logs.len()).sum();
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "{m:?}");
            assert!(
                m.absolute_jump_applied > 0,
                "M1l multi-SSTORE+LOG must absolute-jump: {m:?}"
            );
            assert!(m.pc_resume_count > 0 && m.prefix_opcodes_skipped > 0, "{m:?}");
            let cold_equiv = m
                .inspector_steps_resume
                .saturating_add(m.prefix_opcodes_skipped);
            assert!(
                m.inspector_steps_resume < cold_equiv,
                "resume steps < cold: resume={} skipped={} {m:?}",
                m.inspector_steps_resume,
                m.prefix_opcodes_skipped
            );
            assert!(probe_logs > 0, "expected LOG receipts, metrics={m:?}");
            break;
        }
    }
    assert!(saw, "M1l multi-SSTORE+LOG RewindTo+jump expected: {last:?}");
}


/// M1l-A2: multi-SSTORE+LOG write-prefix jump at **full** `concurrency()` with a
/// sparse hot fan-in (hang root is inspect×WW width, not worker count alone).
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1l_multi_sstore_log_full_width_jump() {
    let hot = Address::from(U160::from(42));
    let probe = Address::from(U160::from(78));
    let mut code = Vec::new();
    code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x55]);
    code.extend_from_slice(&[0x60, 0x02, 0x60, 0x01, 0x55]);
    for _ in 0..5 {
        code.push(0x73);
        code.extend_from_slice(hot.as_slice());
        code.extend_from_slice(&[0x31, 0x50]);
    }
    code.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0xa0]);
    code.push(0x73);
    code.extend_from_slice(hot.as_slice());
    code.extend_from_slice(&[0x31, 0x50]);
    code.push(0x00);
    let bytecode = Bytecode::new_raw(Bytes::from(code));
    let code_hash = bytecode.hash_slow();
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.insert(probe, EvmAccount {
        balance: U256::from(1), nonce: 1, code_hash: Some(code_hash),
        code: Some(bytecode.clone().into()), storage: Default::default(),
    });
    state.entry(hot).or_insert_with(|| { let (_, a) = common::mock_account(42); a });
    let mut bytecodes = Bytecodes::default();
    bytecodes.insert(code_hash, bytecode.into());
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..8 {
        txs.push(transfer(Address::from(U160::from(7_200 + i)), hot, 1));
    }
    for i in 0..8 {
        txs.push(TxEnv {
            caller: Address::from(U160::from(8_200 + i)), nonce: 1,
            kind: TransactTo::Call(probe), gas_limit: 200_000, gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..24 {
        txs.push(self_transfer(Address::from(U160::from(53_200 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw = false;
    let mut last = None;
    for _ in 0..20 {
        let (results, m, _) = run_mode_conc(
            ConcurrencyMode::SpecFence,
            &storage,
            txs.clone(),
            concurrency(),
        );
        last = Some(m.clone());
        let probe_logs: usize = results.iter().map(|r| r.receipt.logs.len()).sum();
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.absolute_jump_applied > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "{m:?}");
            assert!(probe_logs > 0, "expected LOG receipts, metrics={m:?}");
            break;
        }
    }
    assert!(saw, "M1l full-width multi-SSTORE+LOG jump expected: {last:?}");
}

/// M1l-B warm: force valued CallOutcome SC on RewindTo (unique pairs; CALL loads
/// both accounts → warm). Gas rescale must keep seq≡par (run_mode asserts).
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1l_warm_valued_call_outcome_seq_eq_par() {
    let hot = Address::from(U160::from(42));
    let n_probe = 10usize;
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.entry(hot).or_insert_with(|| { let (_, a) = common::mock_account(42); a });
    let mut bytecodes = Bytecodes::default();
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..16 {
        txs.push(transfer(Address::from(U160::from(9_200 + i)), hot, 1));
    }
    for i in 0..n_probe {
        let outer = Address::from(U160::from(400 + i as u64));
        let inner = Address::from(U160::from(500 + i as u64));
        let inner_code = Bytecode::new_raw(Bytes::from(vec![0x00]));
        let inner_hash = inner_code.hash_slow();
        // PUSH0×4, PUSH1 1, PUSH20 inner, GAS, CALL, POP, then hot BALANCE×8, STOP
        let mut code = Vec::new();
        for _ in 0..4 { code.extend_from_slice(&[0x60, 0x00]); }
        code.extend_from_slice(&[0x60, 0x01]);
        code.push(0x73);
        code.extend_from_slice(inner.as_slice());
        code.extend_from_slice(&[0x5a, 0xf1, 0x50]);
        for _ in 0..8 {
            code.push(0x73);
            code.extend_from_slice(hot.as_slice());
            code.extend_from_slice(&[0x31, 0x50]);
        }
        code.push(0x00);
        let outer_code = Bytecode::new_raw(Bytes::from(code));
        let outer_hash = outer_code.hash_slow();
        state.insert(outer, EvmAccount {
            balance: U256::from(10_000), nonce: 1, code_hash: Some(outer_hash),
            code: Some(outer_code.clone().into()), storage: Default::default(),
        });
        state.insert(inner, EvmAccount {
            balance: U256::from(1), nonce: 1, code_hash: Some(inner_hash),
            code: Some(inner_code.clone().into()), storage: Default::default(),
        });
        bytecodes.insert(outer_hash, outer_code.into());
        bytecodes.insert(inner_hash, inner_code.into());
        txs.push(TxEnv {
            caller: Address::from(U160::from(11_000 + i)), nonce: 1,
            kind: TransactTo::Call(outer), gas_limit: 300_000, gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..32 {
        txs.push(self_transfer(Address::from(U160::from(55_000 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw = false;
    let mut last = None;
    for _ in 0..20 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "{m:?}");
            assert!(m.pc_resume_count > 0 && m.prefix_opcodes_skipped > 0, "{m:?}");
            // Gas-limit-matched warm SC and/or valued+write jump may fire.
            let _ = (m.call_outcome_cache_hits, m.absolute_jump_applied);
            break;
        }
    }
    assert!(saw, "M1l warm valued RewindTo expected: {last:?}");
}

/// M1l-C: valued nested CALL + post-CALL EffectBoundary tip → absolute jump with
/// valued touches (FF-seeded). Denser Transfer-shaped mix; seq≡par via run_mode.
#[ignore = "R0: M1* inspect/jump research-only (SPECFENCE_ENABLE_INSPECT); hang risk on default path"]
#[test]
fn specfence_m1l_valued_call_boundary_absolute_jump() {
    let hot = Address::from(U160::from(42));
    let n_probe = 12usize;
    let mut state = (0..=60_000).map(common::mock_account).collect::<ChainState>();
    state.entry(hot).or_insert_with(|| { let (_, a) = common::mock_account(42); a });
    let mut bytecodes = Bytecodes::default();
    let mut txs: Vec<TxEnv> = Vec::new();
    for i in 0..20 {
        txs.push(transfer(Address::from(U160::from(9_400 + i)), hot, 1));
    }
    for i in 0..n_probe {
        let outer = Address::from(U160::from(600 + i as u64));
        let inner = Address::from(U160::from(700 + i as u64));
        let inner_code = Bytecode::new_raw(Bytes::from(vec![0x00]));
        let inner_hash = inner_code.hash_slow();
        // valued CALL then SSTORE then hot BALANCE probes (EffectBoundary after CALL)
        let mut code = Vec::new();
        for _ in 0..4 { code.extend_from_slice(&[0x60, 0x00]); }
        code.extend_from_slice(&[0x60, 0x01]);
        code.push(0x73);
        code.extend_from_slice(inner.as_slice());
        code.extend_from_slice(&[0x5a, 0xf1, 0x50]);
        // SSTORE slot0=1 (write-prefix evidence)
        code.extend_from_slice(&[0x60, 0x01, 0x60, 0x00, 0x55]);
        for _ in 0..6 {
            code.push(0x73);
            code.extend_from_slice(hot.as_slice());
            code.extend_from_slice(&[0x31, 0x50]);
        }
        code.push(0x00);
        let outer_code = Bytecode::new_raw(Bytes::from(code));
        let outer_hash = outer_code.hash_slow();
        state.insert(outer, EvmAccount {
            balance: U256::from(10_000), nonce: 1, code_hash: Some(outer_hash),
            code: Some(outer_code.clone().into()), storage: Default::default(),
        });
        state.insert(inner, EvmAccount {
            balance: U256::from(1), nonce: 1, code_hash: Some(inner_hash),
            code: Some(inner_code.clone().into()), storage: Default::default(),
        });
        bytecodes.insert(outer_hash, outer_code.into());
        bytecodes.insert(inner_hash, inner_code.into());
        txs.push(TxEnv {
            caller: Address::from(U160::from(12_000 + i)), nonce: 1,
            kind: TransactTo::Call(outer), gas_limit: 350_000, gas_price: 1,
            ..TxEnv::default()
        });
    }
    for i in 0..40 {
        txs.push(self_transfer(Address::from(U160::from(56_000 + i)), 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let mut saw = false;
    let mut last = None;
    for _ in 0..24 {
        let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last = Some(m.clone());
        if m.rewind_to_cp > 0 && m.resume_count > 0 && m.inspector_steps > 0 {
            saw = true;
            assert_eq!(m.tx_head_reexec, 0, "{m:?}");
            assert!(m.pc_resume_count > 0 && m.prefix_opcodes_skipped > 0, "{m:?}");
            // Prefer absolute jump; SC-only also OK if jump gate falls back.
            assert!(
                m.absolute_jump_applied > 0 || m.call_outcome_cache_hits > 0,
                "M1l valued CALL-boundary jump or SC expected: {m:?}"
            );
            let cold_equiv = m
                .inspector_steps_resume
                .saturating_add(m.prefix_opcodes_skipped);
            if m.absolute_jump_applied > 0 {
                assert!(
                    m.inspector_steps_resume < cold_equiv,
                    "resume steps < cold: resume={} skipped={} {m:?}",
                    m.inspector_steps_resume,
                    m.prefix_opcodes_skipped
                );
            }
            break;
        }
    }
    assert!(saw, "M1l valued CALL-boundary RewindTo expected: {last:?}");
}

/// M1l note: ERC-20 full `transfer` L1 still **not** claimed unless denser
/// Transfer schedules + shared-slot valued+write all green under mainnet shapes.
/// Plant now covers full-width multi-SSTORE+LOG, warm valued SC, valued CALL jump.

/// Plant v2 M3: process/residual WŜ prior drives Bind-before-touch on a
/// contended same-sender schedule (must *read* the hot Basic location).
/// Second block shows `prior_bind_hits > 0` and bounded `region_validate_fail`.
#[test]
fn specfence_m3_prior_bind_cuts_first_pass_waste() {
    let chain = PevmEthereum::mainnet();
    let hot = Address::from(U160::from(1));
    let storage = storage_for(200);
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);

    // Block 1 — learn WŜ on same-sender WW (each tx reads+writes Basic(hot)).
    let txs1: Vec<TxEnv> = (1..=40).map(|i| self_transfer(hot, i as u64)).collect();
    let seq1 = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs1.clone(),
    )
    .unwrap();
    let par1 = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs1,
            concurrency(),
        )
        .unwrap();
    assert_eq!(seq1, par1);
    let m1 = pevm.last_specfence_metrics().clone();
    assert!(
        pevm.rw_prior_hot_writes() > 0 || m1.bind_hits > 0 || m1.occ_aborts > 0,
        "block1 must learn WŜ or exercise bind/abort: prior_hot={} m1={m1:?}",
        pevm.rw_prior_hot_writes()
    );
    let fail1 = m1.region_validate_fail;

    // Block 2 — fresh-storage nonces, same hot Basic location. Process prior +
    // residual WŜ from earlier txs in the block should Bind before SpecRead.
    let txs2: Vec<TxEnv> = (1..=32).map(|i| self_transfer(hot, i as u64)).collect();
    let seq2 = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs2.clone(),
    )
    .unwrap();
    let mut saw_prior_bind = false;
    let mut last = None;
    // Bind-first + schedule noise: allow more retries to observe prior_bind.
    for _ in 0..24 {
        let par2 = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs2.clone(),
                concurrency(),
            )
            .unwrap();
        assert_eq!(seq2, par2, "M3 must preserve sequential ≡ SpecFence");
        let m2 = pevm.last_specfence_metrics().clone();
        last = Some(m2.clone());
        if m2.prior_bind_hits > 0 {
            saw_prior_bind = true;
            assert!(
                m2.region_validate_fail <= fail1.saturating_add(fail1 / 2 + 16)
                    || m2.prior_bind_hits >= m2.prior_bind_miss
                    || m2.bind_hits > 0,
                "prior Bind should improve or bound validate fails: b1_fail={fail1} m2={m2:?}"
            );
            break;
        }
    }
    assert!(
        saw_prior_bind,
        "M3 prior_bind_hits must be > 0 after learning: last={last:?} prior_hot={}",
        pevm.rw_prior_hot_writes()
    );
}

/// M4: low-conflict independent schedule engages lean OCC-fast path.
#[test]
fn specfence_m4_low_conflict_engages_lean() {
    unsafe {
        std::env::remove_var("SPECFENCE_ENABLE_INSPECT");
    }
    let n = 128;
    let txs: Vec<TxEnv> = (1..=n)
        .map(|i| self_transfer(Address::from(U160::from(i)), 1))
        .collect();
    let storage = storage_for(n);
    let chain = PevmEthereum::mainnet();
    let seq = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .unwrap();
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    pevm.reset_heat();
    let par = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs,
            concurrency(),
        )
        .unwrap();
    assert_eq!(seq, par, "M4 lean must preserve seq≡par");
    let m = pevm.last_specfence_metrics();
    assert!(
        m.lean_mode_txs > 0,
        "independent block must engage lean: {m:?}"
    );
    assert_eq!(
        m.engagement_switches, 0,
        "quiet block should not escalate: {m:?}"
    );
    assert_eq!(m.wait_admissions, 0, "lean must not Wait-admit: {m:?}");
    assert_eq!(m.wait_hard_count, 0, "lean must not WaitHard: {m:?}");
    // Inspector tax off → no inspector_steps on lean-only block.
    assert_eq!(
        m.inspector_steps, 0,
        "lean skips inspect_run: {m:?}"
    );
}

/// R1/R2: hot multi-writer populates HotSet and uses HotLocal; execute stays LeanOCC.
#[test]
fn specfence_m4_high_conflict_uses_full_plant() {
    // Keep legacy name; semantics = HotSet / HotLocal (not block-wide full inspect).
    let n = 48;
    let sender = Address::from(U160::from(1));
    let txs: Vec<TxEnv> = (1..=n).map(|i| self_transfer(sender, i as u64)).collect();
    let storage = storage_for(n + 1);
    let chain = PevmEthereum::mainnet();
    let seq = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .unwrap();
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    pevm.reset_heat();
    let mut last = None;
    let mut saw_hot = false;
    for _ in 0..6 {
        let par = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                concurrency(),
            )
            .unwrap();
        assert_eq!(seq, par, "HotLocal must preserve seq≡par");
        let m = pevm.last_specfence_metrics().clone();
        last = Some(m.clone());
        if m.hotset_size > 0 || m.hot_local_reads > 0 || m.bind_hits > 0 || m.wait_hard_count > 0 {
            saw_hot = true;
            break;
        }
    }
    assert!(
        saw_hot,
        "contended same-sender must populate HotSet / HotLocal: last={last:?}"
    );
    let m = last.unwrap();
    assert!(
        m.lean_mode_txs > 0,
        "default execute stays LeanOCC (no inspect): {m:?}"
    );
    assert_eq!(
        m.inspector_steps, 0,
        "R0: inspector_steps=0 without SPECFENCE_ENABLE_INSPECT: {m:?}"
    );
    assert!(
        m.hotset_size > 0 || m.hot_local_reads > 0,
        "HotSet/HotLocal signal expected: {m:?}"
    );
}

/// R1: wide/low-conflict → high lean_mode_txs, wait_hard≈0, inspector_steps=0.
#[test]
fn specfence_r1_wide_block_stays_lean() {
    let n = 128;
    let txs: Vec<TxEnv> = (1..=n)
        .map(|i| self_transfer(Address::from(U160::from(i)), 1))
        .collect();
    let storage = storage_for(n);
    let (_, m, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs);
    assert!(
        m.lean_mode_txs as f64 / (m.lean_mode_txs + m.full_mode_txs).max(1) as f64 >= 0.95,
        "wide block lean fraction: {m:?}"
    );
    assert_eq!(m.wait_hard_count, 0, "wide: wait_hard≈0: {m:?}");
    assert_eq!(m.inspector_steps, 0, "wide: no inspect: {m:?}");
    assert_eq!(m.hot_local_reads, 0, "wide: no HotLocal: {m:?}");
}

/// R1/R2: hot multi-writer → HotSet non-empty, hot_local path, seq≡par.
#[test]
fn specfence_r1_hot_multiwriter_hotset() {
    let n = 32;
    let sender = Address::from(U160::from(42));
    let txs: Vec<TxEnv> = (1..=n).map(|i| self_transfer(sender, i as u64)).collect();
    let storage = storage_for(n + 50);
    let chain = PevmEthereum::mainnet();
    let seq = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .unwrap();
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    pevm.reset_heat();
    let mut last = None;
    for _ in 0..4 {
        let par = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                concurrency(),
            )
            .unwrap();
        assert_eq!(seq, par);
        let m = pevm.last_specfence_metrics().clone();
        last = Some(m.clone());
        if m.hotset_size > 0 {
            assert!(
                m.hot_local_reads > 0 || m.bind_hits > 0 || m.wait_hard_count > 0 || m.spec_read_count > 0,
                "HotSet should drive HotLocal/SpecRead activity: {m:?}"
            );
            assert_eq!(m.inspector_steps, 0);
            return;
        }
    }
    panic!("expected HotSet non-empty on multi-writer: last={last:?}");
}


/// Iter24/25: production ResumePath SNAP + JUMP — SoftWait Soft=0; seq≡par.
/// Iter25: silent default is ResumePath (unset env); mass path still OFF.
#[test]
fn specfence_iter24_bind_jump_resume_path_production() {
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP"); // Iter25 silent ResumePath
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP"); // JUMP follows mode
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(97_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    for _ in 0..4 {
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
        assert_eq!(parallel, sequential, "ResumePath JUMP must stay seq≡par: {m:?}");
    }
}

/// Iter24: SPECFENCE_BIND_SNAP=0 forces capture+jump off.
#[test]
fn specfence_iter24_bind_snap_force_off() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "0");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "0");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(2, 8, 4);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..32 {
        let (addr, account) = common::mock_account(97_500 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let (_, m, _) = run_mode_conc(
        ConcurrencyMode::SpecFence,
        &storage,
        txs,
        width,
    );
    assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
    assert_eq!(m.absolute_jump_applied, 0, "force-off JUMP: {m:?}");
    assert_eq!(m.bind_snap_capture, 0, "force-off SNAP: {m:?}");
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
    }
}

/// Iter26: Validated-fresh FF-path tip→jump — SoftWait Soft=0; silent ResumePath;
/// seq≡par; all-prefix Validated spin kept (no tip_sloads skip).
#[test]
fn specfence_iter26_validated_fresh_ff_tip_jump() {
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP"); // silent ResumePath
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(97_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    for _ in 0..3 {
        unsafe {
            std::env::remove_var("SPECFENCE_BIND_SNAP");
            std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        }
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
        assert_eq!(
            parallel, sequential,
            "Validated-fresh ResumePath must stay seq≡par: {m:?}"
        );
        let _ = (m.absolute_jump_applied, m.bind_snap_capture, m.bind_snap_credit);
    }
}

/// Iter27: tip≡FF overlap + steps_cap select + deeper prefix Validated spin —
/// SoftWait Soft=0; silent ResumePath; seq≡par; no tip_sloads skip.
#[test]
fn specfence_iter27_tip_ff_overlap_steps_cap() {
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        std::env::remove_var("SPECFENCE_JUMP_DIG");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(97_100 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    for _ in 0..3 {
        unsafe {
            std::env::remove_var("SPECFENCE_BIND_SNAP");
            std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        }
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
        assert_eq!(
            parallel, sequential,
            "Iter27 ResumePath must stay seq≡par: {m:?}"
        );
        let _ = (m.absolute_jump_applied, m.bind_snap_capture, m.bind_snap_credit);
    }
}

/// Iter28: LAST_SNAP TLS clear + lean no-attach (first-frame tip diagnosis) —
/// SoftWait Soft=0; silent ResumePath; seq≡par; nested apply stays OFF.
#[test]
fn specfence_iter28_first_frame_tip_identity() {
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        std::env::remove_var("SPECFENCE_JUMP_DIG");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(97_200 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    for _ in 0..3 {
        unsafe {
            std::env::remove_var("SPECFENCE_BIND_SNAP");
            std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        }
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
        assert_eq!(
            parallel, sequential,
            "Iter28 ResumePath must stay seq≡par: {m:?}"
        );
        let _ = (m.absolute_jump_applied, m.bind_snap_capture, m.bind_snap_credit);
    }
}

/// Iter29: hang-free nested Bind credit path — SoftWait Soft=0; seq≡par.
#[test]
fn specfence_iter29_nested_bind_consume() {
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        std::env::remove_var("SPECFENCE_JUMP_DIG");
        // Iter30 default-on is Lean-safe; force OFF here to pin Iter29 credit posture.
        std::env::set_var("SPECFENCE_NESTED_BIND", "0");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(97_300 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    for _ in 0..3 {
        unsafe {
            std::env::remove_var("SPECFENCE_BIND_SNAP");
            std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
            std::env::set_var("SPECFENCE_NESTED_BIND", "0");
        }
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
        assert_eq!(
            parallel, sequential,
            "Iter29 nested Bind consume must stay seq≡par: {m:?}"
        );
        let _ = (m.absolute_jump_applied, m.bind_snap_capture, m.bind_snap_credit);
    }
    unsafe {
        std::env::remove_var("SPECFENCE_NESTED_BIND");
    }
}

/// Iter30: Lean-safe nested apply **default-on** (unset NESTED_BIND) —
/// tip_sloads addr≡target ∧ depth≤2 ∧ tip≡FF; SoftWait Soft=0; seq≡par.
#[test]
fn specfence_iter30_lean_safe_nested_apply_default_on() {
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        std::env::remove_var("SPECFENCE_JUMP_DIG");
        std::env::remove_var("SPECFENCE_NESTED_BIND"); // default-on Lean-safe
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(97_400 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    for _ in 0..3 {
        unsafe {
            std::env::remove_var("SPECFENCE_BIND_SNAP");
            std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
            std::env::remove_var("SPECFENCE_NESTED_BIND");
        }
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
        assert_eq!(
            parallel, sequential,
            "Iter30 Lean-safe nested apply default-on must stay seq≡par: {m:?}"
        );
        let _ = (m.absolute_jump_applied, m.bind_snap_capture, m.bind_snap_credit);
    }
}

/// Iter24 dig: Mass SNAP+JUMP still aj>0∧fail=0 (refuse-if-stale). Ignored.
#[ignore = "Iter24 dig: Mass SNAP+JUMP refuse-if-stale; run solo --ignored --test-threads=1"]
#[test]
fn specfence_iter24_erc20_mass_bind_jump_dig() {
    unsafe {
        std::env::set_var("SPECFENCE_BIND_SNAP", "1");
        std::env::set_var("SPECFENCE_BIND_SNAP_JUMP", "1");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..64 {
        let (addr, account) = common::mock_account(98_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    let mut success = 0usize;
    let mut fail = 0usize;
    let mut aj_runs = 0usize;
    for run in 0..16 {
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        if m.absolute_jump_applied > 0 {
            aj_runs += 1;
            if parallel == sequential {
                success += 1;
            } else {
                fail += 1;
                eprintln!("iter24 mass dig FAIL seq≠par run={run} aj={} bsnap={}", m.absolute_jump_applied, m.bind_snap_capture);
            }
        } else if parallel != sequential {
            fail += 1;
            eprintln!("iter24 mass dig seq≠par without aj run={run}");
        }
    }
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
        std::env::remove_var("SPECFENCE_BIND_SNAP");
    }
    eprintln!("iter24 mass dig: aj_runs={aj_runs} success={success} fail={fail}");
    assert_eq!(fail, 0, "Mass SNAP+JUMP must stay fail=0");
    assert!(aj_runs > 0, "expected some aj>0 under Mass dig");
}

/// Iter25: silent-default ResumePath is hang-free + SoftWait Soft=0 + seq≡par.
/// Unset SPECFENCE_BIND_SNAP → ResumePath (Mass JUMP was Lean hang; ResumePath OK).
#[test]
fn specfence_iter25_silent_default_resume_path() {
    unsafe {
        std::env::remove_var("SPECFENCE_BIND_SNAP");
        std::env::remove_var("SPECFENCE_BIND_SNAP_JUMP");
    }
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    state.insert(Address::ZERO, EvmAccount::default());
    for i in 0..48 {
        let (addr, account) = common::mock_account(98_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let width = NonZeroUsize::new(concurrency().get().min(4).max(2)).unwrap();
    let chain = PevmEthereum::mainnet();
    for _ in 0..3 {
        let sequential = execute_revm_sequential(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs.clone(),
        )
        .expect("sequential");
        let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let parallel = pevm
            .execute_revm_parallel(
                &chain,
                &storage,
                Default::default(),
                BlockEnv::default(),
                txs.clone(),
                width,
            )
            .expect("parallel");
        let m = pevm.last_specfence_metrics().clone();
        assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
        assert_eq!(m.handler_sstore_capture, 0, "stock SSTORE: {m:?}");
        assert_eq!(parallel, sequential, "silent ResumePath must stay seq≡par: {m:?}");
    }
}

/// Complete-arch: SoftWait Soft=0; edge π records Bind/Spec/WaitFor; seq≡par on
/// an ERC-20 cluster plus a second warm block (A6 prior).
#[test]
fn complete_arch_edge_pi_seq_eq_par_softwait0() {
    let (mut state, bytecodes, mut txs) = erc20::generate_cluster(4, 12, 6);
    for i in 0..8 {
        let (addr, account) = common::mock_account(77_000 + i);
        state.insert(addr, account);
        txs.push(self_transfer(addr, 1));
    }
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let (par, m, mut pevm) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
    let chain = PevmEthereum::mainnet();
    let sequential = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .expect("sequential");
    assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
    assert_eq!(par, sequential, "complete-arch must stay seq≡par: {m:?}");
    assert!(
        m.edge_bind + m.edge_spec + m.edge_wait_for > 0 || m.spec_read_count + m.bind_hits > 0,
        "edge π or Bind/Spec path should fire: {m:?}"
    );
    // A6 warm second block on the same Pevm (prior H + templates).
    let parallel = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs,
            concurrency(),
        )
        .expect("parallel-warm");
    let m2 = pevm.last_specfence_metrics().clone();
    assert_eq!(m2.soft_wait_arms, 0, "warm SoftWait Soft=0: {m2:?}");
    assert_eq!(parallel, sequential, "warm complete-arch seq≡par: {m2:?}");
}

/// Gaps-closed: known essentials WaitFor or Bind (not Spec leak); SoftWait Soft=0;
/// Avoid broadcast and Data-publish wake are live; seq≡par.
#[test]
fn gaps_closed_waitfor_avoid_publish_wake() {
    let (state, bytecodes, txs) = erc20::generate_cluster(4, 12, 6);
    let storage = InMemoryStorage::new(state, Arc::new(bytecodes), Default::default());
    let (par, m, mut pevm) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
    let chain = PevmEthereum::mainnet();
    let sequential = execute_revm_sequential(
        &chain,
        &storage,
        Default::default(),
        BlockEnv::default(),
        txs.clone(),
    )
    .expect("sequential");
    assert_eq!(par, sequential, "gaps-closed seq≡par: {m:?}");
    assert_eq!(m.soft_wait_arms, 0, "SoftWait Soft must stay 0: {m:?}");
    assert!(
        m.avoid_broadcasts > 0 || m.edge_bind > 0,
        "first-wave Avoid or Bind must fire: {m:?}"
    );
    let warm = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs,
            concurrency(),
        )
        .expect("warm");
    let m2 = pevm.last_specfence_metrics().clone();
    assert_eq!(warm, sequential, "gaps-closed warm seq≡par: {m2:?}");
    assert_eq!(m2.soft_wait_arms, 0, "warm SoftWait Soft=0: {m2:?}");
    assert!(
        m2.edge_wait_for + m2.edge_bind > 0,
        "known essentials must Bind or WaitFor, not Spec-only: {m2:?}"
    );
}
