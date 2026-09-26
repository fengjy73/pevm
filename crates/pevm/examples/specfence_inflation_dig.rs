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

/// Mimalloc, selected at compile time. The default binary does not install a
/// global allocator: a runtime branch inside `alloc` slowed every engine,
/// sequential execution included.
#[cfg(feature = "specfence-mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn allocator_name() -> &'static str {
    if cfg!(feature = "specfence-mimalloc") {
        "mimalloc"
    } else {
        "system"
    }
}

fn allocator_requested_mimalloc() -> bool {
    if std::env::var("SPECFENCE_ALLOCATOR").ok().as_deref() == Some("mimalloc") {
        return true;
    }
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == "--allocator=mimalloc" {
            return true;
        }
        if arg == "--allocator" && args.next().as_deref() == Some("mimalloc") {
            return true;
        }
    }
    false
}

use alloy_primitives::Address;
use alloy_rpc_types_eth::Block;
use flate2::bufread::GzDecoder;
use hashbrown::HashMap;
use pevm::{
    BlockHashes, BuildSuffixHasher, EvmAccount, InMemoryStorage, Pevm,
    chain::{PevmChain, PevmEthereum},
    specfence::{SfAttempt, SfClassKey, SfOptions, run_sf_block},
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

fn engine_selected(name: &str, workers: usize) -> bool {
    // TPS_SEQ is the workers=1 baseline. Later C values do not re-time it.
    if name == "seq"
        && workers != 1
        && std::env::var("SPECFENCE_INFLATION_SEQ_ALWAYS")
            .ok()
            .as_deref()
            != Some("1")
    {
        return false;
    }
    match std::env::var("SPECFENCE_INFLATION_ENGINES") {
        Ok(list) if !list.trim().is_empty() => list.split(',').any(|s| s.trim() == name),
        _ => true,
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
    beneficiary: u64,
    delta_mismatch: usize,
    delta_abort: usize,
    learned_active: usize,
    learned_peak: usize,
    parked: usize,
    woken: usize,
    hops: usize,
    hops_same_worker: usize,
    hop_gap_max_ns: u64,
    pipelined_hops: usize,
    hot_location: u64,
    hot_hops: usize,
    hot_same: usize,
    hot_gap_max_ns: u64,
    hot_exec_ns: u64,
    hot_span_ns: u64,
    active_samples: Vec<u16>,
    gap_location: u64,
    gap_tx: u32,
    gap_prev_worker: u32,
    gap_reason: u8,
    delta_notes: Vec<pevm::specfence::DeltaNote>,
    attempts: Vec<SfAttempt>,
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
        "sf" => {
            pevm::specfence::set_timeline_block(loaded.block_no);
            run_sf_block(
                &chain,
                &loaded.storage,
                loaded.spec_id,
                loaded.block_env.clone(),
                loaded.txs.clone(),
                SfOptions::fresh(cores, class_key()),
            )
        }
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
        beneficiary: 0,
        delta_mismatch: 0,
        delta_abort: 0,
        learned_active: 0,
        learned_peak: 0,
        parked: 0,
        woken: 0,
        hops: 0,
        hops_same_worker: 0,
        hop_gap_max_ns: 0,
        pipelined_hops: 0,
        hot_location: 0,
        hot_hops: 0,
        hot_same: 0,
        hot_gap_max_ns: 0,
        hot_exec_ns: 0,
        hot_span_ns: 0,
        active_samples: Vec::new(),
        gap_location: 0,
        gap_tx: 0,
        gap_prev_worker: 0,
        gap_reason: 0,
        delta_notes: Vec::new(),
        attempts: Vec::new(),
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
        out.beneficiary = trace.beneficiary;
        out.delta_mismatch = trace.delta_mismatch;
        out.delta_abort = trace.delta_abort;
        out.learned_active = trace.learned_active;
        out.learned_peak = trace.learned_peak;
        out.parked = trace.parked;
        out.woken = trace.woken;
        out.hops = trace.hops;
        out.hops_same_worker = trace.hops_same_worker;
        out.hop_gap_max_ns = trace.hop_gap_max_ns;
        out.pipelined_hops = trace.pipelined_hops;
        out.hot_location = trace.hot_location;
        out.hot_hops = trace.hot_hops;
        out.hot_same = trace.hot_same;
        out.hot_gap_max_ns = trace.hot_gap_max_ns;
        out.hot_exec_ns = trace.hot_exec_ns;
        out.hot_span_ns = trace.hot_span_ns;
        out.active_samples = trace.active_samples;
        out.gap_location = trace.gap_location;
        out.gap_tx = trace.gap_tx;
        out.gap_prev_worker = trace.gap_prev_worker;
        out.gap_reason = trace.gap_reason;
        out.delta_notes = trace.delta_notes;
        out.attempts = trace.attempts;
    }
    let _ = result;
    out
}

fn attempts_json(attempts: &[SfAttempt]) -> serde_json::Value {
    serde_json::Value::Array(
        attempts
            .iter()
            .map(|a| {
                serde_json::json!({
                    "tx": a.tx,
                    "inc": a.inc,
                    "kind": a.kind,
                    "total_ns": a.total_ns,
                    "interp_ns": a.interp_ns,
                    "reads": a.reads,
                    "writes": a.writes,
                    "lazy_writes": a.lazy_writes,
                })
            })
            .collect(),
    )
}

#[allow(clippy::too_many_arguments)]
fn write_row(
    out: &mut dyn Write,
    loaded: &Loaded,
    workers: usize,
    engine: &str,
    kind: &str,
    round: usize,
    row: &RunOut,
    dump: bool,
) {
    let path = match engine {
        "seq" => "execute_revm_sequential",
        "occ" => "pevm@e94b0e3 execute_revm_parallel",
        "sf" => "run_sf_block",
        _ => engine,
    };
    let attempts = if dump {
        attempts_json(&row.attempts)
    } else {
        serde_json::json!([])
    };
    let line = serde_json::json!({
        "block": loaded.block_no,
        "n_tx": loaded.n,
        "gas_used": loaded.gas_used,
        "workers": workers,
        "engine": engine,
        "path": path,
        "kind": kind,
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
        "delta_mismatch": row.delta_mismatch,
        "delta_abort": row.delta_abort,
        "learned_active": row.learned_active,
        "learned_peak": row.learned_peak,
        "parked": row.parked,
        "woken": row.woken,
        "hops": row.hops,
        "hops_same_worker": row.hops_same_worker,
        "hop_gap_max_ns": row.hop_gap_max_ns,
        "pipelined_hops": row.pipelined_hops,
        "hot_location": row.hot_location,
        "hot_hops": row.hot_hops,
        "hot_same": row.hot_same,
        "hot_gap_max_ns": row.hot_gap_max_ns,
        "hot_exec_ns": row.hot_exec_ns,
        "hot_span_ns": row.hot_span_ns,
        "active_samples": row.active_samples,
        "gap_location": row.gap_location,
        "gap_tx": row.gap_tx,
        "gap_prev_worker": row.gap_prev_worker,
        "gap_reason": row.gap_reason,
        "delta_notes": row.delta_notes.iter().map(|note| serde_json::json!({
            "reader": note.reader,
            "location": note.location,
            "writer": note.writer,
            "reason": note.reason,
        })).collect::<Vec<_>>(),
        "armed": row.armed,
        "exec_entries": row.exec_entries,
        "class_key": row.class_key,
        "tx_ns": row.tx_ns,
        "raw_edges": row.raw_edges,
        "product_parallel": engine != "seq" && loaded.n >= workers && loaded.gas_used >= 4_000_000,
        "product_gate_fallback": loaded.n < workers || loaded.gas_used < 4_000_000,
        "tps": if row.wall_ms > 0.0 { loaded.n as f64 / (row.wall_ms / 1000.0) } else { 0.0 },
        "n_attempts": if dump { row.attempts.len() } else { 0 },
        "n_seq": 0,
        "n_vals": 0,
        "beneficiary": row.beneficiary,
        "boundary": serde_json::Value::Null,
        "attempts": attempts,
        "seq": [],
        "ge_1_5": false,
    });
    writeln!(out, "{line}").expect("write row");
    println!(
        "ROW block={} engine={} round={} workers={} wall_ms={:.3} ok={} reexec={} full_replay={} chain_len={} delta_mismatch={} delta_abort={} learned_active={} hops={} hops_same={} hop_gap_max_ns={} hot={:#x} hot_hops={}/{} hot_gap_us={:.1} hot_span_us={:.1} hot_exec_us={:.1} class_key={}",
        loaded.block_no,
        engine,
        round,
        workers,
        row.wall_ms,
        row.ok,
        row.reexec,
        row.full_replay,
        row.chain_len,
        row.delta_mismatch,
        row.delta_abort,
        row.learned_active,
        row.hops,
        row.hops_same_worker,
        row.hop_gap_max_ns,
        row.hot_location,
        row.hot_same,
        row.hot_hops,
        row.hot_gap_max_ns as f64 / 1000.0,
        row.hot_span_ns as f64 / 1000.0,
        row.hot_exec_ns as f64 / 1000.0,
        row.class_key,
    );
    if engine == "sf" && (row.gap_location != 0 || !row.active_samples.is_empty()) {
        println!(
            "CTRL active={:?} gap_loc={:#x} gap_tx={} gap_prev={} gap_reason={}",
            row.active_samples, row.gap_location, row.gap_tx, row.gap_prev_worker, row.gap_reason,
        );
    }
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

struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }

    fn shuffle<T>(&mut self, xs: &mut [T]) {
        for i in (1..xs.len()).rev() {
            let j = (self.next() as usize) % (i + 1);
            xs.swap(i, j);
        }
    }
}

