//! SpecFence (A0/A1 adaptive OCC on one Block-STM spine) vs harness OCC
//! baseline on Ethereum mainnet block 3356896 (Soft=0). Compare is measurement
//! only — not a protocol fork.
//!
//! ```
//! SPECFENCE_COMPARE_ITERS=5 cargo run -p pevm --release \
//!   --config 'profile.release.lto=false' --example specfence_3356896_compare
//! ```

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
use serde::Serialize;

const BLOCK: u64 = 3_356_896;
const DEFAULT_CORES: usize = 8;
const DEFAULT_ITERS: usize = 3;

/// Tip-DAG independents that PR15 taxed at begin_block (same-from lazy + dual empty-to).
const TAXED_INDEP: &[usize] = &[
    6, 7, 8, 9, 10, 11, 12, 13, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86,
];
const MAIN_CHAIN: &[usize] = &[
    4, 31, 66, 67, 69, 70, 93, 96, 103, 115, 131, 132, 135, 138, 141, 166, 171,
];
const STORAGE_141617: &[usize] = &[14, 16, 17];

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

#[derive(Serialize)]
struct IterRow {
    mode: String,
    i: usize,
    wall_ms: f64,
    occ_aborts: usize,
    refuse_admit: usize,
    wait_for_dependency: usize,
    wait_for_full_abort: usize,
    soft_wait_arms: usize,
    incarnation_gt0: usize,
    reexec_entries: usize,
    miss_detect: usize,
    ready_width_mean: f64,
    idle_core_ns: u64,
    a1_cohorts: usize,
    lean_a0_cohorts: usize,
    edge_ordered_admit: usize,
    edge_optimistic_read: usize,
    refuse_ns: u64,
    reexec_ns: u64,
    thin_shell: bool,
    ns_ev_keep_a1: usize,
    ns_ev_demote: usize,
    commute_skip: usize,
    batch_repair: usize,
    conflict_promote: usize,
    conflict_ignore: usize,
    begin_blocked: Vec<usize>,
    taxed_indep_blocked: Vec<usize>,
    main_inc_gt0: Vec<usize>,
    storage_inc_gt0: Vec<usize>,
    off_edge_inc_gt0: Vec<usize>,
    edge_4_31: bool,
}

fn inc_gt0_in(incs: &[usize], set: &[usize]) -> Vec<usize> {
    set.iter()
        .copied()
        .filter(|&t| incs.get(t).copied().unwrap_or(0) > 0)
        .collect()
}

