//! Journal/interpreter effect-RAW stream analysis (lab opt-in).
//!
//! Runs OCC@1 (G*) + OCC@8 with FineGrain **journal** mode: Inspector logs every
//! SLOAD/SSTORE/BALANCE/EXT* (including journal-cached repeats), live write
//! ordinals, mid-tx gas. Writes:
//! - `lab/results/effect-raw-journal-stream.{json,csv}`
//! - per-core `lab/results/effect-raw-journal-stream-b{N}.json`
//!
//! Usage:
//! ```
//! cargo run -p pevm --release --config 'profile.release.lto=false' \
//!   --example specfence_effect_raw_journal_stream
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
const CORE_BLOCKS: &[u64] = &[14_689_597, 19_606_599, 19_469_097];
const NEIGHBOR_FOCUS: &[u64] = &[
    14_689_597, 14_689_599, 19_606_598, 19_606_599, 19_469_096, 19_469_097,
];
/// Collect only cores + neighbors (task scope); full segments remain in deep runner.
const COLLECT_BLOCKS: &[u64] = NEIGHBOR_FOCUS;
const OCC_CORES: &[usize] = &[1, 8];

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
    /// Gas-based depth percentiles (None→0 when unavailable).
    gas_depth_p10: f64,
    gas_depth_p50: f64,
    gas_depth_p90: f64,
    gas_depth_mean: f64,
    frac_gas_depth_lt_0_01: f64,
    n_consumers_with_gas_depth: usize,
    journal_mode: bool,
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
    abort_events_sample: Vec<pevm::AbortEvent>,
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
        journal_mode: snap.journal_mode,
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
    let user = match bn {
        14_689_597 => Some(3802),
        _ => None,
    };
    if let Some(u) = user {
        format!(
            "block {bn}: journal_RAW={} (prog={}) vs user≈{u}; prior Db-deep=593; final_RW={}; unique_pcl={}; mean_effects/pcl={:.2}; gas_depth_p50={:.4}; frac_gas≪1%={:.3}. Remaining gap: location-hash / producer-effect definition / WAW vs RAW / aborted incarnations.",
            stats.n_raw_effect_total,
            stats.n_raw_effect_program,
            stats.n_raw_final_rw,
            stats.n_unique_pcl,
            stats.mean_effects_per_pcl,
            stats.gas_depth_p50,
            stats.frac_gas_depth_lt_0_01
        )
    } else {
        format!(
            "block {bn}: effect_RAW={} prog={} hand={}; final_RW={}; fanout_prog={}; depth_p50={:.4}",
            stats.n_raw_effect_total,
            stats.n_raw_effect_program,
            stats.n_raw_effect_handler,
            stats.n_raw_final_rw,
            stats.max_program_fanout,
            stats.depth_p50
        )
    }
}

fn main() {
    let out = repo_root().join("lab/results/effect-raw-journal-stream.json");
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
            "  journal_RAW={} (prog={} hand={}) final_RW={} mean/pcl={:.2} depth_p50={:.4} gas_p50={:.4} frac_gas≪1%={:.3} redo_saved={:.1} wait_added={:.1}",
            effect.n_raw_effect_total,
            effect.n_raw_effect_program,
            effect.n_raw_effect_handler,
            effect.n_raw_final_rw,
            effect.mean_effects_per_pcl,
            effect.depth_p50,
            effect.gas_depth_p50,
            effect.frac_gas_depth_lt_0_01,
            effect.redo_saved,
            effect.wait_added
        );

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

        if CORE_BLOCKS.contains(&bn) || NEIGHBOR_FOCUS.contains(&bn) {
            let mut sample = snap.effect_edges.clone();
            sample.truncate(3000);
            let mut cfc = snap.consumer_first_cross.clone();
            cfc.truncate(2000);
            let dive = DeepDive {
                block: bn,
                summary: summary.clone(),
                sample_effect_edges: sample,
                sample_consumer_first_cross: cfc,
                abort_events_sample: snap.abort_events.iter().take(200).cloned().collect(),
                gap_note: gap_note(bn, &effect),
            };
            let deep_path = out.with_file_name(format!("effect-raw-journal-stream-b{bn}.json"));
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
        "user≈3802; prior final-RW=449; prior Db-deep=593; this run=journal stream".into(),
    );
    let payload = Out {
        generated: chrono_lite_now(),
        method_notes: vec![
            "OCC@1 preferred for clean G* effect stream (serial-forced parallel path).".into(),
            "Journal mode: SpecFenceInspector step logs SLOAD/SSTORE/BALANCE/EXT*/SELFBALANCE.".into(),
            "producer_effect_k = live SSTORE ordinal (fallback finalize for Basic/CodeHash).".into(),
            "depth_frac_effects = first_program_cross_k / total_journal_reads; depth_frac_gas = gas_used/gas_limit.".into(),
            "M-A/M-D proxies prefer gas depth when present.".into(),
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
        "seg,block,n_tx,occ1_tps,occ8_tps,occ8_abort,n_raw_effect,n_prog,n_hand,n_final_rw,unique_pcl,mean_eff_per_pcl,depth_p10,depth_p50,depth_p90,frac_lt_1pct,gas_p10,gas_p50,gas_p90,frac_gas_lt_1pct,redo_saved,wait_added,eff_prog_path,final_rw_chain,fanout"
    )
    .unwrap();
    for s in &summaries {
        let e = &s.effect;
        writeln!(
            csv,
            "{},{},{},{:.1},{:.1},{:.4},{},{},{},{},{},{:.3},{:.5},{:.5},{:.5},{:.4},{:.5},{:.5},{:.5},{:.4},{:.2},{:.2},{},{},{}",
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
            e.depth_p10,
            e.depth_p50,
            e.depth_p90,
            e.frac_depth_lt_0_01,
            e.gas_depth_p10,
            e.gas_depth_p50,
            e.gas_depth_p90,
            e.frac_gas_depth_lt_0_01,
            e.redo_saved,
            e.wait_added,
            e.longest_effect_program_path,
            e.longest_final_rw_chain,
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
