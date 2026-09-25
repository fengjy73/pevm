//! The old fork replaced `SLOAD` with `Instruction::new(handler, 0)`.
//! `insert_instruction` replaces static gas with the handler, and the
//! interpreter charges that static gas before the handler. Stock `SLOAD`
//! does not charge it again. The hook lived in `build_evm`, so sequential
//! execution lost the same gas and `seq == par` still missed the header.
//!
//! 3356896 is Spurious Dragon (`SLOAD` static gas 200). 15274915 is London
//! (warm static gas 100). Pre-Byzantium receipt roots embed the post-state
//! root, which this engine does not rebuild, so 3356896 checks gas and logs
//! bloom. London checks gas and the receipt root.

use std::{fs::File, io::BufReader, num::NonZeroUsize, sync::Arc};

use alloy_primitives::{Address, Bloom};
use alloy_rpc_types_eth::Block;
use flate2::bufread::GzDecoder;
use hashbrown::HashMap;
use pevm::{
    BlockHashes, BuildSuffixHasher, EvmAccount, InMemoryStorage, Pevm, PevmTxExecutionResult,
    chain::{CalculateReceiptRootError, PevmChain, PevmEthereum},
};
use revm::primitives::hardfork::SpecId;

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

fn assert_header(
    chain: &PevmEthereum,
    spec_id: SpecId,
    block: &Block<alloy_rpc_types_eth::Transaction>,
    results: &[PevmTxExecutionResult],
    label: &str,
) {
    let gas = results
        .last()
        .map(|tx| tx.receipt.cumulative_gas_used)
        .unwrap_or_default();
    assert_eq!(
        block.header.gas_used, gas,
        "{label} block {} gas {gas} != header {} (SLOAD static gas)",
        block.header.number, block.header.gas_used
    );
    assert_eq!(
        block.header.logs_bloom,
        results
            .iter()
            .map(|tx| tx.receipt.bloom_slow())
            .fold(Bloom::default(), |acc, bloom| acc.bit_or(bloom)),
        "{label} bloom"
    );
    // EIP-658 (Byzantium) is the first fork whose receipt root is status, gas,
    // and logs. Spurious Dragon still folds in the intermediate state root.
    match chain.calculate_receipt_root(spec_id, &block.transactions, results) {
        Ok(root) => assert_eq!(
            block.header.receipts_root, root,
            "{label} receipt root block {}",
            block.header.number
        ),
        Err(CalculateReceiptRootError::Unsupported) => {
            assert!(
                spec_id < SpecId::BYZANTIUM,
                "{label} receipt root unsupported on {spec_id:?}"
            );
        }
        Err(err) => panic!("{label} receipt root {err:?}"),
    }
}

fn check_block(block_no: u64, spec_id: SpecId) {
    let chain = PevmEthereum::mainnet();
    let (block, storage) = load_block(block_no);
    let got = chain.get_block_spec(&block.header).unwrap();
    assert_eq!(got, spec_id, "block {block_no} spec");
    let n = match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs.len(),
        _ => panic!("full txs"),
    };
    let workers = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    // `Pevm::execute` falls back to sequential when the block is smaller than
    // the worker count or used under 4_000_000 gas. Both focus blocks are past
    // that gate, so `force_sequential = false` is upstream OCC.
    assert!(n >= usize::from(workers), "block {block_no} would skip OCC");
    assert!(
        block.header.gas_used >= 4_000_000,
        "block {block_no} would skip OCC"
    );
    let mut pevm = Pevm::default();
    let seq = pevm
        .execute(&chain, &storage, &block, workers, true)
        .unwrap_or_else(|err| panic!("seq {block_no} {err}"));
    let occ = pevm
        .execute(&chain, &storage, &block, workers, false)
        .unwrap_or_else(|err| panic!("occ {block_no} {err}"));
    assert_eq!(seq, occ, "seq!=occ {block_no}");
    assert_header(&chain, spec_id, &block, &seq, "seq");
    assert_header(&chain, spec_id, &block, &occ, "occ");

    #[cfg(feature = "specfence")]
    {
        use pevm::specfence::{SfClassKey, SfOptions, run_sf_block};
        let block_env = pevm::specfence::block_env(&block.header, spec_id);
        let txs = match &block.transactions {
            alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs
                .iter()
                .map(|tx| chain.get_tx_env(tx).unwrap())
                .collect::<Vec<_>>(),
            _ => panic!("full txs"),
        };
        let sf = run_sf_block(
            &chain,
            &storage,
            spec_id,
            block_env,
            txs,
            SfOptions::fresh(workers, SfClassKey::ToSelector),
        )
        .unwrap_or_else(|err| panic!("sf {block_no} {err}"));
        assert_eq!(seq, sf, "seq!=sf {block_no}");
        assert_header(&chain, spec_id, &block, &sf, "sf");
    }
}

#[test]
fn sload_static_gas_matches_chain_header() {
    check_block(3_356_896, SpecId::SPURIOUS_DRAGON);
    check_block(15_274_915, SpecId::LONDON);
}
