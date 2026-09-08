//! Deeper effect-RAW instrumentation pass (lab opt-in).
//!
//! Extends journal stream with: account-grain structured observes + Wait/Bind EV
//! proxies, producer readiness at discovery, OCC@8 vs OCC@1 discovery timing,
//! WAW-only multi-writer spurious HotLocal Wait proxy. Does **not** implement
//! choose_action / HotSet replacement. Writes:
//! - `lab/results/effect-raw-deeper.{json,csv}`
//! - per-block `lab/results/effect-raw-deeper-b{N}.json`
//!
//! Usage:
//! ```
//! cargo run -p pevm --release --config 'profile.release.lto=false' \
//!   --example specfence_effect_raw_deeper
//! ```

#![allow(missing_docs)]

use std::{
    collections::HashMap as StdHashMap,
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
    BlockHashes, BuildSuffixHasher, Bytecodes, ConcurrencyMode, EvmAccount, FineGrainSnapshot,
    InMemoryStorage, Pevm, analyze_dag, classify_raw_edges, effect_raw_longest_chain,
    effect_raw_max_fanout, estimate_ma_md, filter_effect_edges, hot_locations, kind_histogram,
    chain::{PevmChain, PevmEthereum},
};
use serde::Serialize;

const SEGMENT_A: &[u64] = &[14_689_595, 14_689_596, 14_689_597, 14_689_598, 14_689_599];
const SEGMENT_B: &[u64] = &[19_606_597, 19_606_598, 19_606_599, 19_606_600];
const SEGMENT_C: &[u64] = &[19_469_096, 19_469_097, 19_469_098, 19_469_099];
const COLLECT_BLOCKS: &[u64] = &[
    14_689_597, 19_606_598, 19_606_599, 19_469_096, 19_469_097,
];

#[derive(Clone, Serialize)]
struct ModeTiming {
    mode: String,
    cores: usize,
    elapsed_ms: f64,
    tps: f64,
    ok: bool,
    error: Option<String>,
    occ_aborts: usize,
    abort_rate: f64,
    evm_entries: usize,
    reexec_entry_frac: f64,
    max_incarnation: usize,
}

#[derive(Clone, Serialize)]
struct EffectRawStats {
    n_raw_effect_total: usize,
    n_raw_effect_program: usize,
    n_raw_effect_handler: usize,
    /// Final-RW pair RAW (for gap vs user / prior collector).
    n_raw_final_rw: usize,
    n_raw_final_program: usize,
    longest_effect_program_path: usize,
    longest_final_rw_chain: usize,
    max_program_fanout: usize,
    depth_p10: f64,
    depth_p50: f64,
    depth_p90: f64,
    depth_mean: f64,
    frac_depth_lt_0_01: f64,
    ma_redo_cost: f64,
    md_redo_cost: f64,
    redo_saved: f64,
    wait_added: f64,
    n_consumers_with_program_cross: usize,
    /// Unique (producer,consumer,location) among effect edges.
    n_unique_pcl: usize,
    /// Mean effect edges per unique final-style RAW pair (pcl).
    mean_effects_per_pcl: f64,
    /// Legacy gas/limit depth percentiles.
    gas_depth_p10: f64,
    gas_depth_p50: f64,
    gas_depth_p90: f64,
    gas_depth_mean: f64,
    frac_gas_depth_lt_0_01: f64,
    n_consumers_with_gas_depth: usize,
    /// Preferred gross-work depth: gas_at_cross / tx_gas_used.
    gross_work_depth_p10: f64,
    gross_work_depth_p50: f64,
    gross_work_depth_p90: f64,
    gross_work_depth_mean: f64,
    frac_gross_work_lt_0_01: f64,
    n_consumers_with_gross_work: usize,
    opcode_depth_p50: f64,
    frac_opcode_lt_0_01: f64,
    /// Stream diagnostics from FineGrainSnapshot.stream_diag.
    journal_reads: usize,
    journal_reads_cross: usize,
    journal_reads_no_prior: usize,
    sload_account_grain_cross: usize,
    journal_sstore: usize,
    journal_account_writes: usize,
    journal_mode: bool,
    // --- deeper pass ---
    account_grain_sload: usize,
    account_grain_balance: usize,
    account_grain_ext: usize,
    account_grain_would_wait: usize,
    account_grain_would_bind: usize,
    slot_and_account_both: usize,
    account_grain_would_wait_frac: f64,
    /// Producer readiness histogram on location RAW edges (validated/executed/data/estimate/running/...).
    producer_ready_hist: StdHashMap<String, usize>,
    producer_mv_hist: StdHashMap<String, usize>,
    /// First program-cross producer readiness (last incarnation per consumer).
    first_cross_ready_hist: StdHashMap<String, usize>,
    first_cross_ready_data_or_done_frac: f64,
    first_cross_ready_running_or_estimate_frac: f64,
    /// Mean / p50 gross-work at first program cross (same as gw_* but echoed for timing compare).
    discovery_gw_p50: f64,
    discovery_incarnation_mean: f64,
    /// Abort × discovery depth correlation (OCC@8 primarily).
    n_aborts: usize,
    abort_consumers_with_prior_discovery: usize,
    mean_gw_depth_at_abort_consumer: f64,
    mean_discovery_incarnation_of_aborted: f64,
    waw_only_multi_writer_locs: usize,
    multi_writer_locs: usize,
    spurious_hotlocal_writer_count_waits: usize,
    max_waw_chain_no_raw: usize,
    spurious_hotlocal_frac_of_multi_writer: f64,
    waw_pairs: usize,
    waw_pairs_no_intervening_raw: usize,
    multi_writer_no_readers: usize,
    waw_pairs_no_intervening_frac: f64,
    /// Edge-level (all RAW instances) ready fractions — includes aborted incarnations.
    edge_ready_done_frac: f64,
    edge_ready_waitish_frac: f64,
}