fn writers_have_4_31(orders: &[(u64, Vec<usize>)]) -> bool {
    orders.iter().any(|(_, w)| {
        let i4 = w.iter().position(|&t| t == 4);
        let i31 = w.iter().position(|&t| t == 31);
        match (i4, i31) {
            (Some(a), Some(b)) => a < b,
            _ => false,
        }
    })
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

    let mut rows: Vec<IterRow> = Vec::with_capacity(iters * 2);
    let mut occ_walls = Vec::new();
    let mut sf_walls = Vec::new();
    let mut last_occ = None;
    let mut last_sf = None;

    // Interleave OCC / SpecFence so CPU warmup does not gift one mode
    // a colder first-half (measurement only — not a protocol fork).
    for i in 0..iters {
        for mode_name in ["occ", "specfence"] {
            let mode = if mode_name == "occ" {
                ConcurrencyMode::Occ
            } else {
                ConcurrencyMode::SpecFence
            };
            let mut pevm = Pevm::with_concurrency_mode(mode);
            pevm.reset_heat();
            pevm.reset_inter_prior();
            let t0 = Instant::now();
            let result = pevm.execute(&chain, &storage, &block, cores_nz, false);
            let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(_) => {
                    let m = pevm.last_specfence_metrics();
                    let incs = pevm.last_incarnations().to_vec();
                    let begin = pevm.last_begin_blocked().to_vec();
                    let taxed: Vec<usize> = begin
                        .iter()
                        .copied()
                        .filter(|t| TAXED_INDEP.contains(t))
                        .collect();
                    let edge_4_31 = writers_have_4_31(pevm.last_location_writers());
                    let off_edge: Vec<usize> = incs
                        .iter()
                        .enumerate()
                        .filter(|&(t, inc)| {
                            *inc > 0
                                && !MAIN_CHAIN.contains(&t)
                                && !STORAGE_141617.contains(&t)
                                && ![20, 109, 149].contains(&t)
                        })
                        .map(|(t, _)| t)
                        .collect();
                    println!(
                        "  {mode_name}[{i}] ok wall_ms={wall_ms:.3} tps={:.0} occ_aborts={} inc>0={} reexec={} refuse_admit={} wait_for_dependency={} soft_wait_arms={} idle_ns={} ready_width={:.2} a1={} lean_a0={} edge_oa={} edge_or={} refuse_ns={} reexec_ns={} thin={} ns_keep={} ns_demote={} commute={} batch={} d1_prom={} d1_ign={} taxed_begin={} edge_4_31={} miss_detect={}",
                        n as f64 / (wall_ms / 1000.0),
                        m.occ_aborts,
                        m.incarnation_gt0,
                        m.reexec_entries,
                        m.refuse_admit,
                        m.wait_for_dependency,
                        m.soft_wait_arms,
                        m.idle_core_ns,
                        m.ready_width_mean,
                        m.a1_cohorts,
                        m.lean_a0_cohorts,
                        m.edge_ordered_admit,
                        m.edge_optimistic_read,
                        m.refuse_ns,
                        m.reexec_ns,
                        m.thin_shell,
                        m.ns_ev_keep_a1,
                        m.ns_ev_demote,
                        m.commute_skip,
                        m.batch_repair,
                        m.conflict_promote,
                        m.conflict_ignore,
                        taxed.len(),
                        edge_4_31,
                        m.miss_detect
                    );
                    let row = IterRow {
                        mode: mode_name.to_string(),
                        i,
                        wall_ms,
                        occ_aborts: m.occ_aborts,
                        refuse_admit: m.refuse_admit,
                        wait_for_dependency: m.wait_for_dependency,
                        wait_for_full_abort: m.wait_for_full_abort,
                        soft_wait_arms: m.soft_wait_arms,
                        incarnation_gt0: m.incarnation_gt0,
                        reexec_entries: m.reexec_entries,
                        miss_detect: m.miss_detect,
                        ready_width_mean: m.ready_width_mean,
                        idle_core_ns: m.idle_core_ns,
                        a1_cohorts: m.a1_cohorts,
                        lean_a0_cohorts: m.lean_a0_cohorts,
                        edge_ordered_admit: m.edge_ordered_admit,
                        edge_optimistic_read: m.edge_optimistic_read,
                        refuse_ns: m.refuse_ns,
                        reexec_ns: m.reexec_ns,
                        thin_shell: m.thin_shell,
                        ns_ev_keep_a1: m.ns_ev_keep_a1,
                        ns_ev_demote: m.ns_ev_demote,
                        commute_skip: m.commute_skip,
                        batch_repair: m.batch_repair,
                        conflict_promote: m.conflict_promote,
                        conflict_ignore: m.conflict_ignore,
                        begin_blocked: begin,
                        taxed_indep_blocked: taxed,
                        main_inc_gt0: inc_gt0_in(&incs, MAIN_CHAIN),
                        storage_inc_gt0: inc_gt0_in(&incs, STORAGE_141617),
                        off_edge_inc_gt0: off_edge,
                        edge_4_31,
                    };
                    if mode_name == "occ" {
                        occ_walls.push(wall_ms);
                        last_occ = Some(m.clone());
                    } else {
                        sf_walls.push(wall_ms);
                        last_sf = Some(m.clone());
                    }
                    rows.push(row);
                }
                Err(e) => {
                    eprintln!("  {mode_name}[{i}] ERROR {e:?}");
                    std::process::exit(1);
                }
            }
        }
    }
    for (mode_name, walls, last) in [
        ("occ", occ_walls.clone(), last_occ),
        ("specfence", sf_walls.clone(), last_sf),
    ] {
        if let Some(m) = last {
            println!(
                "  {mode_name} median_wall_ms={:.3} last refuse_admit={} inc>0={} reexec={} wait_for_dependency={} occ_aborts={} soft_wait_arms={} idle_ns={} ready_width={:.2} edge_oa={} edge_or={} thin={} commute={} batch={}",
                median(walls),
                m.refuse_admit,
                m.incarnation_gt0,
                m.reexec_entries,
                m.wait_for_dependency,
                m.occ_aborts,
                m.soft_wait_arms,
                m.idle_core_ns,
                m.ready_width_mean,
                m.edge_ordered_admit,
                m.edge_optimistic_read,
                m.thin_shell,
                m.commute_skip,
                m.batch_repair
            );
        }
    }

    if !occ_walls.is_empty() && !sf_walls.is_empty() {
        let occ_med = median(occ_walls);
        let sf_med = median(sf_walls);
        println!(
            "summary Soft=0 occ_median_ms={occ_med:.3} sf_median_ms={sf_med:.3} sf_le_occ={}",
            sf_med <= occ_med + 1e-9
        );
    }

    if let Ok(path) = std::env::var("SPECFENCE_COMPARE_JSON") {
        let f = File::create(&path).expect("compare json");
        serde_json::to_writer_pretty(f, &rows).expect("write json");
        println!("wrote {path}");
    } else {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap());
    }
}
