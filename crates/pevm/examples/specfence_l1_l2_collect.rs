//! Unified L1 sequential + L2 OCC@1/@8 collection under frozen MeasurementMethod.
//!
//! Writes:
//! - `lab/results/l1l2-b{N}.json` per block (method + L1 dag + effect_log sample + L2 edges)
//! - `lab/results/l1l2-summary.json`
//!
//! ```
//! cargo run -p pevm --release --config 'profile.release.lto=false' \
//!   --example specfence_l1_l2_collect
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
    InMemoryStorage, L1DagSummary, MeasurementMethod, Pevm, analyze_dag, filter_effect_edges,
    l1_dag_summary,
    chain::{PevmChain, PevmEthereum},
};
use serde::Serialize;

const PRIORITY: &[u64] = &[
    14_689_597, 19_606_599, 19_469_097, 19_606_598, 19_469_096,
];

#[derive(Serialize)]
struct ModeRun {
    mode: String,
    cores: usize,
    elapsed_ms: f64,
    ok: bool,
    error: Option<String>,
    occ_aborts: usize,
    n_effect_edges: usize,
    n_effect_log: usize,
}

#[derive(Serialize)]
struct L2EdgeExport {
    producer_tx: usize,
    consumer_tx: usize,
    consumer_incarnation: usize,
    location: u64,
    kind: String,
    class: String,
    gas_used_so_far: Option<u64>,
    opcode_steps: Option<usize>,
    warm: bool,
    call_depth: Option<u16>,
    producer_status: Option<String>,
    ready_for_bind: Option<bool>,
    producer_ready: Option<String>,
    producer_mv: Option<String>,
}

#[derive(Serialize)]
struct BlockExport {
    block: u64,
    method: MeasurementMethod,
    n_tx: usize,
    gas_used: u64,
    l1: Option<L1Export>,
    l2_occ1: Option<L2Export>,
    l2_occ8: Option<L2Export>,
}

#[derive(Serialize)]
struct L1Export {
    timing: ModeRun,
    dag: L1DagSummary,
    effect_log_len: usize,
    effect_log_sample: Vec<pevm::EffectLogEntry>,
    reads_from_sample: Vec<ReadsFrom>,
}

#[derive(Serialize)]
struct ReadsFrom {
    consumer_tx: usize,
    location: u64,
    producer_tx: usize,
    producer_effect_k: usize,
    warm: bool,
}

#[derive(Serialize)]
struct L2Export {
    timing: ModeRun,
    n_raw: usize,
    n_program: usize,
    n_handler: usize,
    producer_status_hist: StdHashMap<String, usize>,
    ready_for_bind_frac: f64,
    warm_frac: f64,
    edges_sample: Vec<L2EdgeExport>,
    abort_events_sample: Vec<pevm::AbortEvent>,
    consumer_first_cross_sample: Vec<pevm::ConsumerFirstCross>,
}

