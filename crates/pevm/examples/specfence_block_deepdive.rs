//! Measurement-only interleaved OCC / SpecFence deep-dig for named blocks.
//! Does **not** change CC / policy / learn. Soft=0. Instant-off is PRIMARY.
//! `SPECFENCE_PROFILE=1` Instant-tax is a separate process and must not be
//! added into wall. Extra finegrain pass is structure-only (not wall).
//!
//! ```
//! SPECFENCE_DEEPDIVE_BLOCKS=15274915,13217637 SPECFENCE_COMPARE_ITERS=7 \
//!   cargo run -p pevm --release --config 'profile.release.lto=false' \
//!   --example specfence_block_deepdive
//! ```

#![allow(missing_docs)]
#![recursion_limit = "256"]

use std::{
    collections::HashMap as StdHashMap,
    fs::{self, File},
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
    analyze_dag,
    chain::{PevmChain, PevmEthereum},
    hot_locations, kind_histogram, BlockHashes, BuildSuffixHasher, Bytecodes, ConcurrencyMode,
    EvmAccount, InMemoryStorage, Pevm,
};
use serde::Serialize;

const DEFAULT_CORES: usize = 8;
const DEFAULT_ITERS: usize = 7;
const SPINE_TOP: usize = 12;
const INC_GT0_CAP: usize = 48;

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

fn profile_on() -> bool {
    match std::env::var_os("SPECFENCE_PROFILE") {
        Some(v) => !v.is_empty() && v != "0",
        None => false,
    }
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
    chosen_w_need: u8,
    pick_occ_n: usize,
    pick_gate_n: usize,
    skip_gate_n: usize,
    occ_pick_while_gated: usize,
    yield_ns: u64,
    gate_stall_ns: u64,
    worker_busy_ns: u64,
    win1_locs: usize,
    win2_locs: usize,
    win3_locs: usize,
    seg_locs: usize,
    full_locs: usize,
    defer_locs: usize,
    opt_locs: usize,
    selected_arms: String,
    double_pay_n: usize,
    sys_reexec_n: usize,
    covering_n: usize,
    arm_switch_n: usize,
    occ_kernel_execs: usize,
    occ_kernel_validates: usize,
    optimistic_read_occ_fast: usize,
    begin_blocked: Vec<usize>,
    begin_blocked_n: usize,
    inc_gt0_txs: Vec<usize>,
    instant_tax: InstantTax,
}

#[derive(Serialize)]
struct InstantTax {
    labeled: &'static str,
    profile_on: bool,
    handler_ns: u64,
    maybe_wait_ns: u64,
    validate_ns: u64,
    scheduler_ns: u64,
}

#[derive(Serialize)]
struct SpineRow {
    loc: u64,
    kind: String,
    n_writers: usize,
    writers: Vec<usize>,
}

fn run_once(
    pevm: &mut Pevm,
    mode_name: &str,
    i: usize,
    chain: &PevmEthereum,
    storage: &InMemoryStorage,
    block: &Block<<PevmEthereum as PevmChain>::Transaction>,
    cores_nz: NonZeroUsize,
) -> IterRow {
    let t0 = Instant::now();
    let result = pevm.execute(chain, storage, block, cores_nz, false);
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    match result {
        Ok(_) => {
            let m = pevm.last_specfence_metrics();
            let incs = pevm.last_incarnations();
            let begin = pevm.last_begin_blocked().to_vec();
            let learn = pevm.last_learn_report().clone();
            let inc_gt0: Vec<usize> = incs
                .iter()
                .enumerate()
                .filter(|(_, inc)| **inc > 0)
                .map(|(t, _)| t)
                .take(INC_GT0_CAP)
                .collect();
            println!(
                "  {mode_name}[{i}] wall_ms={wall_ms:.3} learn={} w={} need={} unf={} dp={} sys={} cover={} refuse={} commute={} inc>0={} reexec_ns={} end_ns={} begin_n={} pick_occ/gate/skip={}/{}/{} occ_while_gated={} soft={} oa={} prepaid_ns={} idle_ns={}",
                learn.chosen_strategy,
                learn.chosen_win_w,
                learn.chosen_w_need,
                m.unfenced_reexec,
                learn.double_pay_n,
                learn.sys_reexec_n,
                learn.covering_n,
                m.refuse_admit,
                m.commute_skip,
                m.incarnation_gt0,
                m.reexec_ns,
                learn.end_block_ns,
                begin.len(),
                learn.pick_occ_n,
                learn.pick_gate_n,
                learn.skip_gate_n,
                learn.occ_pick_while_gated,
                m.soft_wait_arms,
                m.edge_ordered_admit,
                learn.prepaid_ns,
                m.idle_core_ns
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
                chosen_w_need: learn.chosen_w_need,
                pick_occ_n: learn.pick_occ_n,
                pick_gate_n: learn.pick_gate_n,
                skip_gate_n: learn.skip_gate_n,
                occ_pick_while_gated: learn.occ_pick_while_gated,
                yield_ns: learn.yield_ns,
                gate_stall_ns: learn.gate_stall_ns,
                worker_busy_ns: learn.worker_busy_ns,
                win1_locs: learn.win1_locs,
                win2_locs: learn.win2_locs,
                win3_locs: learn.win3_locs,
                seg_locs: learn.seg_locs,
                full_locs: learn.full_locs,
                defer_locs: learn.defer_locs,
                opt_locs: learn.opt_locs,
                selected_arms: learn.selected_arms,
                double_pay_n: learn.double_pay_n,
                sys_reexec_n: learn.sys_reexec_n,
                covering_n: learn.covering_n,
                arm_switch_n: learn.arm_switch_n,
                occ_kernel_execs: m.occ_kernel_execs,
                occ_kernel_validates: m.occ_kernel_validates,
                optimistic_read_occ_fast: m.optimistic_read_occ_fast,
                begin_blocked_n: begin.len(),
                begin_blocked: begin,
                inc_gt0_txs: inc_gt0,
                instant_tax: InstantTax {
                    labeled: if profile_on() {
                        "Instant-tax worker-sum; do not add into wall"
                    } else {
                        "Instant-off (product path); PROFILE buckets are 0"
                    },
                    profile_on: profile_on(),
                    handler_ns: m.profile_handler_ns,
                    maybe_wait_ns: m.profile_maybe_wait_ns,
                    validate_ns: m.profile_validate_ns,
                    scheduler_ns: m.profile_scheduler_ns,
                },
            }
        }
        Err(e) => {
            eprintln!("  {mode_name}[{i}] ERROR {e:?}");
            std::process::exit(1);
        }
    }
}

