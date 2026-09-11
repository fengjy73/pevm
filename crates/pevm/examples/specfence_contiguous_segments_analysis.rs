//! Contiguous-segment serial + OCC fine-grain analysis (lab opt-in).
//!
//! Produces:
//! - `lab/results/contiguous-segments-finegrain.{json,csv}`
//! - deep dives for cores 14689597, 19606599, 19469097 (+ neighbor notes in JSON)
//!
//! Usage:
//! ```
//! cargo run -p pevm --release --config 'profile.release.lto=false' \
//!   --example specfence_contiguous_segments_analysis
//! ```
//!
//! Does **not** enable production inspect tax; finegrain_trace is opt-in on OCC runs only.

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

use hashbrown::HashMap;
use alloy_rpc_types_eth::Block;
use flate2::bufread::GzDecoder;
use pevm::{
    BlockHashes, BuildSuffixHasher, Bytecodes, ConcurrencyMode, EvmAccount, FineGrainSnapshot,
    InMemoryStorage, Pevm, analyze_dag, classify_raw_edges, hot_locations, kind_histogram,
    program_raw_longest_chain,
    chain::{PevmChain, PevmEthereum},
    RawEdge,
};
use serde::Serialize;

const SEGMENT_A: &[u64] = &[14_689_595, 14_689_596, 14_689_597, 14_689_598, 14_689_599];
const SEGMENT_B: &[u64] = &[19_606_597, 19_606_598, 19_606_599, 19_606_600];
const SEGMENT_C: &[u64] = &[19_469_096, 19_469_097, 19_469_098, 19_469_099];
const CORE_BLOCKS: &[u64] = &[14_689_597, 19_606_599, 19_469_097];
const OCC_CORES: &[usize] = &[1, 8];
const RAW_SAMPLE_CAP: usize = 5000;

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
    total_incarnations: usize,
    max_incarnation: usize,
    txs_with_abort: usize,
    abort_events: usize,
    mean_cascade_on_abort: f64,
    reexec_entry_frac: f64,
}

#[derive(Clone, Serialize)]
struct RawClassStats {
    n_raw_total: usize,
    n_raw_program: usize,
    n_raw_handler: usize,
    program_frac: f64,
    handler_frac: f64,
    mean_producer_lag: f64,
    median_producer_lag: f64,
    p90_producer_lag: f64,
    mean_program_lag: f64,
    mean_handler_lag: f64,
    longest_program_raw_chain: usize,
    /// Proxy: among txs with ≥1 RAW inbound, mean (n_cross_tx_reads / n_reads).
    mean_cross_tx_read_frac: f64,
    /// Proxy: fraction of txs that have ≥1 program RAW inbound.
    frac_txs_with_program_raw: f64,
    kind_raw_hist: StdHashMap<String, usize>,
}

#[derive(Clone, Serialize)]
struct BlockSummary {
    segment: String,
    block: u64,
    n_tx: usize,
    gas_used: u64,
    serial: ModeTiming,
    occ: Vec<ModeTiming>,
    dag: pevm::DagStats,
    dag_with_lazy: pevm::DagStats,
    kind_hist: StdHashMap<String, usize>,
    hot_top20: Vec<pevm::HotLocation>,
    mean_reads: f64,
    mean_writes: f64,
    raw: RawClassStats,
}

#[derive(Serialize)]
struct DeepDive {
    block: u64,
    neighbors: Vec<u64>,
    summary: BlockSummary,
    per_tx: Vec<PerTxRow>,
    abort_events_sample: Vec<pevm::AbortEvent>,
    raw_edges_sample: Vec<RawEdge>,
    multi_writer_chains_top20: Vec<MultiWriterChain>,
}

#[derive(Clone, Serialize)]
struct PerTxRow {
    tx_idx: usize,
    incarnation: usize,
    n_reads: usize,
    n_writes: usize,
    n_raw_inbound: usize,
    n_program_raw_inbound: usize,
    min_producer_lag: Option<usize>,
    cross_tx_read_frac: f64,
}

#[derive(Clone, Serialize)]
struct MultiWriterChain {
    location: u64,
    kind: String,
    writers: Vec<usize>,
    n_readers: usize,
}

#[derive(Clone, Serialize)]
struct SegmentSummary {
    name: String,
    blocks: Vec<u64>,
    core: u64,
    n_blocks: usize,
    mean_n_tx: f64,
    mean_occ8_tps: f64,
    mean_occ8_abort_rate: f64,
    mean_raw: f64,
    mean_program_raw: f64,
    mean_handler_raw: f64,
    mean_longest_chain: f64,
    mean_program_raw_chain: f64,
    max_program_raw_chain: usize,
    transitions: Vec<TransitionNote>,
}

