//! SpecFence (OptimisticRead / OrderedAdmit on one Block-STM spine) vs harness OCC
//! baseline on Ethereum mainnet block 3356896 (Soft=0). Compare is measurement
//! only — not a protocol fork.
//!
//! PRIMARY wall is the **learned** SpecFence state: one `Pevm` reused across
//! iters. Reports cold iter0 and reuse median (iters 1..N-1).
//! `SPECFENCE_COLD_EACH_ITER=1` restores new-Pevm-per-iter (cold-only).
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
const DEFAULT_ITERS: usize = 5;

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
    if vals.is_empty() {
        return 0.0;
    }
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
    unfenced_reexec: usize,
    ready_width_mean: f64,
    idle_core_ns: u64,
    ordered_admit_cohorts: usize,
    optimistic_read_cohorts: usize,
    edge_ordered_admit: usize,
    edge_optimistic_read: usize,
    refuse_ns: u64,
    reexec_ns: u64,
    optimistic_majority_block: bool,
    cost_ev_keep_ordered: usize,
    cost_ev_demote_optimistic: usize,
    commute_skip: usize,
    batch_repair: usize,
    conflict_promote: usize,
    conflict_ignore: usize,
    prepaid_ns: u64,
    abort_cf_ns: u64,
    prior_decay: usize,
    admit_seed_begin_ns: u64,
    end_block_ns: u64,
    optimistic_path_tax_ns: u64,
    ordered_ns: u64,
    chosen_strategy: String,
    chosen_win_w: u8,
    chosen_w_cap: u8,
    chosen_seg_len: u8,
    unique_win_w: u8,
    unique_seg_len: u8,
    explore_budget: u8,
    ungated_occ_n: usize,
    pick_gate_n: usize,
    skip_gate_n: usize,
    ungated_occ_while_gated: usize,
    yield_ns: u64,
    gate_stall_ns: u64,
    worker_busy_ns: u64,
    win1_locs: usize,
    win2_locs: usize,
    win3_locs: usize,
    seg_locs: usize,
    full_locs: usize,
    defer_locs: usize,
    arm_switch_n: usize,
    explore_n: usize,
    win2_deviate_n: usize,
    bandit_c_opt: f64,
    bandit_c_win1: f64,
    bandit_c_win2: f64,
    bandit_c_win3: f64,
    bandit_c_defer: f64,
    selected_arms: String,
    detect_resolve_double_charge_n: usize,
    sys_reexec_n: usize,
    covering_n: usize,
    chosen_cover_window: u8,
    begin_blocked: Vec<usize>,
    taxed_indep_blocked: Vec<usize>,
    main_inc_gt0: Vec<usize>,
    storage_inc_gt0: Vec<usize>,
    off_edge_inc_gt0: Vec<usize>,
    edge_4_31: bool,
}