fn block_list() -> Vec<u64> {
    let raw = std::env::var("SPECFENCE_COMPARE_BLOCK")
        .or_else(|_| std::env::var("SPECFENCE_INFLATION_BLOCKS"))
        .unwrap_or_else(|_| "15274915,3356896".to_string());
    raw.split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

fn receipt_check(loaded: &Loaded, workers: usize, n: usize, label: &str) {
    let cores = NonZeroUsize::new(workers.max(1)).unwrap();
    let chain = PevmEthereum::mainnet();
    let mut diverge = 0usize;
    for i in 0..n {
        let seq = pevm::execute_revm_sequential(
            &chain,
            &loaded.storage,
            loaded.spec_id,
            loaded.block_env.clone(),
            loaded.txs.clone(),
        );
        let mut pevm = Pevm::default();
        let par = pevm.execute_revm_parallel(
            &chain,
            &loaded.storage,
            loaded.spec_id,
            loaded.block_env.clone(),
            loaded.txs.clone(),
            cores,
        );
        match (seq, par) {
            (Ok(s), Ok(p)) if s == p => {
                let gas = s.last().map(|r| r.receipt.cumulative_gas_used).unwrap_or(0);
                println!(
                    "{label}=seq ok block={} iter={i} txs={} gas={gas}",
                    loaded.block_no,
                    s.len()
                );
            }
            (Ok(s), Ok(p)) => {
                diverge += 1;
                println!(
                    "{label}!=seq block={} iter={i} len {} {}",
                    loaded.block_no,
                    s.len(),
                    p.len()
                );
            }
            (Err(e), _) => {
                diverge += 1;
                println!("seq err block={} iter={i} {e}", loaded.block_no);
            }
            (_, Err(e)) => {
                diverge += 1;
                println!("{label} err block={} iter={i} {e}", loaded.block_no);
            }
        }
    }
    println!(
        "{} block={} n={n} workers={workers} diverge={diverge} upstream=e94b0e3",
        label.to_ascii_uppercase(),
        loaded.block_no
    );
    if diverge > 0 {
        std::process::exit(1);
    }
}

struct Cli {
    cpu_list: Option<String>,
    workers: Option<usize>,
}

fn apply_cli() -> Cli {
    let mut cpu_list = None;
    let mut workers = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let (key, inline) = if let Some((key, value)) = arg.split_once('=') {
            (key, Some(value.to_string()))
        } else {
            (arg.as_str(), None)
        };
        let mut value = || inline.clone().or_else(|| args.next()).unwrap_or_default();
        match key {
            "--cpu-list" => cpu_list = Some(value()),
            "--workers" => workers = value().parse().ok(),
            _ => {}
        }
    }
    Cli { cpu_list, workers }
}

