//! SpecFence vs OCC on Ethereum mainnet block 3356896 (Soft=0).
//!
//! ```
//! cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_3356896_compare
//! ```
//!
//! Optional: `SPECFENCE_COMPARE_ITERS=5 SPECFENCE_COMPARE_CORES=8`

#![allow(missing_docs)]
#![recursion_limit = "256"]

use std::{
    fs::File,
    io::BufReader,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use alloy_rpc_types_eth::Block;
use flate2::bufread::GzDecoder;
use hashbrown::HashMap;
use pevm::{
    BlockHashes, BuildSuffixHasher, Bytecodes, ConcurrencyMode, EvmAccount, InMemoryStorage, Pevm,
    chain::{PevmChain, PevmEthereum},
};

const BLOCK: u64 = 3_356_896;
const DEFAULT_CORES: usize = 8;
const DEFAULT_ITERS: usize = 3;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_shared(data_dir: &Path) -> (Arc<Bytecodes>, Arc<BlockHashes>) {
    let bytecodes = bincode::serde::decode_from_std_read(
        &mut GzDecoder::new(BufReader::new(
            File::open(data_dir.join("bytecodes.bincode.gz")).expect("bytecodes.bincode.gz"),
        )),
        bincode::config::standard(),
    )
    .map(Arc::new)
    .expect("decode bytecodes");
    let block_hashes = Arc::new(match File::open(data_dir.join("block_hashes.bincode")) {
        Ok(file) => bincode::serde::decode_from_std_read::<BlockHashes, _, _>(
            &mut BufReader::new(file),
            bincode::config::standard(),
        )
        .unwrap_or_default(),
        Err(_) => BlockHashes::default(),
    });
    (bytecodes, block_hashes)
}

fn n_tx(block: &Block<<PevmEthereum as PevmChain>::Transaction>) -> usize {
    match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs.len(),
        alloy_rpc_types_eth::BlockTransactions::Hashes(h) => h.len(),
        alloy_rpc_types_eth::BlockTransactions::Uncle => 0,
    }
}

fn median(mut vals: Vec<f64>) -> f64 {
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    vals[vals.len() / 2]
}

fn main() {
    let data_dir = repo_root().join("data/ethereum");
    let dir = data_dir.join("blocks").join(BLOCK.to_string());
    if !dir.join("block.json").exists() {
        eprintln!("missing {dir:?}/block.json — cannot compare");
        std::process::exit(2);
    }
    let (bytecodes, block_hashes) = load_shared(&data_dir);
    let block: Block<<PevmEthereum as PevmChain>::Transaction> = serde_json::from_reader(
        BufReader::new(File::open(dir.join("block.json")).expect("block.json")),
    )
    .expect("parse block");
    let accounts: HashMap<alloy_primitives::Address, EvmAccount, BuildSuffixHasher> =
        serde_json::from_reader(BufReader::new(
            File::open(dir.join("pre_state.json")).expect("pre_state.json"),
        ))
        .expect("parse pre_state");
    let storage = InMemoryStorage::new(accounts, bytecodes, block_hashes);
    let chain = PevmEthereum::mainnet();
    let cores = std::env::var("SPECFENCE_COMPARE_CORES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CORES);
    let iters = std::env::var("SPECFENCE_COMPARE_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_ITERS)
        .max(1);
    let cores_nz = NonZeroUsize::new(cores.max(1)).unwrap();
    let n = n_tx(&block);
    println!("block={BLOCK} n={n} cores={cores} iters={iters} Soft=0");

    for mode_name in ["occ", "specfence"] {
        let mode = if mode_name == "occ" {
            ConcurrencyMode::Occ
        } else {
            ConcurrencyMode::SpecFence
        };
        let mut walls = Vec::with_capacity(iters);
        let mut last = None;
        for i in 0..iters {
            let mut pevm = Pevm::with_concurrency_mode(mode);
            pevm.reset_heat();
            pevm.reset_inter_prior();
            let t0 = Instant::now();
            let result = pevm.execute(&chain, &storage, &block, cores_nz, false);
            let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(_) => {
                    let m = pevm.last_specfence_metrics();
                    println!(
                        "  {mode_name}[{i}] ok wall_ms={wall_ms:.3} tps={:.0} occ_aborts={} full_abort={} refuse_admit={} wait_for_dependency={} wait_for_full_abort={} partial_abort_win={} partial_abort_attempt={} soft_wait_arms={}",
                        n as f64 / (wall_ms / 1000.0),
                        m.occ_aborts,
                        m.full_abort_reexecute,
                        m.refuse_admit,
                        m.wait_for_dependency,
                        m.wait_for_full_abort,
                        m.partial_abort_win,
                        m.partial_abort_attempt,
                        m.soft_wait_arms
                    );
                    walls.push(wall_ms);
                    last = Some(m.clone());
                }
                Err(e) => {
                    eprintln!("  {mode_name}[{i}] ERROR {e:?}");
                    std::process::exit(1);
                }
            }
        }
        if let Some(m) = last {
            println!(
                "  {mode_name} median_wall_ms={:.3} last refuse_admit={} wait_for_dependency={} wait_for_full_abort={} partial_abort_win={} occ_aborts={} soft_wait_arms={}",
                median(walls),
                m.refuse_admit,
                m.wait_for_dependency,
                m.wait_for_full_abort,
                m.partial_abort_win,
                m.occ_aborts,
                m.soft_wait_arms
            );
        }
    }
}