#[derive(Clone, Serialize)]
struct BlockSummary {
    segment: String,
    block: u64,
    n_tx: usize,
    gas_used: u64,
    serial_forced_occ1: ModeTiming,
    occ8: ModeTiming,
    /// Stats from OCC@1 deep snapshot (clean G* preference).
    effect: EffectRawStats,
    /// OCC@8 deep stats when available (abort incarnation correlation).
    effect_occ8: Option<EffectRawStats>,
    kind_hist: StdHashMap<String, usize>,
}

#[derive(Serialize)]
struct DeepDive {
    block: u64,
    summary: BlockSummary,
    sample_effect_edges: Vec<pevm::RawEffectEdge>,
    sample_consumer_first_cross: Vec<pevm::ConsumerFirstCross>,
    sample_account_grain: Vec<pevm::AccountGrainObserve>,
    abort_events_sample: Vec<pevm::AbortEvent>,
    /// OCC@8 first-cross / readiness samples for timing compare.
    occ8_sample_consumer_first_cross: Vec<pevm::ConsumerFirstCross>,
    occ8_abort_events_sample: Vec<pevm::AbortEvent>,
    gap_note: String,
}

struct LoadedBlock {
    number: u64,
    block: Block<<PevmEthereum as PevmChain>::Transaction>,
    storage: InMemoryStorage,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn segment_of(bn: u64) -> &'static str {
    if SEGMENT_A.contains(&bn) {
        "A"
    } else if SEGMENT_B.contains(&bn) {
        "B"
    } else if SEGMENT_C.contains(&bn) {
        "C"
    } else {
        "?"
    }
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
        eprintln!("missing snapshot {number}");
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

fn timing_from(mode: &str, cores: usize, n: usize, elapsed_ms: f64, tps: f64, ok: bool, err: Option<String>, pevm: &Pevm, snap: Option<&FineGrainSnapshot>) -> ModeTiming {
    let m = pevm.last_specfence_metrics();
    let max_inc = snap
        .map(|s| s.final_incarnations.iter().copied().max().unwrap_or(0))
        .unwrap_or(0);
    let evm_entries = if mode.starts_with("occ") { m.evm_entries } else { 0 };
    ModeTiming {
        mode: mode.into(),
        cores,
        elapsed_ms,
        tps,
        ok,
        error: err,
        occ_aborts: if mode.starts_with("occ") { m.occ_aborts } else { 0 },
        abort_rate: if n == 0 || !mode.starts_with("occ") {
            0.0
        } else {
            m.occ_aborts as f64 / n as f64
        },
        evm_entries,
        reexec_entry_frac: if evm_entries == 0 {
            0.0
        } else {
            (evm_entries.saturating_sub(n)) as f64 / evm_entries as f64
        },
        max_incarnation: max_inc,
    }
}

fn effect_stats(snap: &FineGrainSnapshot) -> EffectRawStats {
    let edges = filter_effect_edges(snap, true, true);
    let n_total = edges.len();
    let n_prog = edges.iter().filter(|e| e.class == "program").count();
    let n_hand = n_total.saturating_sub(n_prog);
    let raw_final = classify_raw_edges(snap, true, true);
    let n_final = raw_final.len();
    let n_final_prog = raw_final.iter().filter(|e| e.class == "program").count();
    let dag = analyze_dag(snap, true, true);
    let ma_md = estimate_ma_md(snap, &edges, snap.n_tx);

    use std::collections::{HashMap, HashSet};
    let mut pcl: HashSet<(usize, usize, u64)> = HashSet::new();
    for e in &edges {
        pcl.insert((e.producer_tx, e.consumer_tx, e.location));
    }
    let n_pcl = pcl.len();
    let mean_per = if n_pcl == 0 {
        0.0
    } else {
        n_total as f64 / n_pcl as f64
    };

    let mut best: HashMap<usize, &pevm::ConsumerFirstCross> = HashMap::new();
    for c in &snap.consumer_first_cross {
        best.entry(c.tx_idx)
            .and_modify(|e| {
                if c.incarnation >= e.incarnation {
                    *e = c;
                }
            })
            .or_insert(c);
    }
    let mut gas_depths: Vec<f64> = best.values().filter_map(|c| c.depth_frac_gas).collect();
    gas_depths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n_g = gas_depths.len();
    let gmean = if n_g == 0 {
        0.0
    } else {
        gas_depths.iter().sum::<f64>() / n_g as f64
    };
    let glt = if n_g == 0 {
        0.0
    } else {
        gas_depths.iter().filter(|&&d| d < 0.01).count() as f64 / n_g as f64
    };
    let mut gw_depths: Vec<f64> = best
        .values()
        .filter_map(|c| c.depth_frac_gross_work)
        .collect();
    gw_depths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n_gw = gw_depths.len();
    let gwmean = if n_gw == 0 {
        0.0
    } else {
        gw_depths.iter().sum::<f64>() / n_gw as f64
    };
    let gwlt = if n_gw == 0 {
        0.0
    } else {
        gw_depths.iter().filter(|&&d| d < 0.01).count() as f64 / n_gw as f64
    };
    let mut op_depths: Vec<f64> = best.values().filter_map(|c| c.depth_frac_opcode).collect();
    op_depths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n_op = op_depths.len();
    let oplt = if n_op == 0 {
        0.0
    } else {
        op_depths.iter().filter(|&&d| d < 0.01).count() as f64 / n_op as f64
    };
    let mut effect_depths: Vec<f64> = best
        .values()
        .filter_map(|c| c.depth_frac_effects)
        .collect();
    effect_depths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n_e = effect_depths.len();
    let emean = if n_e == 0 {
        0.0
    } else {
        effect_depths.iter().sum::<f64>() / n_e as f64
    };
    let elt = if n_e == 0 {
        0.0
    } else {
        effect_depths.iter().filter(|&&d| d < 0.01).count() as f64 / n_e as f64
    };
    let d = &snap.stream_diag;

    let mut producer_ready_hist: StdHashMap<String, usize> = StdHashMap::new();
    let mut producer_mv_hist: StdHashMap<String, usize> = StdHashMap::new();
    for e in &edges {
        if let Some(r) = &e.producer_ready {
            *producer_ready_hist.entry(r.clone()).or_default() += 1;
        }
        if let Some(m) = &e.producer_mv {
            *producer_mv_hist.entry(m.clone()).or_default() += 1;
        }
    }

    let mut first_cross_ready_hist: StdHashMap<String, usize> = StdHashMap::new();
    let mut n_fc_ready = 0usize;
    let mut n_fc_done = 0usize;
    let mut n_fc_waitish = 0usize;
    let mut inc_sum = 0.0f64;
    let mut inc_n = 0usize;
    for c in best.values() {
        inc_sum += c.incarnation as f64;
        inc_n += 1;
        if let Some(r) = &c.producer_ready_at_discovery {
            *first_cross_ready_hist.entry(r.clone()).or_default() += 1;
            n_fc_ready += 1;
            if matches!(r.as_str(), "validated" | "executed" | "data") {
                n_fc_done += 1;
            }
            if matches!(r.as_str(), "running" | "estimate" | "aborting") {
                n_fc_waitish += 1;
            }
        }
    }
    let first_cross_ready_data_or_done_frac = if n_fc_ready == 0 {
        0.0
    } else {
        n_fc_done as f64 / n_fc_ready as f64
    };
    let first_cross_ready_running_or_estimate_frac = if n_fc_ready == 0 {
        0.0
    } else {
        n_fc_waitish as f64 / n_fc_ready as f64
    };
    let discovery_incarnation_mean = if inc_n == 0 { 0.0 } else { inc_sum / inc_n as f64 };

    // Abort correlation: aborted txs that already had a program-cross discovery.
    let n_aborts = snap.abort_events.len();
    let mut abort_with_disc = 0usize;
    let mut gw_at_abort = 0.0f64;
    let mut disc_inc_abort = 0.0f64;
    let mut abort_corr_n = 0usize;
    for a in &snap.abort_events {
        if let Some(c) = best.get(&a.tx_idx) {
            if c.first_program_cross_k.is_some() {
                abort_with_disc += 1;
                if let Some(d) = c.depth_frac_gross_work {
                    gw_at_abort += d;
                    disc_inc_abort += c.incarnation as f64;
                    abort_corr_n += 1;
                }
            }
        }
    }
    let mean_gw_depth_at_abort_consumer = if abort_corr_n == 0 {
        0.0
    } else {
        gw_at_abort / abort_corr_n as f64
    };
    let mean_discovery_incarnation_of_aborted = if abort_corr_n == 0 {
        0.0
    } else {
        disc_inc_abort / abort_corr_n as f64
    };

    let acct_total = d.sload_account_grain_cross;
    let account_grain_would_wait_frac = if acct_total == 0 {
        0.0
    } else {
        d.account_grain_would_wait as f64 / acct_total as f64
    };

    let mut edge_done = 0usize;
    let mut edge_waitish = 0usize;
    let mut edge_n = 0usize;
    for e in &edges {
        if let Some(r) = &e.producer_ready {
            edge_n += 1;
            if matches!(r.as_str(), "validated" | "executed" | "data") {
                edge_done += 1;
            }
            if matches!(r.as_str(), "running" | "estimate" | "aborting") {
                edge_waitish += 1;
            }
        }
    }
    let edge_ready_done_frac = if edge_n == 0 { 0.0 } else { edge_done as f64 / edge_n as f64 };
    let edge_ready_waitish_frac = if edge_n == 0 { 0.0 } else { edge_waitish as f64 / edge_n as f64 };
    let waw_pairs_no_intervening_frac = if d.waw_pairs == 0 {
        0.0
    } else {
        d.waw_pairs_no_intervening_raw as f64 / d.waw_pairs as f64
    };

    let spurious_frac = if d.multi_writer_locs == 0 {
        0.0
    } else {
        d.spurious_hotlocal_writer_count_waits as f64 / d.multi_writer_locs as f64
    };

    EffectRawStats {
        n_raw_effect_total: n_total,
        n_raw_effect_program: n_prog,
        n_raw_effect_handler: n_hand,
        n_raw_final_rw: n_final,
        n_raw_final_program: n_final_prog,
        longest_effect_program_path: effect_raw_longest_chain(&edges, true),
        longest_final_rw_chain: dag.longest_chain,
        max_program_fanout: effect_raw_max_fanout(&edges, true),
        depth_p10: pevm::percentile_f64(&effect_depths, 0.10),
        depth_p50: pevm::percentile_f64(&effect_depths, 0.50),
        depth_p90: pevm::percentile_f64(&effect_depths, 0.90),
        depth_mean: emean,
        frac_depth_lt_0_01: elt,
        ma_redo_cost: ma_md.ma_redo_cost,
        md_redo_cost: ma_md.md_redo_cost,
        redo_saved: ma_md.redo_saved,
        wait_added: ma_md.wait_added,
        n_consumers_with_program_cross: ma_md.n_consumers_with_program_cross,
        n_unique_pcl: n_pcl,
        mean_effects_per_pcl: mean_per,
        gas_depth_p10: pevm::percentile_f64(&gas_depths, 0.10),
        gas_depth_p50: pevm::percentile_f64(&gas_depths, 0.50),
        gas_depth_p90: pevm::percentile_f64(&gas_depths, 0.90),
        gas_depth_mean: gmean,
        frac_gas_depth_lt_0_01: glt,
        n_consumers_with_gas_depth: n_g,
        gross_work_depth_p10: pevm::percentile_f64(&gw_depths, 0.10),
        gross_work_depth_p50: pevm::percentile_f64(&gw_depths, 0.50),
        gross_work_depth_p90: pevm::percentile_f64(&gw_depths, 0.90),
        gross_work_depth_mean: gwmean,
        frac_gross_work_lt_0_01: gwlt,
        n_consumers_with_gross_work: n_gw,
        opcode_depth_p50: pevm::percentile_f64(&op_depths, 0.50),
        frac_opcode_lt_0_01: oplt,
        journal_reads: d.journal_reads,
        journal_reads_cross: d.journal_reads_cross,
        journal_reads_no_prior: d.journal_reads_no_prior_writer,
        sload_account_grain_cross: d.sload_account_grain_cross,
        journal_sstore: d.journal_sstore,
        journal_account_writes: d.journal_account_writes,
        journal_mode: snap.journal_mode,
        account_grain_sload: d.account_grain_sload,
        account_grain_balance: d.account_grain_balance,
        account_grain_ext: d.account_grain_ext,
        account_grain_would_wait: d.account_grain_would_wait,
        account_grain_would_bind: d.account_grain_would_bind,
        slot_and_account_both: d.slot_and_account_both,
        account_grain_would_wait_frac,
        producer_ready_hist,
        producer_mv_hist,
        first_cross_ready_hist,
        first_cross_ready_data_or_done_frac,
        first_cross_ready_running_or_estimate_frac,
        discovery_gw_p50: pevm::percentile_f64(&gw_depths, 0.50),
        discovery_incarnation_mean,
        n_aborts,
        abort_consumers_with_prior_discovery: abort_with_disc,
        mean_gw_depth_at_abort_consumer,
        mean_discovery_incarnation_of_aborted,
        waw_only_multi_writer_locs: d.waw_only_multi_writer_locs,
        multi_writer_locs: d.multi_writer_locs,
        spurious_hotlocal_writer_count_waits: d.spurious_hotlocal_writer_count_waits,
        max_waw_chain_no_raw: d.max_waw_chain_no_raw,
        spurious_hotlocal_frac_of_multi_writer: spurious_frac,
        waw_pairs: d.waw_pairs,
        waw_pairs_no_intervening_raw: d.waw_pairs_no_intervening_raw,
        multi_writer_no_readers: d.multi_writer_no_readers,
        waw_pairs_no_intervening_frac,
        edge_ready_done_frac,
        edge_ready_waitish_frac,
    }
}

fn run_occ_deep(
    chain: &PevmEthereum,
    loaded: &LoadedBlock,
    cores: usize,
) -> (ModeTiming, Option<FineGrainSnapshot>) {
    let n = n_tx(&loaded.block);
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::Occ);
    pevm.reset_heat();
    pevm.set_finegrain_journal(true);
    let cores_nz = NonZeroUsize::new(cores.max(1)).unwrap();
    let mut block = loaded.block.clone();
    if block.header.gas_used < 4_000_000 {
        block.header.gas_used = 4_000_000;
    }
    let t0 = Instant::now();
    let result = pevm.execute(chain, &loaded.storage, &block, cores_nz, false);
    let elapsed = t0.elapsed().as_secs_f64();
    let elapsed_ms = elapsed * 1000.0;
    let tps = if elapsed > 0.0 { n as f64 / elapsed } else { 0.0 };
    let snap = pevm.take_finegrain_snapshot();
    let timing = match &result {
        Ok(_) => timing_from("occ_journal", cores, n, elapsed_ms, tps, true, None, &pevm, snap.as_ref()),
        Err(e) => timing_from(
            "occ_journal",
            cores,
            n,
            elapsed_ms,
            tps,
            false,
            Some(format!("{e}")),
            &pevm,
            snap.as_ref(),
        ),
    };
    (timing, snap)
}

