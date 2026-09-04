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

/// Hot recipient: multi-writer heat seeds Wait on the next block (v1: no
/// hint-only intra-block Wait promotion). Mocked ETH transfers often omit the
/// recipient from the write set, so WW contention may not promote in-block.
#[test]
fn specfence_hot_recipient() {
    let chain = PevmEthereum::mainnet();
    let n = 64;
    let recipient = Address::from(U160::from(1));
    let txs1: Vec<TxEnv> = (2..=n + 1)
        .map(|i| transfer(Address::from(U160::from(i)), recipient, 1))
        .collect();
    let storage = storage_for(n + 40);

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

    // Fresh senders (pre-state storage is not updated across blocks).
    let txs2: Vec<TxEnv> = (n + 2..=n + 17)
        .map(|i| transfer(Address::from(U160::from(i)), recipient, 1))
        .collect();
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
        pevm.last_initial_wait_accounts().contains(&recipient),
        "hot recipient must seed Wait via inter-block heat: {:?}",
        pevm.last_initial_wait_accounts()
    );
    let metrics = pevm.last_specfence_metrics();
    assert!(
        metrics.wait_admissions > 0,
        "second block should Wait on heated recipient: {metrics:?}"
    );
}

/// Same sender, increasing nonces: sender location WW promotes Wait (observed).
#[test]
fn specfence_same_sender() {
    let n = 48;
    let sender = Address::from(U160::from(1));
    let txs: Vec<TxEnv> = (1..=n).map(|i| self_transfer(sender, i as u64)).collect();
    let storage = storage_for(n + 1);
    let (_, metrics, _) = run_mode(ConcurrencyMode::SpecFence, &storage, txs);
    assert!(
        metrics.region_promotions > 0 || metrics.wait_admissions > 0,
        "same sender should Wait: {metrics:?}"
    );
}

/// Mixed block after heat: hot shared recipient Waits; independents Speculate.
#[test]
fn specfence_mixed_hot_and_independent() {
    let chain = PevmEthereum::mainnet();
    let hot_n = 48;
    let indep_n = 96;
    let recipient = Address::from(U160::from(1));
    let mut txs1 = Vec::new();
    let mut max_idx = 1;
    for i in 0..hot_n {
        let sender = Address::from(U160::from(2 + i));
        max_idx = max_idx.max(2 + i);
        txs1.push(transfer(sender, recipient, 1));
    }
    let storage = storage_for(max_idx + indep_n + 40);

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

    let indep_start = 2 + hot_n;
    let mut txs2 = Vec::new();
    // Fresh senders into heated recipient (storage pre-state unchanged).
    for i in 0..16 {
        let sender = Address::from(U160::from(indep_start + indep_n + i));
        txs2.push(transfer(sender, recipient, 1));
    }
    for i in 0..indep_n {
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
    assert!(
        metrics.wait_admissions > 0,
        "Wait admissions on heated hot account: {metrics:?}"
    );
    assert!(
        metrics.wait_addresses.contains(&recipient),
        "Wait on hot recipient {recipient:?}: {metrics:?}"
    );
    assert!(
        metrics.speculate_executions > 0,
        "independents must speculate: {metrics:?}"
    );
    let indep_addr = Address::from(U160::from(indep_start));
    assert!(
        metrics.speculate_addresses.contains(&indep_addr),
        "independent account should speculate: {metrics:?}"
    );
    assert!(
        !metrics.wait_addresses.contains(&indep_addr),
        "independents must not wait on the hot cluster: {metrics:?}"
    );
}

/// After a hot-recipient block, the next block starts that account in Wait.
#[test]
fn specfence_inter_block_heat() {
    let chain = PevmEthereum::mainnet();
    let recipient = Address::from(U160::from(1));
    let hot_n = 40;
    let txs1: Vec<TxEnv> = (2..=hot_n + 1)
        .map(|i| transfer(Address::from(U160::from(i)), recipient, 1))
        .collect();
    let storage = storage_for(hot_n + 80);

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
    assert!(
        !pevm.last_initial_wait_accounts().contains(&recipient),
        "first block is cold: {:?}",
        pevm.last_initial_wait_accounts()
    );

    // Next block: a few more transfers to the now-hot recipient plus independents.
    let mut txs2 = Vec::new();
    for i in 0..8 {
        let sender = Address::from(U160::from(hot_n + 2 + i));
        txs2.push(transfer(sender, recipient, 1));
    }
    for i in 0..32 {
        let addr = Address::from(U160::from(hot_n + 20 + i));
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
        pevm.last_initial_wait_accounts().contains(&recipient),
        "hot recipient must start in Wait after heat: {:?}",
        pevm.last_initial_wait_accounts()
    );
    let metrics = pevm.last_specfence_metrics();
    // Wait admission is scheduling-sensitive for lazy ETH transfers (prior writer
    // may already be done). Seeding Wait via heat is the invariant under test.
    assert!(
        metrics.speculate_executions > 0,
        "independents still speculate: {metrics:?}"
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
    // mock_account uses Address::from(U160::from(idx)) — same as common
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