fn harness_line() {
    let code = std::env::var("SPECFENCE_SHARED_CODE").ok();
    let cache = std::env::var("SPECFENCE_SHARED_CACHE").ok();
    let name = allocator_name();
    if allocator_requested_mimalloc() && name != "mimalloc" {
        eprintln!(
            "HARNESS allocator mimalloc needs --features specfence-mimalloc; this binary uses system malloc"
        );
        std::process::exit(2);
    }
    println!(
        "HARNESS allocator={name} shared_code={} shared_cache={}",
        code.as_deref().unwrap_or("1"),
        cache.as_deref().unwrap_or("1"),
    );
}

fn rank_block(loaded: &Loaded, workers: usize, pin: &[usize], seq_cpus: &[usize]) {
    if std::env::var("SPECFENCE_FORCE_PARALLEL").ok().as_deref() == Some("1") {
        eprintln!("rank refuses SPECFENCE_FORCE_PARALLEL; the fast leg must stay serial");
        std::process::exit(2);
    }
    if std::env::var("SPECFENCE_INFLATION").ok().as_deref() != Some("1") {
        eprintln!("rank needs SPECFENCE_INFLATION=1 so attempts carry interp_ns");
        std::process::exit(2);
    }
    pevm::specfence::prepare_workers(seq_cpus, 1);
    let fast = run_once(loaded, "sf", 1, seq_cpus);
    let mut fast_ns = vec![0u64; loaded.n];
    for attempt in &fast.attempts {
        if attempt.kind == 1 {
            fast_ns[attempt.tx as usize] = attempt.interp_ns;
        }
    }
    pevm::specfence::prepare_workers(pin, workers);
    let par = run_once(loaded, "sf", workers, seq_cpus);
    let mut par_ns = vec![0u64; loaded.n];
    let mut attempts_n = vec![0u32; loaded.n];
    let mut reads_n = vec![0usize; loaded.n];
    let mut writes_n = vec![0usize; loaded.n];
    let mut write0 = vec![0u64; loaded.n];
    for attempt in &par.attempts {
        let i = attempt.tx as usize;
        if i >= loaded.n {
            continue;
        }
        par_ns[i] = par_ns[i].saturating_add(attempt.interp_ns);
        attempts_n[i] = attempts_n[i].saturating_add(1);
        if attempt.kind == 1 {
            reads_n[i] = attempt.reads.len();
            writes_n[i] = attempt.writes.len() + attempt.lazy_writes.len();
            write0[i] = attempt
                .writes
                .first()
                .or(attempt.lazy_writes.first())
                .copied()
                .unwrap_or(0);
        }
    }
    let mut order: Vec<usize> = (0..loaded.n).collect();
    order.sort_by(|&a, &b| {
        let ra = ratio(par_ns[a], fast_ns[a]);
        let rb = ratio(par_ns[b], fast_ns[b]);
        rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal)
    });
    let fast_sum: u64 = fast_ns.iter().sum();
    let par_sum: u64 = par_ns.iter().sum();
    println!(
        "RANK_SUM block={} workers={workers} n={} fast_interp_ms={:.3} par_interp_ms={:.3} ratio={:.3} wall_fast_ms={:.3} wall_par_ms={:.3} ok={} {}",
        loaded.block_no,
        loaded.n,
        fast_sum as f64 / 1e6,
        par_sum as f64 / 1e6,
        ratio(par_sum, fast_sum),
        fast.wall_ms,
        par.wall_ms,
        fast.ok && par.ok,
        allocator_name(),
    );
    for tx in order.into_iter().take(10) {
        let env = &loaded.txs[tx];
        let to = env
            .kind
            .to()
            .map(|a| format!("{a}"))
            .unwrap_or_else(|| "create".to_string());
        let selector = if env.data.len() >= 4 {
            format!(
                "0x{:02x}{:02x}{:02x}{:02x}",
                env.data[0], env.data[1], env.data[2], env.data[3]
            )
        } else {
            "transfer".to_string()
        };
        println!(
            "RANK tx={tx} to={to} sel={selector} fast_us={:.1} par_us={:.1} ratio={:.2} attempts={} reads={} writes={} write0={:#x}",
            fast_ns[tx] as f64 / 1e3,
            par_ns[tx] as f64 / 1e3,
            ratio(par_ns[tx], fast_ns[tx]),
            attempts_n[tx],
            reads_n[tx],
            writes_n[tx],
            write0[tx],
        );
    }
}

