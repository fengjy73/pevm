//! Fine-grain serial + OCC runtime analysis for SpecFence lab blocks.
//!
//! Produces:
//! - `lab/results/mainnet-serial-occ-finegrain.json` (all blocks summary + deep dives)
//! - `lab/results/mainnet-serial-occ-finegrain.csv` (summary rows)
//! - per-block deep JSON for 19807137, 15199017, 19434587
//!
//! Usage:
//! ```
//! cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_finegrain_analysis -- \
//!   --out lab/results/mainnet-serial-occ-finegrain.json
//! ```

#![allow(missing_docs)]

use std::{
    fs::{self, File},
    io::{BufReader, Write},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use hashbrown::HashMap;
use std::collections::HashMap as StdHashMap;

use alloy_rpc_types_eth::Block;
use flate2::bufread::GzDecoder;
use pevm::{
    BlockHashes, BuildSuffixHasher, Bytecodes, ConcurrencyMode, EvmAccount, FineGrainSnapshot,
    InMemoryStorage, Pevm, analyze_dag, hot_locations, kind_histogram,
    chain::{PevmChain, PevmEthereum},
};
use serde::Serialize;

const DEFAULT_BLOCKS: &[u64] = &[
    19_807_137, 14_683_600, 13_217_637, 14_383_540, 15_199_017, 14_029_313, 19_434_587,
];
const DEEP_BLOCKS: &[u64] = &[19_807_137, 15_199_017, 19_434_587];
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
    cascade_validations_scheduled: usize,
    evm_entries: usize,
    full_restart: usize,
    /// Sum of (final_incarnation+1) over txs ≈ execution attempts.
    total_incarnations: usize,
    max_incarnation: usize,
    txs_with_abort: usize,
    abort_events: usize,
    mean_cascade_on_abort: f64,
    /// Proxy: (evm_entries - n_tx) / max(evm_entries,1) = reexec share of interpreter entries.
    reexec_entry_frac: f64,
}

#[derive(Clone, Serialize)]
struct BlockSummary {
    block: u64,
    n_tx: usize,
    gas_used: u64,
    serial: ModeTiming,
    occ: Vec<ModeTiming>,
    /// Effective G* (beneficiary + basic_lazy excluded — matches speculative OCC path).
    dag: pevm::DagStats,
    /// Raw G* including lazy sender/recipient WW chains (often over-serializes).
    dag_with_lazy: pevm::DagStats,
    kind_hist: StdHashMap<String, usize>,
    hot_top20: Vec<pevm::HotLocation>,
    /// Mean |R|, |W| per tx from final incarnation.
    mean_reads: f64,
    mean_writes: f64,
    /// SpecFence v7 reference (from status notes) for why-v7-lost context.
    v7_sf_occ_ratio: Option<f64>,
    v7_wait_hard: Option<usize>,
    v7_inspector_steps: Option<usize>,
    v7_lean_mode_txs: Option<usize>,
    v7_absolute_jump_applied: Option<usize>,
}

#[derive(Serialize)]
struct DeepDive {
    block: u64,
    summary: BlockSummary,
    /// Condensed per-tx: idx, inc, n_r, n_w (full RW hashes omitted unless --full-rw).
    per_tx: Vec<PerTxRow>,
    abort_events_sample: Vec<pevm::AbortEvent>,
    multi_writer_chains_top20: Vec<MultiWriterChain>,
}

#[derive(Clone, Serialize)]
struct PerTxRow {
    tx_idx: usize,
    incarnation: usize,
    n_reads: usize,
    n_writes: usize,
}

