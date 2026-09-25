//! `SpecFence` stage-1 equivalence. Compiled only with `--features specfence`.

#![cfg(feature = "specfence")]

use std::{fs::File, io::BufReader, num::NonZeroUsize, sync::Arc};

use alloy_primitives::{Address, Bloom};
use alloy_rpc_types_eth::Block;
use flate2::bufread::GzDecoder;
use hashbrown::HashMap;
use pevm::{
    BlockHashes, BuildSuffixHasher, EvmAccount, InMemoryStorage, Pevm,
    chain::{CalculateReceiptRootError, PevmChain, PevmEthereum},
    specfence::{SfClassKey, SfOptions, run_sf_block},
};
use revm::{
    context::{BlockEnv, TxEnv},
    primitives::{U256, alloy_primitives::U160},
};

fn mock_tx(idx: usize) -> TxEnv {
    let address = Address::from(U160::from(idx));
    TxEnv {
        caller: address,
        nonce: 1,
        kind: address.into(),
        value: U256::from(1),
        gas_limit: 21_000,
        gas_price: 1,
        ..TxEnv::default()
    }
}

fn assert_three_equal(label: &str, n: usize, workers: usize) {
    let chain = PevmEthereum::mainnet();
    let accounts = (0..=n)
        .map(|i| {
            let address = Address::from(U160::from(i));
            let account = pevm::EvmAccount {
                balance: U256::MAX / U256::from(2),
                nonce: 1,
                ..Default::default()
            };
            (address, account)
        })
        .collect::<pevm::ChainState>();
    let storage = InMemoryStorage::new(accounts, Default::default(), Default::default());
    let txs: Vec<TxEnv> = (1..=n).map(mock_tx).collect();
    let spec = Default::default();
    let block_env = BlockEnv::default();
    let seq = pevm::execute_revm_sequential(&chain, &storage, spec, block_env.clone(), txs.clone())
        .unwrap_or_else(|e| panic!("{label} seq {e}"));
    let mut pevm = Pevm::default();
    let occ = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            spec,
            block_env.clone(),
            txs.clone(),
            NonZeroUsize::new(workers).unwrap(),
        )
        .unwrap_or_else(|e| panic!("{label} occ {e}"));
    let sf = run_sf_block(
        &chain,
        &storage,
        spec,
        block_env,
        txs,
        SfOptions::fresh(NonZeroUsize::new(workers).unwrap(), SfClassKey::ToSelector),
    )
    .unwrap_or_else(|e| panic!("{label} sf {e}"));
    assert_eq!(seq, occ, "{label} seq!=occ");
    assert_eq!(seq, sf, "{label} seq!=sf");
    println!("SMALL_OK {label} n={n} workers={workers}");
}

#[test]
fn sf_matches_sequential_on_independent_transfers() {
    assert_three_equal("c1", 8, 1);
    assert_three_equal("c4", 32, 4);
}

fn load_block(block_no: u64) -> (Block<alloy_rpc_types_eth::Transaction>, InMemoryStorage) {
    let data_dir = std::path::PathBuf::from("../../data/ethereum");
    let bytecodes = bincode::serde::decode_from_std_read(
        &mut GzDecoder::new(BufReader::new(
            File::open(data_dir.join("bytecodes.bincode.gz")).unwrap(),
        )),
        bincode::config::standard(),
    )
    .map(Arc::new)
    .unwrap();
    let block_hashes = Arc::new(match File::open(data_dir.join("block_hashes.bincode")) {
        Ok(file) => bincode::serde::decode_from_std_read::<BlockHashes, _, _>(
            &mut BufReader::new(file),
            bincode::config::standard(),
        )
        .unwrap(),
        Err(_) => BlockHashes::default(),
    });
    let dir = data_dir.join("blocks").join(block_no.to_string());
    let block =
        serde_json::from_reader(BufReader::new(File::open(dir.join("block.json")).unwrap()))
            .unwrap();
    let accounts: HashMap<Address, EvmAccount, BuildSuffixHasher> = serde_json::from_reader(
        BufReader::new(File::open(dir.join("pre_state.json")).unwrap()),
    )
    .unwrap();
    (
        block,
        InMemoryStorage::new(accounts, bytecodes, block_hashes),
    )
}