#[derive(Serialize)]
struct CompareSummary {
    occ_median_ms: f64,
    sf_cold_ms: Option<f64>,
    sf_reuse_median_ms: Option<f64>,
    sf_all_median_ms: f64,
    primary_sf_ms: f64,
    primary_is_reuse: bool,
    sf_le_occ: bool,
    soft: u8,
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

fn run_once(
    pevm: &mut Pevm,
    mode_name: &str,
    i: usize,
    chain: &PevmEthereum,
    storage: &InMemoryStorage,
    block: &Block<<PevmEthereum as PevmChain>::Transaction>,
    cores_nz: NonZeroUsize,
    n: usize,
) -> IterRow {
    let t0 = Instant::now();
    let result = pevm.execute(chain, storage, block, cores_nz, false);
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
            let learn = pevm.last_learn_report().clone();
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
                "  {mode_name}[{i}] ok wall_ms={wall_ms:.3} tps={:.0} occ_aborts={} inc>0={} reexec={} refuse_admit={} wait_for_dependency={} soft_wait_arms={} idle_ns={} ready_width={:.2} ordered_admit={} optimistic_read={} edge_oa={} edge_or={} refuse_ns={} reexec_ns={} ordered_ns={} prepaid_ns={} abort_cf_ns={} prior_decay={} opt_maj={} ev_keep={} ev_demote={} commute={} batch={} d1_prom={} d1_ign={} taxed_begin={} edge_4_31={} unfenced_reexec={} double_charge={} sys_reexec={} covering={} cover_window={} learn={} win_w={} w_cap={} seg_len={} uniq_w/s={}/{} expl_bud={} win1/2/3/seg/full/defer={}/{}/{}/{}/{}/{} arms={} switch={} explore={} win2_dev={} c_opt/w1/w2/w3/def={:.0}/{:.0}/{:.0}/{:.0}/{:.0} main_inc={:?} storage_inc={:?} admit_seed_ns={} end_block_ns={} opt_path_tax_ns={} ungated_occ={} pick_gate={} skip_gate={} ungated_occ_while_gated={} yield_ns={} gate_stall_ns={} busy_ns={}",
                n as f64 / (wall_ms / 1000.0),
                m.occ_aborts,
                m.incarnation_gt0,
                m.reexec_entries,
                m.refuse_admit,
                m.wait_for_dependency,
                m.soft_wait_arms,
                m.idle_core_ns,
                m.ready_width_mean,
                m.ordered_admit_cohorts,
                m.optimistic_read_cohorts,
                m.edge_ordered_admit,
                m.edge_optimistic_read,
                m.refuse_ns,
                m.reexec_ns,
                learn.ordered_ns,
                learn.prepaid_ns,
                learn.abort_cf_ns,
                learn.prior_decay,
                m.optimistic_majority_block,
                m.cost_ev_keep_ordered,
                m.cost_ev_demote_optimistic,
                m.commute_skip,
                m.batch_repair,
                m.conflict_promote,
                m.conflict_ignore,
                taxed.len(),
                edge_4_31,
                m.unfenced_reexec,
                learn.detect_resolve_double_charge_n,
                learn.sys_reexec_n,
                learn.covering_n,
                learn.chosen_cover_window,
                learn.chosen_strategy,
                learn.chosen_win_w,
                learn.chosen_w_cap,
                learn.chosen_seg_len,
                learn.unique_win_w,
                learn.unique_seg_len,
                learn.explore_budget,
                learn.win1_locs,
                learn.win2_locs,
                learn.win3_locs,
                learn.seg_locs,
                learn.full_locs,
                learn.defer_locs,
                learn.selected_arms,
                learn.arm_switch_n,
                learn.explore_n,
                learn.win2_deviate_n,
                learn.bandit_c_opt,
                learn.bandit_c_win1,
                learn.bandit_c_win2,
                learn.bandit_c_win3,
                learn.bandit_c_defer,
                inc_gt0_in(&incs, MAIN_CHAIN),
                inc_gt0_in(&incs, STORAGE_141617),
                m.admit_seed_begin_ns,
                learn.end_block_ns,
                learn.optimistic_path_tax_ns,
                learn.ungated_occ_n,
                learn.pick_gate_n,
                learn.skip_gate_n,
                learn.ungated_occ_while_gated,
                learn.yield_ns,
                learn.gate_stall_ns,
                learn.worker_busy_ns
            );
            IterRow {
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
                unfenced_reexec: m.unfenced_reexec,
                ready_width_mean: m.ready_width_mean,
                idle_core_ns: m.idle_core_ns,
                ordered_admit_cohorts: m.ordered_admit_cohorts,
                optimistic_read_cohorts: m.optimistic_read_cohorts,
                edge_ordered_admit: m.edge_ordered_admit,
                edge_optimistic_read: m.edge_optimistic_read,
                refuse_ns: m.refuse_ns,
                reexec_ns: m.reexec_ns,
                optimistic_majority_block: m.optimistic_majority_block,
                cost_ev_keep_ordered: m.cost_ev_keep_ordered,
                cost_ev_demote_optimistic: m.cost_ev_demote_optimistic,
                commute_skip: m.commute_skip,
                batch_repair: m.batch_repair,
                conflict_promote: m.conflict_promote,
                conflict_ignore: m.conflict_ignore,
                prepaid_ns: learn.prepaid_ns,
                abort_cf_ns: learn.abort_cf_ns,
                prior_decay: learn.prior_decay,
                admit_seed_begin_ns: m.admit_seed_begin_ns,
                end_block_ns: learn.end_block_ns,
                optimistic_path_tax_ns: learn.optimistic_path_tax_ns,
                ordered_ns: learn.ordered_ns,
                chosen_strategy: learn.chosen_strategy,
                chosen_win_w: learn.chosen_win_w,
                chosen_w_cap: learn.chosen_w_cap,
                chosen_seg_len: learn.chosen_seg_len,
                unique_win_w: learn.unique_win_w,
                unique_seg_len: learn.unique_seg_len,
                explore_budget: learn.explore_budget,
                ungated_occ_n: learn.ungated_occ_n,
                pick_gate_n: learn.pick_gate_n,
                skip_gate_n: learn.skip_gate_n,
                ungated_occ_while_gated: learn.ungated_occ_while_gated,
                yield_ns: learn.yield_ns,
                gate_stall_ns: learn.gate_stall_ns,
                worker_busy_ns: learn.worker_busy_ns,
                win1_locs: learn.win1_locs,
                win2_locs: learn.win2_locs,
                win3_locs: learn.win3_locs,
                seg_locs: learn.seg_locs,
                full_locs: learn.full_locs,
                defer_locs: learn.defer_locs,
                arm_switch_n: learn.arm_switch_n,
                explore_n: learn.explore_n,
                win2_deviate_n: learn.win2_deviate_n,
                bandit_c_opt: learn.bandit_c_opt,
                bandit_c_win1: learn.bandit_c_win1,
                bandit_c_win2: learn.bandit_c_win2,
                bandit_c_win3: learn.bandit_c_win3,
                bandit_c_defer: learn.bandit_c_defer,
                selected_arms: learn.selected_arms,
                detect_resolve_double_charge_n: learn.detect_resolve_double_charge_n,
                sys_reexec_n: learn.sys_reexec_n,
                covering_n: learn.covering_n,
                chosen_cover_window: learn.chosen_cover_window,
                begin_blocked: begin,
                taxed_indep_blocked: taxed,
                main_inc_gt0: inc_gt0_in(&incs, MAIN_CHAIN),
                storage_inc_gt0: inc_gt0_in(&incs, STORAGE_141617),
                off_edge_inc_gt0: off_edge,
                edge_4_31,
            }
        }
        Err(e) => {
            eprintln!("  {mode_name}[{i}] ERROR {e:?}");
            std::process::exit(1);
        }
    }
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
    let cold_each = std::env::var("SPECFENCE_COLD_EACH_ITER").is_ok();
    let cores_nz = NonZeroUsize::new(cores.max(1)).unwrap();
    let n = n_tx(&block);
    println!(
        "block={BLOCK} n={n} cores={cores} iters={iters} Soft=0 reuse_sf={} (PRIMARY=learned)",
        !cold_each
    );

    let mut rows: Vec<IterRow> = Vec::with_capacity(iters * 2);
    let mut occ_walls = Vec::new();
    let mut sf_walls = Vec::new();
    let mut last_occ = None;
    let mut last_sf = None;

    let mut occ = Pevm::with_concurrency_mode(ConcurrencyMode::Occ);
    let mut sf = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    sf.reset_heat();
    sf.reset_inter_prior();

    // Interleave OCC / SpecFence so CPU warmup does not gift one mode
    // a colder first-half (measurement only — not a protocol fork).
    for i in 0..iters {
        occ = Pevm::with_concurrency_mode(ConcurrencyMode::Occ);
        let occ_row = run_once(&mut occ, "occ", i, &chain, &storage, &block, cores_nz, n);
        occ_walls.push(occ_row.wall_ms);
        last_occ = Some(occ_row.occ_aborts);
        rows.push(occ_row);

        if cold_each {
            sf = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
            sf.reset_heat();
            sf.reset_inter_prior();
        }
        let sf_row = run_once(
            &mut sf,
            "specfence",
            i,
            &chain,
            &storage,
            &block,
            cores_nz,
            n,
        );
        sf_walls.push(sf_row.wall_ms);
        last_sf = Some((
            sf_row.refuse_admit,
            sf_row.incarnation_gt0,
            sf_row.reexec_entries,
            sf_row.wait_for_dependency,
            sf_row.occ_aborts,
            sf_row.soft_wait_arms,
            sf_row.idle_core_ns,
            sf_row.ready_width_mean,
            sf_row.edge_ordered_admit,
            sf_row.edge_optimistic_read,
            sf_row.optimistic_majority_block,
            sf_row.commute_skip,
            sf_row.batch_repair,
            sf_row.admit_seed_begin_ns,
            sf_row.unfenced_reexec,
            sf_row.ordered_admit_cohorts,
        ));
        rows.push(sf_row);
    }

    let occ_med = median(occ_walls.clone());
    let sf_all_med = median(sf_walls.clone());
    let sf_cold = sf_walls.first().copied();
    let sf_reuse_med = if sf_walls.len() >= 2 {
        Some(median(sf_walls[1..].to_vec()))
    } else {
        None
    };
    let primary_is_reuse = !cold_each && sf_reuse_med.is_some();
    let primary_sf = if primary_is_reuse {
        sf_reuse_med.unwrap()
    } else {
        sf_all_med
    };
    let sf_le_occ = primary_sf <= occ_med + 1e-9;

    if let Some(aborts) = last_occ {
        println!("  occ median_wall_ms={occ_med:.3} last occ_aborts={aborts}");
    }
    if let Some(m) = last_sf {
        println!(
            "  specfence all_median_ms={sf_all_med:.3} cold_ms={:.3} reuse_median_ms={} last refuse_admit={} inc>0={} reexec={} wait_for_dependency={} occ_aborts={} soft_wait_arms={} idle_ns={} ready_width={:.2} edge_oa={} edge_or={} opt_maj={} commute={} batch={} admit_seed_ns={} unfenced={} ordered_admit={}",
            sf_cold.unwrap_or(0.0),
            sf_reuse_med
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| "n/a".into()),
            m.0,
            m.1,
            m.2,
            m.3,
            m.4,
            m.5,
            m.6,
            m.7,
            m.8,
            m.9,
            m.10,
            m.11,
            m.12,
            m.13,
            m.14,
            m.15
        );
    }

    println!(
        "summary Soft=0 occ_median_ms={occ_med:.3} sf_cold_ms={} sf_reuse_median_ms={} primary_sf_ms={primary_sf:.3} primary={} sf_le_occ={sf_le_occ}",
        sf_cold
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".into()),
        sf_reuse_med
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".into()),
        if primary_is_reuse { "reuse" } else { "cold" }
    );

    let summary = CompareSummary {
        occ_median_ms: occ_med,
        sf_cold_ms: sf_cold,
        sf_reuse_median_ms: sf_reuse_med,
        sf_all_median_ms: sf_all_med,
        primary_sf_ms: primary_sf,
        primary_is_reuse,
        sf_le_occ,
        soft: 0,
    };

    if let Ok(path) = std::env::var("SPECFENCE_COMPARE_JSON") {
        let f = File::create(&path).expect("compare json");
        serde_json::to_writer_pretty(f, &serde_json::json!({"rows": rows, "summary": summary}))
            .expect("write json");
        println!("wrote {path}");
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"rows": rows, "summary": summary}))
                .unwrap()
        );
    }
}