#[derive(Clone, Serialize)]
struct MultiWriterChain {
    location: u64,
    kind: String,
    writers: Vec<usize>,
    n_readers: usize,
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

/// v7 SpecFence@8 reference table (from lab/notes/mainnet-sweep-v7-status.md).
fn v7_refs(block: u64) -> (Option<f64>, Option<usize>, Option<usize>, Option<usize>, Option<usize>) {
    // (sf/occ, wait_hard, inspector_steps, lean_mode_txs, absolute_jump_applied)
    match block {
        19_807_137 => (Some(0.085), Some(7292), Some(1_467_978), Some(0), Some(0)),
        14_683_600 => (Some(0.135), Some(1034), Some(1_270_650), Some(0), Some(0)),
        13_217_637 => (Some(0.101), Some(113), Some(405_348), Some(0), Some(0)),
        14_383_540 => (Some(0.123), Some(800), Some(814_046), Some(0), Some(0)),
        15_199_017 => (Some(0.067), Some(270), Some(506_622), Some(0), Some(0)),
        14_029_313 => (Some(0.045), Some(167), Some(443_585), Some(0), Some(0)),
        19_434_587 => (Some(0.044), Some(2185), Some(1_689_283), Some(0), Some(1)),
        _ => (None, None, None, None, None),
    }
}

fn timing_from_run(
    mode: &str,
    cores: usize,
    n_tx: usize,
    elapsed_ms: f64,
    tps: f64,
    ok: bool,
    error: Option<String>,
    pevm: &Pevm,
    snap: Option<&FineGrainSnapshot>,
) -> ModeTiming {
    let m = pevm.last_specfence_metrics();
    let (total_inc, max_inc, txs_abort, abort_events, mean_cascade) = if let Some(s) = snap {
        let total: usize = s.final_incarnations.iter().map(|i| i + 1).sum();
        let max = s.final_incarnations.iter().copied().max().unwrap_or(0);
        let with_abort = s.final_incarnations.iter().filter(|&&i| i > 0).count();
        let mean_c = if s.abort_events.is_empty() {
            0.0
        } else {
            s.abort_events.iter().map(|e| e.cascade_validations as f64).sum::<f64>()
                / s.abort_events.len() as f64
        };
        (total, max, with_abort, s.abort_events.len(), mean_c)
    } else {
        (0, 0, 0, 0, 0.0)
    };
    let evm_entries = if mode == "sequential" { 0 } else { m.evm_entries };
    ModeTiming {
        mode: mode.to_string(),
        cores,
        elapsed_ms,
        tps,
        ok,
        error,
        occ_aborts: if mode == "sequential" { 0 } else { m.occ_aborts },
        abort_rate: if n_tx == 0 || mode == "sequential" {
            0.0
        } else {
            m.occ_aborts as f64 / n_tx as f64
        },
        cascade_validations_scheduled: if mode == "sequential" {
            0
        } else {
            m.cascade_validations_scheduled
        },
        evm_entries,
        full_restart: if mode == "sequential" { 0 } else { m.full_restart },
        total_incarnations: total_inc,
        max_incarnation: max_inc,
        txs_with_abort: txs_abort,
        abort_events,
        mean_cascade_on_abort: mean_cascade,
        reexec_entry_frac: if evm_entries == 0 {
            0.0
        } else {
            (evm_entries.saturating_sub(n_tx)) as f64 / evm_entries as f64
        },
    }
}

fn run_serial(chain: &PevmEthereum, loaded: &LoadedBlock) -> ModeTiming {
    let n = n_tx(&loaded.block);
    let mut pevm = Pevm::default();
    let t0 = Instant::now();
    let result = pevm.execute(
        chain,
        &loaded.storage,
        &loaded.block,
        NonZeroUsize::new(1).unwrap(),
        true,
    );
    let elapsed = t0.elapsed().as_secs_f64();
    let elapsed_ms = elapsed * 1000.0;
    let tps = if elapsed > 0.0 { n as f64 / elapsed } else { 0.0 };
    match result {
        Ok(_) => timing_from_run("sequential", 1, n, elapsed_ms, tps, true, None, &pevm, None),
        Err(e) => timing_from_run(
            "sequential",
            1,
            n,
            elapsed_ms,
            tps,
            false,
            Some(format!("{e}")),
            &pevm,
            None,
        ),
    }
}

fn run_occ(
    chain: &PevmEthereum,
    loaded: &LoadedBlock,
    cores: usize,
) -> (ModeTiming, Option<FineGrainSnapshot>) {
    let n = n_tx(&loaded.block);
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::Occ);
    pevm.reset_heat();
    pevm.set_finegrain_trace(true);
    let cores_nz = NonZeroUsize::new(cores.max(1)).unwrap();
    // Analysis blocks are >> 4M gas so execute() takes the parallel path.
    let t0 = Instant::now();
    let result = pevm.execute(
        chain,
        &loaded.storage,
        &loaded.block,
        cores_nz,
        false,
    );
    let elapsed = t0.elapsed().as_secs_f64();
    let elapsed_ms = elapsed * 1000.0;
    let tps = if elapsed > 0.0 { n as f64 / elapsed } else { 0.0 };
    let snap = pevm.take_finegrain_snapshot();
    let timing = match &result {
        Ok(_) => timing_from_run("occ", cores, n, elapsed_ms, tps, true, None, &pevm, snap.as_ref()),
        Err(e) => timing_from_run(
            "occ",
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

fn multi_writer_chains(snap: &FineGrainSnapshot, top_k: usize) -> Vec<MultiWriterChain> {
    use std::collections::{HashMap, HashSet};
    let kind_map: HashMap<u64, String> = snap.location_kinds.iter().cloned().collect();
    let mut writers: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut readers: HashMap<u64, HashSet<usize>> = HashMap::new();
    for tx in &snap.txs {
        for &w in &tx.writes {
            if w == snap.beneficiary_hash {
                continue;
            }
            writers.entry(w).or_default().push(tx.tx_idx);
        }
        for &r in &tx.reads {
            if r == snap.beneficiary_hash {
                continue;
            }
            readers.entry(r).or_default().insert(tx.tx_idx);
        }
    }
    let mut chains: Vec<MultiWriterChain> = writers
        .into_iter()
        .filter_map(|(loc, mut ws)| {
            ws.sort_unstable();
            ws.dedup();
            if ws.len() < 2 {
                return None;
            }
            Some(MultiWriterChain {
                location: loc,
                kind: kind_map
                    .get(&loc)
                    .cloned()
                    .unwrap_or_else(|| "unknown".into()),
                n_readers: readers.get(&loc).map(|s| s.len()).unwrap_or(0),
                writers: ws,
            })
        })
        .collect();
    chains.sort_by(|a, b| {
        b.writers
            .len()
            .cmp(&a.writers.len())
            .then(b.n_readers.cmp(&a.n_readers))
    });
    chains.truncate(top_k);
    chains
}

fn summarize_from_snap(
    loaded: &LoadedBlock,
    serial: ModeTiming,
    occ: Vec<ModeTiming>,
    snap: &FineGrainSnapshot,
) -> BlockSummary {
    let dag = analyze_dag(snap, true, true);
    let dag_with_lazy = analyze_dag(snap, true, false);
    let kind_hist = kind_histogram(snap);
    let hot_top20 = hot_locations(snap, 20, true);
    let mean_reads = if snap.n_tx == 0 {
        0.0
    } else {
        snap.txs.iter().map(|t| t.n_reads as f64).sum::<f64>() / snap.n_tx as f64
    };
    let mean_writes = if snap.n_tx == 0 {
        0.0
    } else {
        snap.txs.iter().map(|t| t.n_writes as f64).sum::<f64>() / snap.n_tx as f64
    };
    let (r, wh, insp, lean, jump) = v7_refs(loaded.number);
    BlockSummary {
        block: loaded.number,
        n_tx: n_tx(&loaded.block),
        gas_used: loaded.block.header.gas_used,
        serial,
        occ,
        dag,
        dag_with_lazy,
        kind_hist,
        hot_top20,
        mean_reads,
        mean_writes,
        v7_sf_occ_ratio: r,
        v7_wait_hard: wh,
        v7_inspector_steps: insp,
        v7_lean_mode_txs: lean,
        v7_absolute_jump_applied: jump,
    }
}

fn write_csv(path: &Path, rows: &[BlockSummary]) {
    let mut f = File::create(path).expect("csv");
    writeln!(
        f,
        "block,n_tx,gas_used,serial_ms,serial_tps,occ1_ms,occ1_tps,occ1_aborts,occ1_abort_rate,occ1_evm_entries,occ1_reexec_frac,occ8_ms,occ8_tps,occ8_aborts,occ8_abort_rate,occ8_evm_entries,occ8_reexec_frac,occ8_max_inc,occ8_txs_with_abort,longest_chain,independent_frac,max_wave_width,multi_writer_locs,max_writers,max_conflict_component,lazy_longest_chain,mean_reads,mean_writes,v7_sf_occ,v7_wait_hard,v7_inspector_steps"
    )
    .unwrap();
    for s in rows {
        let o1 = s.occ.iter().find(|o| o.cores == 1);
        let o8 = s.occ.iter().find(|o| o.cores == 8);
        writeln!(
            f,
            "{},{},{},{:.3},{:.1},{},{},{},{:.4},{},{:.4},{},{},{},{:.4},{},{:.4},{},{},{},{:.4},{},{},{},{},{},{:.3},{:.3},{},{},{}",
            s.block,
            s.n_tx,
            s.gas_used,
            s.serial.elapsed_ms,
            s.serial.tps,
            o1.map(|o| format!("{:.3}", o.elapsed_ms)).unwrap_or_default(),
            o1.map(|o| format!("{:.1}", o.tps)).unwrap_or_default(),
            o1.map(|o| o.occ_aborts.to_string()).unwrap_or_default(),
            o1.map(|o| o.abort_rate).unwrap_or(0.0),
            o1.map(|o| o.evm_entries.to_string()).unwrap_or_default(),
            o1.map(|o| o.reexec_entry_frac).unwrap_or(0.0),
            o8.map(|o| format!("{:.3}", o.elapsed_ms)).unwrap_or_default(),
            o8.map(|o| format!("{:.1}", o.tps)).unwrap_or_default(),
            o8.map(|o| o.occ_aborts.to_string()).unwrap_or_default(),
            o8.map(|o| o.abort_rate).unwrap_or(0.0),
            o8.map(|o| o.evm_entries.to_string()).unwrap_or_default(),
            o8.map(|o| o.reexec_entry_frac).unwrap_or(0.0),
            o8.map(|o| o.max_incarnation.to_string()).unwrap_or_default(),
            o8.map(|o| o.txs_with_abort.to_string()).unwrap_or_default(),
            s.dag.longest_chain,
            s.dag.independent_frac,
            s.dag.max_wave_width,
            s.dag.multi_writer_locs,
            s.dag.max_writers_on_loc,
            s.dag.max_conflict_component,
            s.dag_with_lazy.longest_chain,
            s.mean_reads,
            s.mean_writes,
            s.v7_sf_occ_ratio.map(|x| format!("{x:.3}")).unwrap_or_default(),
            s.v7_wait_hard.map(|x| x.to_string()).unwrap_or_default(),
            s.v7_inspector_steps.map(|x| x.to_string()).unwrap_or_default(),
        )
        .unwrap();
    }
}

fn main() {
    let mut out = repo_root().join("lab/results/mainnet-serial-occ-finegrain.json");
    let mut blocks: Vec<u64> = DEFAULT_BLOCKS.to_vec();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => {
                if let Some(p) = args.next() {
                    out = PathBuf::from(p);
                    if !out.is_absolute() {
                        out = repo_root().join(out);
                    }
                }
            }
            "--blocks" => {
                if let Some(raw) = args.next() {
                    blocks = parse_csv_u64(&raw);
                }
            }
            other => eprintln!("unknown arg {other}"),
        }
    }

    let data_dir = repo_root().join("data/ethereum");
    let chain = PevmEthereum::mainnet();
    let (bytecodes, block_hashes) = load_shared(&data_dir);

    let mut summaries = Vec::new();
    let mut deep_dives = Vec::new();

    for &bn in &blocks {
        let Some(loaded) = load_block(&data_dir, bn, bytecodes.clone(), block_hashes.clone()) else {
            continue;
        };
        eprintln!(
            "=== block {bn} n_tx={} gas={} ===",
            n_tx(&loaded.block),
            loaded.block.header.gas_used
        );

        let serial = run_serial(&chain, &loaded);
        eprintln!(
            "  serial: {:.2}ms TPS={:.0} ok={}",
            serial.elapsed_ms, serial.tps, serial.ok
        );

        let mut occ_timings = Vec::new();
        let mut best_snap: Option<FineGrainSnapshot> = None;
        for &cores in OCC_CORES {
            let (timing, snap) = run_occ(&chain, &loaded, cores);
            eprintln!(
                "  occ@{cores}: {:.2}ms TPS={:.0} aborts={} abort_rate={:.3} evm_entries={} reexec_frac={:.3} max_inc={} ok={}",
                timing.elapsed_ms,
                timing.tps,
                timing.occ_aborts,
                timing.abort_rate,
                timing.evm_entries,
                timing.reexec_entry_frac,
                timing.max_incarnation,
                timing.ok
            );
            if snap.is_some() {
                best_snap = snap;
            }
            occ_timings.push(timing);
        }

        let Some(snap) = best_snap else {
            eprintln!("  WARN: no finegrain snapshot (sequential fallback?)");
            continue;
        };

        let summary = summarize_from_snap(&loaded, serial, occ_timings, &snap);
        eprintln!(
            "  DAG(effective): longest={} indep_frac={:.3} max_wave={} multi_w={} max_comp={} | raw_lazy_longest={} kinds={:?}",
            summary.dag.longest_chain,
            summary.dag.independent_frac,
            summary.dag.max_wave_width,
            summary.dag.multi_writer_locs,
            summary.dag.max_conflict_component,
            summary.dag_with_lazy.longest_chain,
            summary.kind_hist
        );

        if DEEP_BLOCKS.contains(&bn) {
            let per_tx: Vec<PerTxRow> = snap
                .txs
                .iter()
                .map(|t| PerTxRow {
                    tx_idx: t.tx_idx,
                    incarnation: t.incarnation,
                    n_reads: t.n_reads,
                    n_writes: t.n_writes,
                })
                .collect();
            let abort_sample: Vec<_> = snap.abort_events.iter().take(200).cloned().collect();
            let chains = multi_writer_chains(&snap, 20);
            let deep = DeepDive {
                block: bn,
                summary: summary.clone(),
                per_tx,
                abort_events_sample: abort_sample,
                multi_writer_chains_top20: chains,
            };
            let deep_path = out.with_file_name(format!("mainnet-serial-occ-finegrain-b{bn}.json"));
            fs::create_dir_all(deep_path.parent().unwrap()).ok();
            let mut f = File::create(&deep_path).expect("deep json");
            serde_json::to_writer_pretty(&mut f, &deep).unwrap();
            eprintln!("  wrote {}", deep_path.display());
            deep_dives.push(bn);
        }

        summaries.push(summary);
    }

    #[derive(Serialize)]
    struct OutFile {
        generated: String,
        blocks: Vec<BlockSummary>,
        deep_dive_blocks: Vec<u64>,
        notes: String,
    }
    let out_obj = OutFile {
        generated: "2026-09-06 Asia/Shanghai".into(),
        blocks: summaries.clone(),
        deep_dive_blocks: deep_dives,
        notes: "Final OCC RW sets ≈ true G* (beneficiary excluded from DAG). v7_* fields cite mainnet-sweep-v7-status.md.".into(),
    };
    fs::create_dir_all(out.parent().unwrap()).ok();
    let mut f = File::create(&out).expect("out json");
    serde_json::to_writer_pretty(&mut f, &out_obj).unwrap();
    let csv = out.with_extension("csv");
    write_csv(&csv, &summaries);
    eprintln!("wrote {} and {}", out.display(), csv.display());
}