fn check_block(block_no: u64, workers: usize, key: SfClassKey) {
    let chain = PevmEthereum::mainnet();
    let (block, storage) = load_block(block_no);
    let spec_id = chain.get_block_spec(&block.header).unwrap();
    let block_env = pevm::specfence::block_env(&block.header, spec_id);
    let txs = match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs
            .iter()
            .map(|tx| chain.get_tx_env(tx).unwrap())
            .collect::<Vec<_>>(),
        _ => panic!("full txs"),
    };
    let cores = NonZeroUsize::new(workers).unwrap();
    let seq =
        pevm::execute_revm_sequential(&chain, &storage, spec_id, block_env.clone(), txs.clone())
            .unwrap_or_else(|e| panic!("seq {block_no} {e}"));
    let mut pevm = Pevm::default();
    let occ = pevm
        .execute_revm_parallel(
            &chain,
            &storage,
            spec_id,
            block_env.clone(),
            txs.clone(),
            cores,
        )
        .unwrap_or_else(|e| panic!("occ {block_no} {e}"));
    let sf = run_sf_block(
        &chain,
        &storage,
        spec_id,
        block_env,
        txs,
        SfOptions::fresh(cores, key),
    )
    .unwrap_or_else(|e| panic!("sf {block_no} {key:?} {e}"));
    assert_eq!(seq, occ, "seq!=occ {block_no}");
    assert_eq!(seq, sf, "seq!=sf {block_no} {key:?}");
    match chain.calculate_receipt_root(spec_id, &block.transactions, &seq) {
        Ok(root) => assert_eq!(block.header.receipts_root, root, "receipt {block_no}"),
        Err(CalculateReceiptRootError::Unsupported) => {}
        Err(err) => panic!("{err:?}"),
    }
    assert_eq!(
        block.header.logs_bloom,
        seq.iter()
            .map(|tx| tx.receipt.bloom_slow())
            .fold(Bloom::default(), |a, b| a.bit_or(b))
    );
    assert_eq!(
        block.header.gas_used,
        seq.last()
            .map(|r| r.receipt.cumulative_gas_used)
            .unwrap_or_default()
    );
    let trace = pevm::specfence::last_trace();
    if let Some(t) = &trace {
        println!(
            "BLOCK_OK block={block_no} workers={workers} key={} reexec={} full_replay={} reads_after_arm={} full_replay_after_arm={} chain_len={} armed={} exec_entries={}",
            t.class_key,
            t.reexec,
            t.full_replay,
            t.reads_after_arm,
            t.full_replay_after_arm,
            t.chain_len,
            t.armed,
            t.exec_entries,
        );
    } else {
        println!("BLOCK_OK block={block_no} workers={workers} key={key:?} trace=none");
    }
    let _ = storage;
}

#[test]
fn sf_matches_onchain_focus_blocks() {
    for workers in [1usize, 4, 8] {
        for key in [SfClassKey::ToSelector, SfClassKey::CodeHashSelector] {
            check_block(3_356_896, workers, key);
            check_block(15_274_915, workers, key);
        }
    }
}

/// A few dozen fresh `SpecFence` runs against one sequential result.
#[test]
fn sf_seq_par_repeat() {
    let repeats = std::env::var("SPECFENCE_SEQ_PAR_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    let chain = PevmEthereum::mainnet();
    for block_no in [3_356_896u64, 15_274_915] {
        let (block, storage) = load_block(block_no);
        let spec_id = chain.get_block_spec(&block.header).unwrap();
        let block_env = pevm::specfence::block_env(&block.header, spec_id);
        let txs = match &block.transactions {
            alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs
                .iter()
                .map(|tx| chain.get_tx_env(tx).unwrap())
                .collect::<Vec<_>>(),
            _ => panic!("full txs"),
        };
        let seq = pevm::execute_revm_sequential(
            &chain,
            &storage,
            spec_id,
            block_env.clone(),
            txs.clone(),
        )
        .unwrap_or_else(|e| panic!("seq {block_no} {e}"));
        for workers in [4usize, 8] {
            for key in [SfClassKey::ToSelector, SfClassKey::CodeHashSelector] {
                for i in 0..repeats {
                    let sf = run_sf_block(
                        &chain,
                        &storage,
                        spec_id,
                        block_env.clone(),
                        txs.clone(),
                        SfOptions::fresh(NonZeroUsize::new(workers).unwrap(), key),
                    )
                    .unwrap_or_else(|e| panic!("sf {block_no} c={workers} {key:?} run={i} {e}"));
                    assert_eq!(seq, sf, "seq!=sf {block_no} c={workers} {key:?} run={i}");
                }
                println!(
                    "SEQ_PAR_OK block={block_no} workers={workers} key={key:?} runs={repeats}"
                );
            }
        }
    }
}