fn structure_pass(
    chain: &PevmEthereum,
    storage: &InMemoryStorage,
    block: &Block<<PevmEthereum as PevmChain>::Transaction>,
    cores_nz: NonZeroUsize,
    last_writers: &[(u64, Vec<usize>)],
) -> serde_json::Value {
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    pevm.reset_heat();
    pevm.reset_inter_prior();
    pevm.set_finegrain_trace(true);
    if let Err(e) = pevm.execute(chain, storage, block, cores_nz, false) {
        return serde_json::json!({
            "ok": false,
            "error": format!("{e:?}"),
            "note": "structure pass failed; PRIMARY walls still valid",
        });
    }
    let snap = pevm.take_finegrain_snapshot();
    let kind_map: StdHashMap<u64, String> = snap
        .as_ref()
        .map(|s| s.location_kinds.iter().cloned().collect())
        .unwrap_or_default();
    let mut spines: Vec<SpineRow> = last_writers
        .iter()
        .map(|(loc, w)| SpineRow {
            loc: *loc,
            kind: kind_map
                .get(loc)
                .cloned()
                .unwrap_or_else(|| "unknown".into()),
            n_writers: w.len(),
            writers: w.clone(),
        })
        .collect();
    spines.sort_by(|a, b| b.n_writers.cmp(&a.n_writers).then(a.loc.cmp(&b.loc)));
    let top: Vec<_> = spines.into_iter().take(SPINE_TOP).collect();
    let mut kind_counts: StdHashMap<String, usize> = StdHashMap::new();
    let mut max_basic = 0usize;
    let mut max_storage = 0usize;
    for s in &top {
        *kind_counts.entry(s.kind.clone()).or_default() += 1;
        if s.kind == "basic" || s.kind == "basic_lazy" {
            max_basic = max_basic.max(s.n_writers);
        } else if s.kind == "storage" {
            max_storage = max_storage.max(s.n_writers);
        }
    }
    let dag = snap.as_ref().map(|s| analyze_dag(s, true, true));
    let hot = snap.as_ref().map(|s| hot_locations(s, 15, true));
    let khist = snap.as_ref().map(kind_histogram);
    serde_json::json!({
        "ok": true,
        "note": "finegrain structure pass after PRIMARY; walls from this pass are discarded",
        "d1_top_spines": top,
        "d1_kind_counts_in_top": kind_counts,
        "max_basic_writers": max_basic,
        "max_storage_writers": max_storage,
        "dag": dag,
        "hot_top15": hot,
        "kind_histogram": khist,
        "begin_blocked": pevm.last_begin_blocked(),
        "learn": {
            "chosen_strategy": pevm.last_learn_report().chosen_strategy,
            "chosen_win_w": pevm.last_learn_report().chosen_win_w,
            "chosen_w_need": pevm.last_learn_report().chosen_w_need,
            "unfenced_reexec": pevm.last_learn_report().unfenced_reexec,
            "selected_arms": pevm.last_learn_report().selected_arms,
        },
    })
}