struct LoadedBlock {
    number: u64,
    block: Block<<PevmEthereum as PevmChain>::Transaction>,
    storage: InMemoryStorage,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_shared(data_dir: &Path) -> (Arc<Bytecodes>, Arc<BlockHashes>) {
    let bytecodes = bincode::serde::decode_from_std_read(
        &mut GzDecoder::new(BufReader::new(
            File::open(data_dir.join("bytecodes.bincode.gz")).expect("bytecodes"),
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
            File::open(dir.join("pre_state.json")).expect("pre_state"),
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

fn run_journal(
    chain: &PevmEthereum,
    loaded: &LoadedBlock,
    cores: usize,
) -> (ModeRun, Option<FineGrainSnapshot>) {
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
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let snap = pevm.take_finegrain_snapshot();
    let m = pevm.last_specfence_metrics();
    let timing = ModeRun {
        mode: format!("occ_journal@{cores}"),
        cores,
        elapsed_ms,
        ok: result.is_ok(),
        error: result.err().map(|e| format!("{e}")),
        occ_aborts: m.occ_aborts,
        n_effect_edges: snap.as_ref().map(|s| s.effect_edges.len()).unwrap_or(0),
        n_effect_log: snap.as_ref().map(|s| s.effect_log.len()).unwrap_or(0),
    };
    let _ = n;
    (timing, snap)
}

fn export_l2(timing: ModeRun, snap: &FineGrainSnapshot) -> L2Export {
    let edges = filter_effect_edges(snap, true, true);
    let n_raw = edges.len();
    let n_program = edges.iter().filter(|e| e.class == "program").count();
    let n_handler = n_raw.saturating_sub(n_program);
    let mut hist: StdHashMap<String, usize> = StdHashMap::new();
    let mut n_bind = 0usize;
    let mut n_warm = 0usize;
    for e in &edges {
        if let Some(s) = &e.producer_status {
            *hist.entry(s.clone()).or_default() += 1;
        } else if let (Some(r), Some(m)) = (&e.producer_ready, &e.producer_mv) {
            let s = pevm::producer_status_canonical(r, m).to_string();
            *hist.entry(s).or_default() += 1;
        }
        if e.ready_for_bind.unwrap_or(false) {
            n_bind += 1;
        }
        if e.warm {
            n_warm += 1;
        }
    }
    let edges_sample: Vec<L2EdgeExport> = edges
        .iter()
        .take(200)
        .map(|e| L2EdgeExport {
            producer_tx: e.producer_tx,
            consumer_tx: e.consumer_tx,
            consumer_incarnation: e.consumer_incarnation,
            location: e.location,
            kind: e.kind.clone(),
            class: e.class.clone(),
            gas_used_so_far: e.gas_used_so_far,
            opcode_steps: e.opcode_steps,
            warm: e.warm,
            call_depth: e.call_depth,
            producer_status: e.producer_status.clone(),
            ready_for_bind: e.ready_for_bind,
            producer_ready: e.producer_ready.clone(),
            producer_mv: e.producer_mv.clone(),
        })
        .collect();
    L2Export {
        timing,
        n_raw,
        n_program,
        n_handler,
        producer_status_hist: hist,
        ready_for_bind_frac: if n_raw == 0 {
            0.0
        } else {
            n_bind as f64 / n_raw as f64
        },
        warm_frac: if n_raw == 0 {
            0.0
        } else {
            n_warm as f64 / n_raw as f64
        },
        edges_sample,
        abort_events_sample: snap.abort_events.iter().take(100).cloned().collect(),
        consumer_first_cross_sample: snap.consumer_first_cross.iter().take(100).cloned().collect(),
    }
}

fn export_l1(timing: ModeRun, snap: &FineGrainSnapshot) -> L1Export {
    let dag = l1_dag_summary(snap);
    let _ = analyze_dag(snap, true, true);
    let reads_from_sample: Vec<ReadsFrom> = filter_effect_edges(snap, true, true)
        .into_iter()
        .take(200)
        .map(|e| ReadsFrom {
            consumer_tx: e.consumer_tx,
            location: e.location,
            producer_tx: e.producer_tx,
            producer_effect_k: e.producer_effect_k,
            warm: e.warm,
        })
        .collect();
    L1Export {
        timing,
        dag,
        effect_log_len: snap.effect_log.len(),
        effect_log_sample: snap.effect_log.iter().take(300).cloned().collect(),
        reads_from_sample,
    }
}

fn main() {
    let root = repo_root();
    let out_dir = root.join("lab/results");
    fs::create_dir_all(&out_dir).ok();
    let data_dir = root.join("data/ethereum");
    let chain = PevmEthereum::mainnet();
    let (bytecodes, block_hashes) = load_shared(&data_dir);

    let mut summary = Vec::new();
    for &bn in PRIORITY {
        let Some(loaded) = load_block(&data_dir, bn, bytecodes.clone(), block_hashes.clone()) else {
            continue;
        };
        eprintln!(
            "=== L1/L2 block {bn} n_tx={} ===",
            n_tx(&loaded.block)
        );

        // L1 oracle: OCC@1 journal (ordered serial discovery ≈ sequential G*)
        let (t1, s1) = run_journal(&chain, &loaded, 1);
        eprintln!(
            "  L1/OCC@1: {:.0}ms ok={} edges={} log={} morph={:?}",
            t1.elapsed_ms,
            t1.ok,
            t1.n_effect_edges,
            t1.n_effect_log,
            s1.as_ref().map(|s| l1_dag_summary(s).morphology)
        );

        let l1 = s1.as_ref().map(|s| export_l1(
            ModeRun {
                mode: t1.mode.clone(),
                cores: t1.cores,
                elapsed_ms: t1.elapsed_ms,
                ok: t1.ok,
                error: t1.error.clone(),
                occ_aborts: t1.occ_aborts,
                n_effect_edges: t1.n_effect_edges,
                n_effect_log: t1.n_effect_log,
            },
            s,
        ));
        let l2_1 = s1.as_ref().map(|s| export_l2(t1, s));

        let (t8, s8) = run_journal(&chain, &loaded, 8);
        eprintln!(
            "  L2/OCC@8: {:.0}ms ok={} edges={} aborts={}",
            t8.elapsed_ms, t8.ok, t8.n_effect_edges, t8.occ_aborts
        );
        let l2_8 = s8.as_ref().map(|s| export_l2(t8, s));

        let export = BlockExport {
            block: bn,
            method: MeasurementMethod::frozen(),
            n_tx: n_tx(&loaded.block),
            gas_used: loaded.block.header.gas_used,
            l1,
            l2_occ1: l2_1,
            l2_occ8: l2_8,
        };
        let path = out_dir.join(format!("l1l2-b{bn}.json"));
        let mut f = File::create(&path).expect("create");
        serde_json::to_writer_pretty(&mut f, &export).expect("write");
        eprintln!("  wrote {}", path.display());
        summary.push(serde_json::json!({
            "block": bn,
            "n_tx": export.n_tx,
            "morphology": export.l1.as_ref().map(|l| l.dag.morphology.clone()),
            "l1_raw": export.l1.as_ref().map(|l| l.dag.n_raw_instances),
            "l1_log": export.l1.as_ref().map(|l| l.effect_log_len),
            "l2_occ1_raw": export.l2_occ1.as_ref().map(|l| l.n_raw),
            "l2_occ8_raw": export.l2_occ8.as_ref().map(|l| l.n_raw),
            "l2_occ8_bind_frac": export.l2_occ8.as_ref().map(|l| l.ready_for_bind_frac),
            "method": MeasurementMethod::frozen(),
        }));
    }

    let summary_path = out_dir.join("l1l2-summary.json");
    let mut f = File::create(&summary_path).unwrap();
    serde_json::to_writer_pretty(
        &mut f,
        &serde_json::json!({
            "method": MeasurementMethod::frozen(),
            "blocks": summary,
        }),
    )
    .unwrap();
    writeln!(f).ok();
    eprintln!("wrote {}", summary_path.display());
}
