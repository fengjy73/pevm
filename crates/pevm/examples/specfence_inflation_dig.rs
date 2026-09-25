#![recursion_limit = "256"]
//! Whole-block timing for sequential execution, upstream OCC, and `SpecFence`.
//!
//! The clock starts after the transaction list and block env exist, and stops
//! when the engine returns. Every round builds a fresh engine. There is no
//! untimed warm-up and no carried learned state.
//!
//! ```text
//! SPECFENCE_INFLATION_WHICH=scan SPECFENCE_COMPARE_CORES=4 \
//! SPECFENCE_INFLATION_K=7 SPECFENCE_PIN_CPUS=0,1,2,3 \
//! SPECFENCE_INFLATION_BLOCKS=15274915,3356896 \
//! SPECFENCE_CLASS_KEY=to
//! ```

use std::{
    fs::File,
    io::{BufReader, Write},
    num::NonZeroUsize,
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use alloy_primitives::Address;
use alloy_rpc_types_eth::Block;
use flate2::bufread::GzDecoder;
use hashbrown::HashMap;
use pevm::{
    BlockHashes, BuildSuffixHasher, EvmAccount, InMemoryStorage, Pevm,
    chain::{PevmChain, PevmEthereum},
    specfence::{SfClassKey, SfOptions, run_sf_block},
};
use revm::primitives::hardfork::SpecId;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn sched_getaffinity(pid: i32, cpusetsize: usize, mask: *mut u8) -> i32;
    fn sched_setaffinity(pid: i32, cpusetsize: usize, mask: *const u8) -> i32;
}

const CPU_SET_BYTES: usize = 128;

struct AffinityGuard {
    prev: Option<[u8; CPU_SET_BYTES]>,
}

impl AffinityGuard {
    fn pin(cpus: &[usize]) -> Self {
        #[cfg(target_os = "linux")]
        {
            let mut prev = [0u8; CPU_SET_BYTES];
            let mut next = [0u8; CPU_SET_BYTES];
            for &cpu in cpus {
                if cpu < CPU_SET_BYTES * 8 {
                    next[cpu / 8] |= 1 << (cpu % 8);
                }
            }
            unsafe {
                if sched_getaffinity(0, CPU_SET_BYTES, prev.as_mut_ptr()) != 0 {
                    return Self { prev: None };
                }
                if sched_setaffinity(0, CPU_SET_BYTES, next.as_ptr()) != 0 {
                    return Self { prev: None };
                }
            }
            Self { prev: Some(prev) }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = cpus;
            Self { prev: None }
        }
    }
}

impl Drop for AffinityGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(prev) = &self.prev {
            unsafe {
                sched_setaffinity(0, CPU_SET_BYTES, prev.as_ptr());
            }
        }
    }
}

struct Loaded {
    block_no: u64,
    gas_used: u64,
    n: usize,
    spec_id: SpecId,
    block_env: revm::context::BlockEnv,
    txs: Vec<revm::context::TxEnv>,
    storage: InMemoryStorage,
}

fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SPECFENCE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    for candidate in ["data/ethereum", "../../data/ethereum"] {
        let path = PathBuf::from(candidate);
        if path.join("bytecodes.bincode.gz").is_file() {
            return path;
        }
    }
    PathBuf::from("data/ethereum")
}