fn parse_blocks() -> Vec<u64> {
    let raw = std::env::var("SPECFENCE_DEEPDIVE_BLOCKS").unwrap_or_else(|_| "3356896".into());
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<u64>()
                .unwrap_or_else(|_| panic!("bad block id {s}"))
        })
        .collect()
}

fn load_block(
    data_dir: &Path,
    number: u64,
    bytecodes: Arc<Bytecodes>,
    block_hashes: Arc<BlockHashes>,
) -> (
    Block<<PevmEthereum as PevmChain>::Transaction>,
    InMemoryStorage,
) {
    let dir = data_dir.join("blocks").join(number.to_string());
    if !dir.join("block.json").exists() {
        eprintln!("missing {}/block.json", dir.display());
        std::process::exit(2);
    }
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
    (block, storage)
}

fn main() {
    let data_dir = repo_root().join("data/ethereum");
    let (bytecodes, block_hashes) = load_shared(&data_dir);
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
    let numbers = parse_blocks();
    let instant_off = !profile_on();
    println!(
        "deepdive Soft=0 cores={cores} iters={iters} profile={} PRIMARY={} blocks={numbers:?}",
        profile_on(),
        if instant_off {
            "Instant-off wall"
        } else {
            "Instant-tax (not PRIMARY)"
        }
    );

    let mut blocks_out = Vec::new();
    for bn in numbers {
        let (block, storage) = load_block(
            &data_dir,
            bn,
            Arc::clone(&bytecodes),
            Arc::clone(&block_hashes),
        );
        let n = n_tx(&block);
        println!("=== block={bn} n_tx={n} ===");
        let mut rows = Vec::with_capacity(iters * 2);
        let mut occ_walls = Vec::new();
        let mut sf_walls = Vec::new();
        let mut sf = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        sf.reset_heat();
        sf.reset_inter_prior();
        for i in 0..iters {
            let mut occ = Pevm::with_concurrency_mode(ConcurrencyMode::Occ);
            let occ_row = run_once(&mut occ, "occ", i, &chain, &storage, &block, cores_nz);
            occ_walls.push(occ_row.wall_ms);
            rows.push(occ_row);
            let sf_row = run_once(&mut sf, "specfence", i, &chain, &storage, &block, cores_nz);
            sf_walls.push(sf_row.wall_ms);
            rows.push(sf_row);
        }
        let occ_med = median(occ_walls.clone());
        let sf_all = median(sf_walls.clone());
        let sf_cold = sf_walls.first().copied();
        let sf_reuse = if sf_walls.len() >= 2 {
            Some(median(sf_walls[1..].to_vec()))
        } else {
            None
        };
        let primary_sf = sf_reuse.unwrap_or(sf_all);
        let last_writers = sf.last_location_writers().to_vec();
        let structure = if instant_off {
            Some(structure_pass(
                &chain,
                &storage,
                &block,
                cores_nz,
                &last_writers,
            ))
        } else {
            None
        };
        let last_sf = rows.iter().rev().find(|r| r.mode == "specfence");
        let summary = serde_json::json!({
            "block": bn,
            "n_tx": n,
            "soft": 0,
            "profile_on": profile_on(),
            "primary_is": if instant_off { "Instant-off reuse median" } else { "Instant-tax reuse median (not wall PRIMARY)" },
            "occ_median_ms": occ_med,
            "sf_cold_ms": sf_cold,
            "sf_reuse_median_ms": sf_reuse,
            "sf_all_median_ms": sf_all,
            "sf_le_occ": primary_sf <= occ_med + 1e-9,
            "last_arm": last_sf.map(|r| r.chosen_strategy.clone()),
            "last_w_need": last_sf.map(|r| r.chosen_w_need),
            "last_unfenced": last_sf.map(|r| r.unfenced_reexec),
            "last_double_pay": last_sf.map(|r| r.double_pay_n),
            "last_begin_n": last_sf.map(|r| r.begin_blocked_n),
        });
        println!(
            "  summary occ_med={occ_med:.3} sf_cold={} sf_reuse={} sf_le_occ={}",
            sf_cold
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| "-".into()),
            sf_reuse
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| "-".into()),
            summary["sf_le_occ"]
        );
        blocks_out.push(serde_json::json!({
            "summary": summary,
            "rows": rows,
            "structure": structure,
        }));
        drop(sf);
        drop(storage);
    }

    let doc = serde_json::json!({
        "test": "specfence_block_deepdive Soft=0",
        "head": "0144211ae115c87eb2e80828e17e0750d3e2cf6b",
        "cores": cores,
        "iters": iters,
        "profile_on": profile_on(),
        "instant_tax_rule": "PROFILE worker-sum Instant buckets must not be added into wall",
        "blocks": blocks_out,
    });
    if let Ok(path) = std::env::var("SPECFENCE_COMPARE_JSON") {
        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent).ok();
        }
        serde_json::to_writer_pretty(File::create(&path).expect("json"), &doc).expect("write");
        println!("wrote {path}");
    } else {
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
    }
}