fn ratio(numer: u64, denom: u64) -> f64 {
    if denom == 0 {
        0.0
    } else {
        numer as f64 / denom as f64
    }
}

fn main() {
    let cli = apply_cli();
    harness_line();
    let which = env_str("SPECFENCE_INFLATION_WHICH", "scan");
    let workers = cli
        .workers
        .unwrap_or_else(|| env_usize("SPECFENCE_COMPARE_CORES", 4));
    let blocks = block_list();
    if which == "steptrace" {
        let out_path = std::env::var("SPECFENCE_INFLATION_OUT").ok();
        let mut out_file;
        let mut stdout = std::io::stdout();
        let out: &mut dyn Write = if let Some(path) = out_path.as_ref() {
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent).ok();
            }
            out_file = File::create(path).expect("create out");
            &mut out_file
        } else {
            &mut stdout
        };
        // Stage 1 has no opcode hook. A meta line keeps the scan's file
        // non-empty. The report ignores rows without `txs`, so TPS_ideal_step
        // stays absent.
        writeln!(
            out,
            "{}",
            serde_json::json!({"meta": true, "note": "stage 1 has no opcode step trace"})
        )
        .expect("write steptrace");
        println!("STEPTRACE stage1 no opcode hook");
        return;
    }
    if which == "spawn" {
        // Same `thread::scope` shape as `Pevm::execute_revm_parallel`, without
        // entering the upstream scheduler. The clock is spawn plus join.
        let k = env_usize("SPECFENCE_INFLATION_K", 10);
        let list =
            std::env::var("SPECFENCE_SPAWN_LIST").unwrap_or_else(|_| "4,8,16,32".to_string());
        for part in list.split(',') {
            let c: usize = part.trim().parse().unwrap_or(0);
            if c == 0 {
                continue;
            }
            let mut samples = Vec::with_capacity(k);
            for _ in 0..k {
                let started = Instant::now();
                std::thread::scope(|scope| {
                    for _ in 0..c {
                        scope.spawn(|| {
                            std::hint::black_box(c);
                        });
                    }
                });
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mid = samples[samples.len() / 2];
            println!(
                "SPAWN workers={c} k={k} median_ms={mid:.4} min_ms={:.4} max_ms={:.4}",
                samples[0],
                samples[samples.len() - 1],
            );
        }
        return;
    }
    if which == "seqcheck" || which == "occcheck" {
        let n = if which == "seqcheck" {
            env_usize("SPECFENCE_INFLATION_SEQCHECK_N", 10)
        } else {
            env_usize("SPECFENCE_INFLATION_OCCCHECK_N", 3)
        };
        for block_no in blocks {
            let loaded = load_block(block_no);
            receipt_check(&loaded, workers, n, &which);
        }
        return;
    }
    if which != "scan" && which != "rank" {
        eprintln!("unknown WHICH={which}");
        std::process::exit(2);
    }
    let k = env_usize("SPECFENCE_INFLATION_K", 10);
    let oracle_k = env_usize("SPECFENCE_INFLATION_ORACLE_K", 0);
    let seq_cpu = env_usize("SPECFENCE_INFLATION_SEQ_CPU", 0);
    let seed = env_usize("SPECFENCE_INFLATION_SEED", 1) as u64;
    let dump = std::env::var("SPECFENCE_INFLATION_DUMP").ok().as_deref() == Some("1");
    let pin = match cli.cpu_list {
        Some(list) => parse_cpus(&list),
        None => parse_cpus(&std::env::var("SPECFENCE_PIN_CPUS").unwrap_or_default()),
    };
    let seq_cpus = if pin.is_empty() || pin.contains(&seq_cpu) {
        vec![seq_cpu]
    } else {
        vec![pin[0]]
    };
    if which == "rank" {
        for block_no in blocks {
            let loaded = load_block(block_no);
            rank_block(&loaded, workers, &pin, &seq_cpus);
        }
        return;
    }
    // Threads are created here, before the timed rounds. `dispatch` reuses
    // this pool when the environment list is empty or already applied.
    pevm::specfence::prepare_workers(&pin, workers);
    let out_path = std::env::var("SPECFENCE_INFLATION_OUT").ok();
    let mut out_file;
    let mut stdout = std::io::stdout();
    let out: &mut dyn Write = if let Some(path) = out_path.as_ref() {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        out_file = File::create(path).expect("create out");
        &mut out_file
    } else {
        &mut stdout
    };
    let mut engines = ["seq", "occ", "sf"];
    for block_no in blocks {
        let loaded = load_block(block_no);
        let mut rng = Rng(seed ^ block_no.wrapping_mul(0x9E37));
        for round in 0..k {
            rng.shuffle(&mut engines);
            for engine in engines {
                if !engine_selected(engine, workers) {
                    continue;
                }
                let row = run_once(&loaded, engine, workers, &seq_cpus);
                write_row(out, &loaded, workers, engine, "timed", round, &row, dump);
            }
        }
        if oracle_k > 0 {
            for engine in engines {
                if !engine_selected(engine, workers) {
                    continue;
                }
                for round in 0..oracle_k {
                    let row = run_once(&loaded, engine, workers, &seq_cpus);
                    write_row(out, &loaded, workers, engine, "oracle", round, &row, false);
                }
            }
        }
    }
}
