//! Multi-core mainnet sweep: Sequential / OCC / PCC / SpecFence.
//!
//! Writes JSON for `lab/experiments/scripts/plot_vldb.py`.

#![allow(missing_docs)]

use std::{
    fs::{self, File},
    io::{BufReader, Write},
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

const DEFAULT_BLOCKS: &[u64] = &[
    19_807_137, 14_683_600, 13_217_637, 14_383_540, 15_199_017, 14_029_313, 19_434_587,
];
const DEFAULT_CORES: &[usize] = &[1, 2, 4, 8];
const DEFAULT_REPEATS: usize = 3;

#[derive(Clone)]
struct RunRow {
    block: u64,
    n_tx: usize,
    gas_used: u64,
    mode: String,
    cores: usize,
    repeat: usize,
    elapsed_ms: f64,
    tps: f64,
    occ_aborts: usize,
    abort_rate: f64,
    wait_admissions: usize,
    speculate_executions: usize,
    region_promotions: usize,
    cascade_validations_scheduled: usize,
    independent_txs_skipped_by_fence: usize,
    ok: bool,
    error: Option<String>,
}

struct LoadedBlock {
    number: u64,
    block: Block<<PevmEthereum as PevmChain>::Transaction>,
    storage: InMemoryStorage,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse_csv_u64(raw: &str) -> Vec<u64> {
    raw.split(',').filter_map(|s| s.trim().parse().ok()).collect()
}

fn parse_csv_usize(raw: &str) -> Vec<usize> {
    raw.split(',').filter_map(|s| s.trim().parse().ok()).collect()
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

fn load_block(
    data_dir: &Path,
    number: u64,
    bytecodes: Arc<Bytecodes>,
    block_hashes: Arc<BlockHashes>,
) -> Option<LoadedBlock> {
    let dir = data_dir.join("blocks").join(number.to_string());
    if !dir.join("block.json").exists() {
        eprintln!("missing snapshot {number} at {}", dir.display());
        return None;
    }
    let block = serde_json::from_reader(BufReader::new(
        File::open(dir.join("block.json")).expect("block.json"),
    ))
    .unwrap_or_else(|e| panic!("parse block {number}: {e}"));
    let accounts: HashMap<alloy_primitives::Address, EvmAccount, BuildSuffixHasher> =
        serde_json::from_reader(BufReader::new(
            File::open(dir.join("pre_state.json")).expect("pre_state.json"),
        ))
        .unwrap_or_else(|e| panic!("parse pre_state {number}: {e}"));
    Some(LoadedBlock {
        number,
        block,
        storage: InMemoryStorage::new(accounts, bytecodes, block_hashes),
    })
}

fn n_tx(block: &Block<<PevmEthereum as PevmChain>::Transaction>) -> usize {
    match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs.len(),
        other => other.len(),
    }
}

fn measure(
    chain: &PevmEthereum,
    loaded: &LoadedBlock,
    mode: &str,
    cores: usize,
    repeat: usize,
) -> RunRow {
    let n_tx = n_tx(&loaded.block);
    let gas_used = loaded.block.header.gas_used;
    let mut pevm = match mode {
        "occ" => Pevm::with_concurrency_mode(ConcurrencyMode::Occ),
        "pcc" => Pevm::with_concurrency_mode(ConcurrencyMode::Pcc),
        "specfence" => Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence),
        _ => Pevm::default(),
    };
    pevm.reset_heat();
    let cores_nz = NonZeroUsize::new(cores.max(1)).unwrap();
    let sequential = mode == "sequential";
    let t0 = Instant::now();
    let result = pevm.execute(chain, &loaded.storage, &loaded.block, cores_nz, sequential);
    let elapsed = t0.elapsed().as_secs_f64();
    let elapsed_ms = elapsed * 1000.0;
    let tps = if elapsed > 0.0 { n_tx as f64 / elapsed } else { 0.0 };
    match result {
        Ok(_) => {
            let m = pevm.last_specfence_metrics();
            let occ_aborts = if sequential { 0 } else { m.occ_aborts };
            RunRow {
                block: loaded.number,
                n_tx,
                gas_used,
                mode: mode.to_string(),
                cores: if sequential { 1 } else { cores },
                repeat,
                elapsed_ms,
                tps,
                occ_aborts,
                abort_rate: if n_tx == 0 { 0.0 } else { occ_aborts as f64 / n_tx as f64 },
                wait_admissions: if sequential { 0 } else { m.wait_admissions },
                speculate_executions: if sequential { 0 } else { m.speculate_executions },
                region_promotions: if sequential { 0 } else { m.region_promotions },
                cascade_validations_scheduled: if sequential {
                    0
                } else {
                    m.cascade_validations_scheduled
                },
                independent_txs_skipped_by_fence: if sequential {
                    0
                } else {
                    m.independent_txs_skipped_by_fence
                },
                ok: true,
                error: None,
            }
        }
        Err(err) => RunRow {
            block: loaded.number,
            n_tx,
            gas_used,
            mode: mode.to_string(),
            cores: if sequential { 1 } else { cores },
            repeat,
            elapsed_ms,
            tps: 0.0,
            occ_aborts: 0,
            abort_rate: 0.0,
            wait_admissions: 0,
            speculate_executions: 0,
            region_promotions: 0,
            cascade_validations_scheduled: 0,
            independent_txs_skipped_by_fence: 0,
            ok: false,
            error: Some(format!("{err:?}")),
        },
    }
}

fn main() {
    let mut blocks: Vec<u64> = DEFAULT_BLOCKS.to_vec();
    let mut cores: Vec<usize> = DEFAULT_CORES.to_vec();
    let mut repeats = DEFAULT_REPEATS;
    let mut out = repo_root().join("lab/results/mainnet-sweep.json");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--blocks" => {
                i += 1;
                blocks = parse_csv_u64(&args[i]);
            }
            "--cores" => {
                i += 1;
                cores = parse_csv_usize(&args[i]);
            }
            "--repeats" => {
                i += 1;
                repeats = args[i].parse().unwrap();
            }
            "--out" => {
                i += 1;
                out = PathBuf::from(&args[i]);
            }
            other => panic!("unknown arg {other}"),
        }
        i += 1;
    }

    let data_dir = repo_root().join("data/ethereum");
    let (bytecodes, block_hashes) = load_shared(&data_dir);
    let chain = PevmEthereum::mainnet();
    let mut rows = Vec::new();

    for number in blocks {
        let Some(loaded) = load_block(
            &data_dir,
            number,
            Arc::clone(&bytecodes),
            Arc::clone(&block_hashes),
        ) else {
            continue;
        };
        eprintln!(
            "block {}  txs={}  gas={}",
            loaded.number,
            n_tx(&loaded.block),
            loaded.block.header.gas_used
        );
        for repeat in 0..repeats {
            rows.push(measure(&chain, &loaded, "sequential", 1, repeat));
        }
        for mode in ["occ", "pcc", "specfence"] {
            for &c in &cores {
                for repeat in 0..repeats {
                    let row = measure(&chain, &loaded, mode, c, repeat);
                    eprintln!(
                        "  {mode:10} cores={c} r{repeat} tps={:.0} abort={:.3} wait={} ok={}",
                        row.tps, row.abort_rate, row.wait_admissions, row.ok
                    );
                    rows.push(row);
                }
            }
        }
    }

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).expect("mkdir out");
    }
    let values: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "block": r.block,
                "n_tx": r.n_tx,
                "gas_used": r.gas_used,
                "mode": r.mode,
                "cores": r.cores,
                "repeat": r.repeat,
                "elapsed_ms": r.elapsed_ms,
                "tps": r.tps,
                "occ_aborts": r.occ_aborts,
                "abort_rate": r.abort_rate,
                "wait_admissions": r.wait_admissions,
                "speculate_executions": r.speculate_executions,
                "region_promotions": r.region_promotions,
                "cascade_validations_scheduled": r.cascade_validations_scheduled,
                "independent_txs_skipped_by_fence": r.independent_txs_skipped_by_fence,
                "ok": r.ok,
                "error": r.error,
            })
        })
        .collect();
    let mut f = File::create(&out).expect("write out");
    serde_json::to_writer_pretty(&mut f, &values).expect("json");
    f.write_all(b"\n").ok();
    eprintln!("wrote {} rows to {}", rows.len(), out.display());
}
