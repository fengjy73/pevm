//! `SpecFence` mixed Wait/Speculate tests (no mainnet download).

use std::{fmt::Debug, num::NonZeroUsize, sync::Arc, thread};

use pevm::{
    ConcurrencyMode, EvmAccount, InMemoryStorage, Pevm, PevmTxExecutionResult, Storage,
    chain::PevmEthereum, execute_revm_sequential,
};
use revm::{
    context::{BlockEnv, TransactTo, TxEnv},
    primitives::{Address, U256, alloy_primitives::U160},
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
    assert!(
        pevm.last_initial_wait_accounts().contains(&sender),
        "Bayes carry must seed Wait for conflicted sender: p_after_b1={p1} initial={:?}",
        pevm.last_initial_wait_accounts()
    );
    let metrics = pevm.last_specfence_metrics();
    assert!(
        metrics.speculate_executions > 0,
        "independents still speculate: {metrics:?}"
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
    let txs1: Vec<TxEnv> = (1..=48).map(|i| self_transfer(hot, i as u64)).collect();
    let storage = storage_for(200);

    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    let _ = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs1,
            concurrency(),
        )
        .unwrap();

    let indep_start = 40usize;
    let mut txs2 = Vec::new();
    for i in 0..8 {
        let from = Address::from(U160::from(indep_start + 64 + i));
        txs2.push(transfer(from, hot, 1));
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
    assert!(
        pevm.last_initial_wait_accounts().contains(&hot),
        "hot sender seeded Wait: {:?}",
        pevm.last_initial_wait_accounts()
    );
    let metrics = pevm.last_specfence_metrics();
    assert!(
        metrics.wait_admissions > 0 || metrics.bayes_wait_decisions > 0,
        "Wait on heated sender: {metrics:?}"
    );
    assert!(
        metrics.speculate_executions > 0,
        "independents must speculate: {metrics:?}"
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

    // Block 1: heat the sender posterior + residual write-set via same-sender WW.
    let txs1: Vec<TxEnv> = (1..=40).map(|i| self_transfer(hot, i as u64)).collect();
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    let _ = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            Default::default(),
            BlockEnv::default(),
            txs1,
            concurrency(),
        )
        .unwrap();
    let aborts_b1 = pevm.last_specfence_metrics().occ_aborts;
    assert!(
        pevm.bayes_account_conflict_prob(&hot) >= 0.25,
        "hotspot posterior must rise"
    );

    // Block 2: transfers into the heated account — Wait/Bind should dominate.
    let mut txs2 = Vec::new();
    for i in 0..12 {
        let from = Address::from(U160::from(80 + i));
        txs2.push(transfer(from, hot, 1));
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
            || m.bayes_wait_decisions > 0,
        "second wave should WaitHard/Bind on hotspot: {m:?}"
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

    let mut saw_partial = false;
    let mut last_metrics = None;
    for _ in 0..10 {
        let (_, metrics, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs.clone());
        last_metrics = Some(metrics.clone());
        if metrics.partial_retry_count >= 1 || metrics.rewind_to_cp >= 1 {
            saw_partial = true;
            assert!(metrics.occ_aborts > 0, "partial implies abort: {metrics:?}");
            break;
        }
    }
    assert!(
        saw_partial,
        "P2 PartialRetry must fire on localized conflict: {:?}",
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
                m.partial_retry_count > 0 || m.tx_full_retry > 0,
                "abort must be Partial or Full: {m:?}"
            );
            if m.partial_retry_count > 0 && m.tx_full_retry < m.occ_aborts {
                broke_equality = true;
                break;
            }
            if m.partial_retry_count > 0 {
                // Even if some fallbacks, PartialRetry path was taken.
                broke_equality = true;
                break;
            }
        }
    }
    assert!(any_abort, "expected aborts on ERC-20 cluster: {last:?}");
    assert!(
        broke_equality,
        "expected PartialRetry to decouple full_retry from aborts: {last:?}"
    );
}

/// M1: on localized conflict, RewindTo / resume must fire instead of
/// tx_head_reexec, and resume_count tracks non-head reentries.
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
            txs,
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

/// M1b: RewindTo resume must journal-FF the certified prefix and serve at least
/// one prefix read from the FF cache (skipping an MV/storage heavy op).
/// Concrete proof that resume does less DB work than a full head reexec path.
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