fn gap_note(bn: u64, stats: &EffectRawStats) -> String {
    format!(
        "block {bn}: location_RAW={} (prog={} hand={}); acct_grain={} (sload={} bal={} ext={}) would_wait={} ({:.3}) would_bind={}; producer_ready_done_frac={:.3} waitish_frac={:.3}; gw_p50={:.4}; waw_only_mw={} / multi_writer={} spurious_hotlocal={:.3}; aborts={} abort_w_disc={}. Plant-observed only; v3 control law frozen (design).",
        stats.n_raw_effect_total,
        stats.n_raw_effect_program,
        stats.n_raw_effect_handler,
        stats.sload_account_grain_cross,
        stats.account_grain_sload,
        stats.account_grain_balance,
        stats.account_grain_ext,
        stats.account_grain_would_wait,
        stats.account_grain_would_wait_frac,
        stats.account_grain_would_bind,
        stats.first_cross_ready_data_or_done_frac,
        stats.first_cross_ready_running_or_estimate_frac,
        stats.gross_work_depth_p50,
        stats.waw_only_multi_writer_locs,
        stats.multi_writer_locs,
        stats.spurious_hotlocal_frac_of_multi_writer,
        stats.n_aborts,
        stats.abort_consumers_with_prior_discovery,
    )
}