#[derive(Clone, Serialize)]
struct TransitionNote {
    from: u64,
    to: u64,
    delta_n_tx: i64,
    delta_occ8_abort_rate: f64,
    delta_raw: i64,
    delta_program_raw: i64,
    delta_handler_raw: i64,
    delta_longest_chain: i64,
    delta_program_chain: i64,
    note: String,
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

fn neighbors_of(bn: u64) -> Vec<u64> {
    let seg: &[u64] = if SEGMENT_A.contains(&bn) {
        SEGMENT_A
    } else if SEGMENT_B.contains(&bn) {
        SEGMENT_B
    } else if SEGMENT_C.contains(&bn) {
        SEGMENT_C
    } else {
        return vec![];
    };
    seg.iter().copied().filter(|&x| x != bn).collect()
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
    if !dir.join("block.json").exists() || !dir.join("pre_state.json").exists() {
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

fn percentile_sorted(sorted: &[usize], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)] as f64
}

fn raw_stats(snap: &FineGrainSnapshot, raw: &[RawEdge]) -> RawClassStats {
    let n = raw.len();
    let n_prog = raw.iter().filter(|e| e.class == "program").count();
    let n_hand = n.saturating_sub(n_prog);
    let mut lags: Vec<usize> = raw.iter().map(|e| e.producer_lag).collect();
    lags.sort_unstable();
    let mean_lag = if n == 0 {
        0.0
    } else {
        lags.iter().sum::<usize>() as f64 / n as f64
    };
    let prog_lags: Vec<usize> = raw
        .iter()
        .filter(|e| e.class == "program")
        .map(|e| e.producer_lag)
        .collect();
    let hand_lags: Vec<usize> = raw
        .iter()
        .filter(|e| e.class == "handler")
        .map(|e| e.producer_lag)
        .collect();
    let mean_program_lag = if prog_lags.is_empty() {
        0.0
    } else {
        prog_lags.iter().sum::<usize>() as f64 / prog_lags.len() as f64
    };
    let mean_handler_lag = if hand_lags.is_empty() {
        0.0
    } else {
        hand_lags.iter().sum::<usize>() as f64 / hand_lags.len() as f64
    };

    // Per-tx proxies from final RW sets (no gas-depth without inspect).
    let mut inbound: StdHashMap<usize, Vec<&RawEdge>> = StdHashMap::new();
    for e in raw {
        inbound.entry(e.consumer_tx).or_default().push(e);
    }
    let mut cross_fracs = Vec::new();
    let mut txs_with_prog = 0usize;
    for tx in &snap.txs {
        if let Some(edges) = inbound.get(&tx.tx_idx) {
            let cross = edges.len(); // each RAW inbound is a cross-tx read on distinct (prod,loc) — approx
            // Better: unique locations among inbound / n_reads
            let mut locs = edges.iter().map(|e| e.location).collect::<Vec<_>>();
            locs.sort_unstable();
            locs.dedup();
            let frac = if tx.n_reads == 0 {
                0.0
            } else {
                locs.len() as f64 / tx.n_reads as f64
            };
            cross_fracs.push(frac);
            if edges.iter().any(|e| e.class == "program") {
                txs_with_prog += 1;
            }
            let _ = cross;
        }
    }
    let mean_cross = if cross_fracs.is_empty() {
        0.0
    } else {
        cross_fracs.iter().sum::<f64>() / cross_fracs.len() as f64
    };
    let mut kind_hist = StdHashMap::new();
    for e in raw {
        *kind_hist.entry(e.kind.clone()).or_default() += 1;
    }

    RawClassStats {
        n_raw_total: n,
        n_raw_program: n_prog,
        n_raw_handler: n_hand,
        program_frac: if n == 0 { 0.0 } else { n_prog as f64 / n as f64 },
        handler_frac: if n == 0 { 0.0 } else { n_hand as f64 / n as f64 },
        mean_producer_lag: mean_lag,
        median_producer_lag: percentile_sorted(&lags, 0.5),
        p90_producer_lag: percentile_sorted(&lags, 0.9),
        mean_program_lag,
        mean_handler_lag,
        longest_program_raw_chain: program_raw_longest_chain(raw),
        mean_cross_tx_read_frac: mean_cross,
        frac_txs_with_program_raw: if snap.n_tx == 0 {
            0.0
        } else {
            txs_with_prog as f64 / snap.n_tx as f64
        },
        kind_raw_hist: kind_hist,
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
            s.abort_events
                .iter()
                .map(|e| e.cascade_validations as f64)
                .sum::<f64>()
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
    // Lab-only: pevm::execute falls back to sequential when gas_used < 4M or
    // n_tx < concurrency, which skips FineGrainCollector. Bump gas_used on a
    // header clone so low-gas neighbors still take the parallel OCC path.
    let mut block = loaded.block.clone();
    if block.header.gas_used < 4_000_000 {
        block.header.gas_used = 4_000_000;
    }
    let t0 = Instant::now();
    let result = pevm.execute(
        chain,
        &loaded.storage,
        &block,
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

fn summarize(
    loaded: &LoadedBlock,
    serial: ModeTiming,
    occ: Vec<ModeTiming>,
    snap: &FineGrainSnapshot,
) -> (BlockSummary, Vec<RawEdge>) {
    let dag = analyze_dag(snap, true, true);
    let dag_with_lazy = analyze_dag(snap, true, false);
    let kind_hist = kind_histogram(snap);
    let hot_top20 = hot_locations(snap, 20, true);
    let raw = classify_raw_edges(snap, true, true);
    let raw_stats = raw_stats(snap, &raw);
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
    (
        BlockSummary {
            segment: segment_of(loaded.number).into(),
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
            raw: raw_stats,
        },
        raw,
    )
}

fn per_tx_rows(snap: &FineGrainSnapshot, raw: &[RawEdge]) -> Vec<PerTxRow> {
    let mut inbound: StdHashMap<usize, Vec<&RawEdge>> = StdHashMap::new();
    for e in raw {
        inbound.entry(e.consumer_tx).or_default().push(e);
    }
    snap.txs
        .iter()
        .map(|t| {
            let edges = inbound.get(&t.tx_idx);
            let n_raw = edges.map(|e| e.len()).unwrap_or(0);
            let n_prog = edges
                .map(|e| e.iter().filter(|x| x.class == "program").count())
                .unwrap_or(0);
            let min_lag = edges.and_then(|e| e.iter().map(|x| x.producer_lag).min());
            let mut locs = edges
                .map(|e| e.iter().map(|x| x.location).collect::<Vec<_>>())
                .unwrap_or_default();
            locs.sort_unstable();
            locs.dedup();
            let cross_frac = if t.n_reads == 0 {
                0.0
            } else {
                locs.len() as f64 / t.n_reads as f64
            };
            PerTxRow {
                tx_idx: t.tx_idx,
                incarnation: t.incarnation,
                n_reads: t.n_reads,
                n_writes: t.n_writes,
                n_raw_inbound: n_raw,
                n_program_raw_inbound: n_prog,
                min_producer_lag: min_lag,
                cross_tx_read_frac: cross_frac,
            }
        })
        .collect()
}

fn write_csv(path: &Path, rows: &[BlockSummary]) {
    let mut f = File::create(path).expect("csv");
    writeln!(
        f,
        "segment,block,n_tx,gas_used,serial_ms,serial_tps,occ1_ms,occ1_tps,occ1_aborts,occ1_abort_rate,occ1_reexec_frac,occ8_ms,occ8_tps,occ8_aborts,occ8_abort_rate,occ8_evm_entries,occ8_reexec_frac,occ8_max_inc,occ8_txs_with_abort,dag_n_raw,dag_n_waw,longest_chain,independent_frac,max_wave_width,multi_writer_locs,max_conflict_component,raw_total,raw_program,raw_handler,program_frac,mean_producer_lag,median_producer_lag,p90_producer_lag,longest_program_raw_chain,mean_cross_tx_read_frac,frac_txs_with_program_raw,mean_reads,mean_writes"
    )
    .unwrap();
    for s in rows {
        let o1 = s.occ.iter().find(|o| o.cores == 1);
        let o8 = s.occ.iter().find(|o| o.cores == 8);
        writeln!(
            f,
            "{},{},{},{},{:.3},{:.1},{},{},{},{:.4},{:.4},{},{},{},{:.4},{},{:.4},{},{},{},{},{},{:.4},{},{},{},{},{},{},{:.4},{:.2},{:.2},{:.2},{},{:.4},{:.4},{:.3},{:.3}",
            s.segment,
            s.block,
            s.n_tx,
            s.gas_used,
            s.serial.elapsed_ms,
            s.serial.tps,
            o1.map(|o| format!("{:.3}", o.elapsed_ms)).unwrap_or_default(),
            o1.map(|o| format!("{:.1}", o.tps)).unwrap_or_default(),
            o1.map(|o| o.occ_aborts.to_string()).unwrap_or_default(),
            o1.map(|o| o.abort_rate).unwrap_or(0.0),
            o1.map(|o| o.reexec_entry_frac).unwrap_or(0.0),
            o8.map(|o| format!("{:.3}", o.elapsed_ms)).unwrap_or_default(),
            o8.map(|o| format!("{:.1}", o.tps)).unwrap_or_default(),
            o8.map(|o| o.occ_aborts.to_string()).unwrap_or_default(),
            o8.map(|o| o.abort_rate).unwrap_or(0.0),
            o8.map(|o| o.evm_entries.to_string()).unwrap_or_default(),
            o8.map(|o| o.reexec_entry_frac).unwrap_or(0.0),
            o8.map(|o| o.max_incarnation.to_string()).unwrap_or_default(),
            o8.map(|o| o.txs_with_abort.to_string()).unwrap_or_default(),
            s.dag.n_raw,
            s.dag.n_waw,
            s.dag.longest_chain,
            s.dag.independent_frac,
            s.dag.max_wave_width,
            s.dag.multi_writer_locs,
            s.dag.max_conflict_component,
            s.raw.n_raw_total,
            s.raw.n_raw_program,
            s.raw.n_raw_handler,
            s.raw.program_frac,
            s.raw.mean_producer_lag,
            s.raw.median_producer_lag,
            s.raw.p90_producer_lag,
            s.raw.longest_program_raw_chain,
            s.raw.mean_cross_tx_read_frac,
            s.raw.frac_txs_with_program_raw,
            s.mean_reads,
            s.mean_writes,
        )
        .unwrap();
    }
}

fn segment_summary(name: &str, core: u64, blocks: &[u64], rows: &[BlockSummary]) -> SegmentSummary {
    let seg_rows: Vec<&BlockSummary> = blocks
        .iter()
        .filter_map(|b| rows.iter().find(|r| r.block == *b))
        .collect();
    let n = seg_rows.len();
    let mean = |f: fn(&BlockSummary) -> f64| {
        if n == 0 {
            0.0
        } else {
            seg_rows.iter().map(|r| f(r)).sum::<f64>() / n as f64
        }
    };
    let occ8_abort = |r: &BlockSummary| {
        r.occ
            .iter()
            .find(|o| o.cores == 8)
            .map(|o| o.abort_rate)
            .unwrap_or(0.0)
    };
    let occ8_tps = |r: &BlockSummary| {
        r.occ
            .iter()
            .find(|o| o.cores == 8)
            .map(|o| o.tps)
            .unwrap_or(0.0)
    };
    let mut transitions = Vec::new();
    for w in seg_rows.windows(2) {
        let (a, b) = (w[0], w[1]);
        let da = occ8_abort(a);
        let db = occ8_abort(b);
        let note = if db + 0.05 < da && b.raw.n_raw_total as i64 <= a.raw.n_raw_total as i64 + 20 {
            "abort_down_or_stable_raw".into()
        } else if db > da + 0.1 {
            "abort_up".into()
        } else if b.raw.longest_program_raw_chain > a.raw.longest_program_raw_chain + 5 {
            "program_chain_grows".into()
        } else if (b.n_tx as i64 - a.n_tx as i64).abs() > 50 {
            "n_tx_shift".into()
        } else {
            "contiguous_similar".into()
        };
        transitions.push(TransitionNote {
            from: a.block,
            to: b.block,
            delta_n_tx: b.n_tx as i64 - a.n_tx as i64,
            delta_occ8_abort_rate: db - da,
            delta_raw: b.raw.n_raw_total as i64 - a.raw.n_raw_total as i64,
            delta_program_raw: b.raw.n_raw_program as i64 - a.raw.n_raw_program as i64,
            delta_handler_raw: b.raw.n_raw_handler as i64 - a.raw.n_raw_handler as i64,
            delta_longest_chain: b.dag.longest_chain as i64 - a.dag.longest_chain as i64,
            delta_program_chain: b.raw.longest_program_raw_chain as i64
                - a.raw.longest_program_raw_chain as i64,
            note,
        });
    }
    let max_prog = seg_rows
        .iter()
        .map(|r| r.raw.longest_program_raw_chain)
        .max()
        .unwrap_or(0);
    SegmentSummary {
        name: name.into(),
        blocks: blocks.to_vec(),
        core,
        n_blocks: n,
        mean_n_tx: mean(|r| r.n_tx as f64),
        mean_occ8_tps: mean(occ8_tps),
        mean_occ8_abort_rate: mean(occ8_abort),
        mean_raw: mean(|r| r.raw.n_raw_total as f64),
        mean_program_raw: mean(|r| r.raw.n_raw_program as f64),
        mean_handler_raw: mean(|r| r.raw.n_raw_handler as f64),
        mean_longest_chain: mean(|r| r.dag.longest_chain as f64),
        mean_program_raw_chain: mean(|r| r.raw.longest_program_raw_chain as f64),
        max_program_raw_chain: max_prog,
        transitions,
    }
}

fn main() {
    let out = repo_root().join("lab/results/contiguous-segments-finegrain.json");
    let mut all_blocks: Vec<u64> = Vec::new();
    all_blocks.extend_from_slice(SEGMENT_A);
    all_blocks.extend_from_slice(SEGMENT_B);
    all_blocks.extend_from_slice(SEGMENT_C);

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
            eprintln!("  WARN: no finegrain snapshot");
            continue;
        };

        let (summary, raw) = summarize(&loaded, serial, occ_timings, &snap);
        eprintln!(
            "  DAG: longest={} raw={} waw={} | RAW prog={} hand={} prog_chain={} mean_lag={:.1} cross_read_frac={:.3}",
            summary.dag.longest_chain,
            summary.dag.n_raw,
            summary.dag.n_waw,
            summary.raw.n_raw_program,
            summary.raw.n_raw_handler,
            summary.raw.longest_program_raw_chain,
            summary.raw.mean_producer_lag,
            summary.raw.mean_cross_tx_read_frac
        );

        if CORE_BLOCKS.contains(&bn) {
            let mut raw_sample = raw.clone();
            raw_sample.truncate(RAW_SAMPLE_CAP);
            let deep = DeepDive {
                block: bn,
                neighbors: neighbors_of(bn),
                summary: summary.clone(),
                per_tx: per_tx_rows(&snap, &raw),
                abort_events_sample: snap.abort_events.iter().take(200).cloned().collect(),
                raw_edges_sample: raw_sample,
                multi_writer_chains_top20: multi_writer_chains(&snap, 20),
            };
            let deep_path = out.with_file_name(format!("contiguous-segments-finegrain-b{bn}.json"));
            fs::create_dir_all(deep_path.parent().unwrap()).ok();
            let mut f = File::create(&deep_path).expect("deep json");
            serde_json::to_writer_pretty(&mut f, &deep).unwrap();
            eprintln!("  wrote {}", deep_path.display());
            deep_dives.push(bn);
        }

        summaries.push(summary);
    }

    let segments = vec![
        segment_summary("A", 14_689_597, SEGMENT_A, &summaries),
        segment_summary("B", 19_606_599, SEGMENT_B, &summaries),
        segment_summary("C", 19_469_097, SEGMENT_C, &summaries),
    ];

    #[derive(Serialize)]
    struct OutFile {
        generated: String,
        rpc_note: String,
        method_notes: Vec<String>,
        blocks: Vec<BlockSummary>,
        segments: Vec<SegmentSummary>,
        deep_dive_blocks: Vec<u64>,
    }
    let out_obj = OutFile {
        generated: "2026-09-06 Asia/Shanghai".into(),
        rpc_note: "Snapshots fetched via Alchemy ETH RPC from ~/.config/ethereum".into(),
        method_notes: vec![
            "Effective DAG excludes beneficiary + basic_lazy".into(),
            "RAW edges: closest prior writer→reader; class program=storage|code_hash|selfdestruct, handler=basic|unknown".into(),
            "Early-read gas depth NOT available without inspect; proxies = producer_lag + cross_tx_read_frac on final RW".into(),
            "finegrain_trace opt-in on OCC only; production seq≡par unchanged".into(),
        ],
        blocks: summaries.clone(),
        segments: segments.clone(),
        deep_dive_blocks: deep_dives,
    };
    fs::create_dir_all(out.parent().unwrap()).ok();
    let mut f = File::create(&out).expect("out json");
    serde_json::to_writer_pretty(&mut f, &out_obj).unwrap();
    let csv = out.with_extension("csv");
    write_csv(&csv, &summaries);
    eprintln!("wrote {} and {}", out.display(), csv.display());
    for s in &segments {
        eprintln!(
            "segment {}: n_blocks={} mean_raw={:.0} prog={:.0} hand={:.0} mean_prog_chain={:.1} max_prog_chain={}",
            s.name,
            s.n_blocks,
            s.mean_raw,
            s.mean_program_raw,
            s.mean_handler_raw,
            s.mean_program_raw_chain,
            s.max_program_raw_chain
        );
    }
}