fn load_block(block_no: u64) -> Loaded {
    let chain = PevmEthereum::mainnet();
    let data_dir = data_dir();
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
    let dir = data_dir.join("blocks").join(block_no.to_string());
    let block: Block<<PevmEthereum as PevmChain>::Transaction> =
        serde_json::from_reader(BufReader::new(File::open(dir.join("block.json")).unwrap()))
            .unwrap();
    let accounts: HashMap<Address, EvmAccount, BuildSuffixHasher> = serde_json::from_reader(
        BufReader::new(File::open(dir.join("pre_state.json")).unwrap()),
    )
    .unwrap();
    let storage = InMemoryStorage::new(accounts, bytecodes, block_hashes);
    let spec_id = chain.get_block_spec(&block.header).unwrap();
    let block_env = pevm::specfence::block_env(&block.header, spec_id);
    let txs = match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs
            .iter()
            .map(|tx| chain.get_tx_env(tx).unwrap())
            .collect::<Vec<_>>(),
        _ => panic!("full transactions required"),
    };
    let n = txs.len();
    Loaded {
        block_no,
        gas_used: block.header.gas_used,
        n,
        spec_id,
        block_env,
        txs,
        storage,
    }
}

fn class_key() -> SfClassKey {
    match std::env::var("SPECFENCE_CLASS_KEY").ok().as_deref() {
        Some("code" | "code_hash" | "code_hash+selector") => SfClassKey::CodeHashSelector,
        _ => SfClassKey::ToSelector,
    }
}

fn engine_selected(name: &str) -> bool {
    match std::env::var("SPECFENCE_INFLATION_ENGINES") {
        Ok(list) => list.split(',').any(|s| s.trim() == name),
        Err(_) => true,
    }
}

struct RunOut {
    wall_ms: f64,
    ok: bool,
    err: String,
    reexec: usize,
    full_replay: usize,
    reads_after_arm: usize,
    full_replay_after_arm: usize,
    chain_len: usize,
    armed: usize,
    exec_entries: usize,
    class_key: String,
    tx_ns: Vec<u64>,
    raw_edges: Vec<(u32, u32)>,
}

fn run_once(loaded: &Loaded, engine: &str, workers: usize, seq_cpus: &[usize]) -> RunOut {
    let chain = PevmEthereum::mainnet();
    let cores = NonZeroUsize::new(workers.max(1)).unwrap();
    let pin = (engine == "seq" && !seq_cpus.is_empty()).then(|| AffinityGuard::pin(seq_cpus));
    let started = Instant::now();
    let result = match engine {
        "seq" => pevm::execute_revm_sequential(
            &chain,
            &loaded.storage,
            loaded.spec_id,
            loaded.block_env.clone(),
            loaded.txs.clone(),
        ),
        "occ" => {
            let mut pevm = Pevm::default();
            pevm.execute_revm_parallel(
                &chain,
                &loaded.storage,
                loaded.spec_id,
                loaded.block_env.clone(),
                loaded.txs.clone(),
                cores,
            )
        }
        "sf" => run_sf_block(
            &chain,
            &loaded.storage,
            loaded.spec_id,
            loaded.block_env.clone(),
            loaded.txs.clone(),
            SfOptions::fresh(cores, class_key()),
        ),
        other => panic!("unknown engine {other}"),
    };
    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    drop(pin);
    let mut out = RunOut {
        wall_ms,
        ok: result.is_ok(),
        err: result
            .as_ref()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default(),
        reexec: 0,
        full_replay: 0,
        reads_after_arm: 0,
        full_replay_after_arm: 0,
        chain_len: 0,
        armed: 0,
        exec_entries: 0,
        class_key: String::new(),
        tx_ns: Vec::new(),
        raw_edges: Vec::new(),
    };
    if engine == "sf"
        && let Some(trace) = pevm::specfence::last_trace()
    {
        out.reexec = trace.reexec;
        out.full_replay = trace.full_replay;
        out.reads_after_arm = trace.reads_after_arm;
        out.full_replay_after_arm = trace.full_replay_after_arm;
        out.chain_len = trace.chain_len;
        out.armed = trace.armed;
        out.exec_entries = trace.exec_entries;
        out.class_key = trace.class_key;
        out.tx_ns = trace.tx_ns;
        out.raw_edges = trace.raw_edges;
    }
    let _ = result;
    out
}