fn main() {
    let out = repo_root().join("lab/results/effect-raw-deeper.json");
    let all_blocks: Vec<u64> = COLLECT_BLOCKS.to_vec();

    let data_dir = repo_root().join("data/ethereum");
    let chain = PevmEthereum::mainnet();
    let (bytecodes, block_hashes) = load_shared(&data_dir);

    let mut summaries = Vec::new();
    let mut deep_dives = Vec::new();

    for &bn in &all_blocks {
        let Some(loaded) = load_block(&data_dir, bn, bytecodes.clone(), block_hashes.clone()) else {
            continue;
        };
        eprintln!(
            "=== block {bn} seg={} n_tx={} gas={} ===",
            segment_of(bn),
            n_tx(&loaded.block),
            loaded.block.header.gas_used
        );

        let (occ1, snap1) = run_occ_deep(&chain, &loaded, 1);
        eprintln!(
            "  occ@1 journal: {:.1}ms TPS={:.0} aborts={} edges={} journal={} ok={}",
            occ1.elapsed_ms,
            occ1.tps,
            occ1.occ_aborts,
            snap1.as_ref().map(|s| s.effect_edges.len()).unwrap_or(0),
            snap1.as_ref().map(|s| s.journal_mode).unwrap_or(false),
            occ1.ok
        );

        let (occ8, snap8) = run_occ_deep(&chain, &loaded, 8);
        eprintln!(
            "  occ@8 journal: {:.1}ms TPS={:.0} aborts={} edges={} ok={}",
            occ8.elapsed_ms,
            occ8.tps,
            occ8.occ_aborts,
            snap8.as_ref().map(|s| s.effect_edges.len()).unwrap_or(0),
            occ8.ok
        );

        let Some(snap) = snap1 else {
            eprintln!("  WARN: no OCC@1 deep snapshot");
            continue;
        };
        let effect = effect_stats(&snap);
        let effect_occ8 = snap8.as_ref().map(effect_stats);
        eprintln!(
            "  RAW={} acct_grain={} would_wait={}/{:.2} would_bind={} edge_ready_done={:.2} waitish={:.2} gw_p50={:.3} waw_pairs={}/{} (no_raw_frac={:.2}) mw_no_readers={} aborts={}",
            effect.n_raw_effect_total,
            effect.sload_account_grain_cross,
            effect.account_grain_would_wait,
            effect.account_grain_would_wait_frac,
            effect.account_grain_would_bind,
            effect.edge_ready_done_frac,
            effect.edge_ready_waitish_frac,
            effect.gross_work_depth_p50,
            effect.waw_pairs_no_intervening_raw,
            effect.waw_pairs,
            effect.waw_pairs_no_intervening_frac,
            effect.multi_writer_no_readers,
            effect.n_aborts
        );
        if let Some(ref e8) = effect_occ8 {
            eprintln!(
                "  occ8: RAW={} acct_would_wait={}/{:.2} edge_ready_done={:.2} waitish={:.2} fc_done={:.2} gw_p50={:.3} aborts={} abort_w_disc={} mean_gw_abort={:.3} waw_pairs_noraw={}/{}",
                e8.n_raw_effect_total,
                e8.account_grain_would_wait,
                e8.account_grain_would_wait_frac,
                e8.edge_ready_done_frac,
                e8.edge_ready_waitish_frac,
                e8.first_cross_ready_data_or_done_frac,
                e8.gross_work_depth_p50,
                e8.n_aborts,
                e8.abort_consumers_with_prior_discovery,
                e8.mean_gw_depth_at_abort_consumer,
                e8.waw_pairs_no_intervening_raw,
                e8.waw_pairs
            );
        }

        let summary = BlockSummary {
            segment: segment_of(bn).into(),
            block: bn,
            n_tx: n_tx(&loaded.block),
            gas_used: loaded.block.header.gas_used,
            serial_forced_occ1: occ1,
            occ8,
            effect: effect.clone(),
            effect_occ8,
            kind_hist: kind_histogram(&snap),
        };

        if COLLECT_BLOCKS.contains(&bn) {
            let mut sample = snap.effect_edges.clone();
            sample.truncate(3000);
            let mut cfc = snap.consumer_first_cross.clone();
            cfc.truncate(2000);
            let mut ag = snap.account_grain_edges.clone();
            ag.truncate(3000);
            let mut cfc8 = snap8
                .as_ref()
                .map(|s| s.consumer_first_cross.clone())
                .unwrap_or_default();
            cfc8.truncate(2000);
            let abort8 = snap8
                .as_ref()
                .map(|s| s.abort_events.iter().take(200).cloned().collect())
                .unwrap_or_default();
            let dive = DeepDive {
                block: bn,
                summary: summary.clone(),
                sample_effect_edges: sample,
                sample_consumer_first_cross: cfc,
                sample_account_grain: ag,
                abort_events_sample: snap.abort_events.iter().take(200).cloned().collect(),
                occ8_sample_consumer_first_cross: cfc8,
                occ8_abort_events_sample: abort8,
                gap_note: gap_note(bn, &effect),
            };
            let deep_path = out.with_file_name(format!("effect-raw-deeper-b{bn}.json"));
            fs::create_dir_all(deep_path.parent().unwrap()).ok();
            let mut f = File::create(&deep_path).expect("deep json");
            serde_json::to_writer_pretty(&mut f, &dive).unwrap();
            eprintln!("  wrote {}", deep_path.display());
            deep_dives.push(bn);
        }

        // Also write a slim csv row file incrementally via summaries
        let _ = hot_locations(&snap, 5, true);
        summaries.push(summary);
    }

    #[derive(Serialize)]
    struct Out {
        generated: String,
        method_notes: Vec<String>,
        user_gap_reference: StdHashMap<String, String>,
        blocks: Vec<BlockSummary>,
        deep_dive_blocks: Vec<u64>,
    }
    let mut user_gap = StdHashMap::new();
    user_gap.insert(
        "14689597".into(),
        "plant-observed journal RAW (this run); prior final-RW≈449 / Db-deep≈593 for history only — not targets".into(),
    );
    let payload = Out {
        generated: chrono_lite_now(),
        method_notes: vec![
            "Deeper pass: account-grain structured observes + Wait/Bind EV proxies; producer readiness at discovery; OCC@8 vs OCC@1 timing; WAW-only HotLocal proxy.".into(),
            "OCC@1 preferred for clean G* effect stream; OCC@8 for abort×discovery timing.".into(),
            "Primary RAW remains location last-writer; account-grain is diag only (not a control-law target).".into(),
            "producer_ready ∈ {validated,executed,data,estimate,running,aborting,unknown} sampled live via MvMemory+Scheduler attach.".into(),
            "gross_work depth (preferred) = gas_at_cross / tx_gas_used.".into(),
            "Control law v3 stays FROZEN as design; this run may only amend via note, not choose_action code.".into(),
            "Production default unchanged: finegrain_journal off (Handler::run, zero overhead).".into(),
        ],
        user_gap_reference: user_gap,
        blocks: summaries.clone(),
        deep_dive_blocks: deep_dives,
    };
    fs::create_dir_all(out.parent().unwrap()).ok();
    let mut f = File::create(&out).expect("out json");
    serde_json::to_writer_pretty(&mut f, &payload).unwrap();
    eprintln!("wrote {}", out.display());

    // CSV
    let csv_path = out.with_extension("csv");
    let mut csv = File::create(&csv_path).expect("csv");
    writeln!(
        csv,
        "seg,block,n_tx,occ1_tps,occ8_tps,occ8_abort,n_raw_effect,n_prog,n_hand,n_final_rw,unique_pcl,mean_eff_per_pcl,gw_p50,opcode_p50,acct_grain,acct_sload,acct_would_wait,acct_would_bind,acct_wait_frac,ready_done_frac,waitish_frac,waw_only_mw,multi_writer,spurious_hl_frac,occ8_gw_p50,occ8_ready_done,occ8_waitish,occ8_aborts,occ8_abort_w_disc,occ8_mean_gw_abort,fanout"
    )
    .unwrap();
    for s in &summaries {
        let e = &s.effect;
        let e8 = s.effect_occ8.as_ref();
        writeln!(
            csv,
            "{},{},{},{:.1},{:.1},{:.4},{},{},{},{},{},{:.3},{:.5},{:.5},{},{},{},{},{:.4},{:.4},{:.4},{},{},{:.4},{:.5},{:.4},{:.4},{},{},{:.5},{}",
            s.segment,
            s.block,
            s.n_tx,
            s.serial_forced_occ1.tps,
            s.occ8.tps,
            s.occ8.abort_rate,
            e.n_raw_effect_total,
            e.n_raw_effect_program,
            e.n_raw_effect_handler,
            e.n_raw_final_rw,
            e.n_unique_pcl,
            e.mean_effects_per_pcl,
            e.gross_work_depth_p50,
            e.opcode_depth_p50,
            e.sload_account_grain_cross,
            e.account_grain_sload,
            e.account_grain_would_wait,
            e.account_grain_would_bind,
            e.account_grain_would_wait_frac,
            e.first_cross_ready_data_or_done_frac,
            e.first_cross_ready_running_or_estimate_frac,
            e.waw_only_multi_writer_locs,
            e.multi_writer_locs,
            e.spurious_hotlocal_frac_of_multi_writer,
            e8.map(|x| x.gross_work_depth_p50).unwrap_or(0.0),
            e8.map(|x| x.first_cross_ready_data_or_done_frac).unwrap_or(0.0),
            e8.map(|x| x.first_cross_ready_running_or_estimate_frac).unwrap_or(0.0),
            e8.map(|x| x.n_aborts).unwrap_or(0),
            e8.map(|x| x.abort_consumers_with_prior_discovery).unwrap_or(0),
            e8.map(|x| x.mean_gw_depth_at_abort_consumer).unwrap_or(0.0),
            e.max_program_fanout
        )
        .unwrap();
    }
    eprintln!("wrote {}", csv_path.display());
}

fn chrono_lite_now() -> String {
    // UTC now; parent report converts to Asia/Shanghai.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix_utc={secs}")
}