fn write_row(
    out: &mut dyn Write,
    loaded: &Loaded,
    workers: usize,
    engine: &str,
    round: usize,
    row: &RunOut,
) {
    let path = match engine {
        "seq" => "execute_revm_sequential",
        "occ" => "Pevm::execute_revm_parallel",
        "sf" => "run_sf_block",
        _ => engine,
    };
    let line = serde_json::json!({
        "block": loaded.block_no,
        "n_tx": loaded.n,
        "gas_used": loaded.gas_used,
        "workers": workers,
        "engine": engine,
        "path": path,
        "kind": "timed",
        "round": round,
        "wall_ms": row.wall_ms,
        "ok": row.ok,
        "err": row.err,
        "est": 0,
        "soft": 0,
        "occ_picks": 0,
        "spine_cores_max": 0,
        "phase_exec_n": 0,
        "phase_exec_ns": 0,
        "phase_pre_ns": 0,
        "phase_interp_ns": 0,
        "phase_post_ns": 0,
        "phase_val_ns": 0,
        "reexec_entries": row.reexec,
        "reexec": row.reexec,
        "full_replay": row.full_replay,
        "reads_after_arm": row.reads_after_arm,
        "full_replay_after_arm": row.full_replay_after_arm,
        "chain_len": row.chain_len,
        "armed": row.armed,
        "exec_entries": row.exec_entries,
        "class_key": row.class_key,
        "tx_ns": row.tx_ns,
        "raw_edges": row.raw_edges,
        "product_parallel": engine != "seq",
        "product_gate_fallback": false,
        "tps": if row.wall_ms > 0.0 { loaded.n as f64 / (row.wall_ms / 1000.0) } else { 0.0 },
        "n_attempts": 0,
        "n_seq": 0,
        "n_vals": 0,
        "beneficiary": 0,
        "boundary": serde_json::Value::Null,
        "attempts": [],
        "seq": [],
        "ge_1_5": false,
    });
    writeln!(out, "{line}").expect("write row");
    println!(
        "ROW block={} engine={} round={} workers={} wall_ms={:.3} ok={} reexec={} full_replay={} chain_len={} class_key={}",
        loaded.block_no,
        engine,
        round,
        workers,
        row.wall_ms,
        row.ok,
        row.reexec,
        row.full_replay,
        row.chain_len,
        row.class_key,
    );
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_str(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn parse_cpus(raw: &str) -> Vec<usize> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

fn main() {
    let which = env_str("SPECFENCE_INFLATION_WHICH", "scan");
    if which != "scan" {
        eprintln!("stage 1 harness only runs WHICH=scan (got {which})");
        std::process::exit(2);
    }
    let workers = env_usize("SPECFENCE_COMPARE_CORES", 4);
    let k = env_usize("SPECFENCE_INFLATION_K", 7);
    let seq_cpu = env_usize("SPECFENCE_INFLATION_SEQ_CPU", 0);
    let pin = parse_cpus(&std::env::var("SPECFENCE_PIN_CPUS").unwrap_or_default());
    let seq_cpus = if pin.is_empty() || pin.contains(&seq_cpu) {
        vec![seq_cpu]
    } else {
        vec![pin[0]]
    };
    let blocks: Vec<u64> = env_str("SPECFENCE_INFLATION_BLOCKS", "15274915,3356896")
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let out_path = std::env::var("SPECFENCE_INFLATION_OUT").ok();
    let mut out_file;
    let mut stdout = std::io::stdout();
    let out: &mut dyn Write = if let Some(path) = &out_path {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        out_file = File::create(path).expect("create out");
        &mut out_file
    } else {
        &mut stdout
    };
    let engines = ["seq", "occ", "sf"];
    for block_no in blocks {
        let loaded = load_block(block_no);
        for round in 0..k {
            for engine in engines {
                if !engine_selected(engine) {
                    continue;
                }
                let row = run_once(&loaded, engine, workers, &seq_cpus);
                write_row(out, &loaded, workers, engine, round, &row);
            }
        }
    }
}
