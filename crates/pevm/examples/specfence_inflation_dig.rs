//! Measurement harness for the Soft=0 execute-inflation dig.
//!
//! Timed region, per engine, after the tx list and block env are built
//! (in-memory state in, results out; no state root):
//!
//! - `seq`: [`pevm::execute_revm_sequential`] — CacheDB + `transact` + commit.
//!   Same binary. Not OCC at one worker.
//! - `occ`: [`Pevm::execute_revm_parallel`] with [`ConcurrencyMode::Occ`]
//!   (`next_occ_task` / `try_execute` / `validate_occ_stage`).
//! - `sf`: [`Pevm::execute_revm_parallel`] with [`ConcurrencyMode::SpecFence`]
//!   (`run_sf_block`). Not the OCC worker loop, and not the `Pevm::execute`
//!   gas / `n_tx < workers` sequential fallback.
//!
//! Every timed round builds a fresh [`Pevm`] (SpecFence learned state reset).
//! There is no untimed warm-up. Same-instance reuse is `oracle` only.
//!
//! ```text
//! SPECFENCE_INFLATION_WHICH=scan SPECFENCE_COMPARE_CORES=4 \
//! SPECFENCE_INFLATION_K=10 SPECFENCE_PIN_CPUS=0,1,2,3 \
//! taskset -c 0-3 target/release/examples/specfence_inflation_dig
//! ```

#![allow(missing_docs)]

use std::{
    fs::File,
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
    chain::{PevmChain, PevmEthereum},
    execute_revm_sequential, BlockHashes, BuildSuffixHasher, Bytecodes, ConcurrencyMode,
    EvmAccount, InMemoryStorage, Pevm, PevmTxExecutionResult,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
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

fn n_tx(block: &Block<<PevmEthereum as PevmChain>::Transaction>) -> usize {
    match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs.len(),
        alloy_rpc_types_eth::BlockTransactions::Hashes(h) => h.len(),
        alloy_rpc_types_eth::BlockTransactions::Uncle => 0,
    }
}

fn host_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("model name"))
        .and_then(|s| s.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Pin the calling thread to one cpu, then restore the previous mask.
struct AffinityGuard {
    prev: [u8; 128],
    ok: bool,
}

impl AffinityGuard {
    fn pin_one(cpu: usize) -> Self {
        let mut prev = [0u8; 128];
        let got = unsafe { sched_getaffinity(0, prev.len(), prev.as_mut_ptr()) };
        let mut mask = [0u8; 128];
        if cpu < 1024 {
            mask[cpu / 8] = 1 << (cpu % 8);
        }
        let set = unsafe { sched_setaffinity(0, mask.len(), mask.as_ptr()) };
        Self {
            prev,
            ok: got == 0 && set == 0,
        }
    }
}

impl Drop for AffinityGuard {
    fn drop(&mut self) {
        if self.ok {
            unsafe { sched_setaffinity(0, self.prev.len(), self.prev.as_ptr()) };
        }
    }
}

unsafe extern "C" {
    fn sched_setaffinity(pid: i32, cpusetsize: usize, mask: *const u8) -> i32;
    fn sched_getaffinity(pid: i32, cpusetsize: usize, mask: *mut u8) -> i32;
}

#[derive(Clone, Copy)]
struct Engine {
    name: &'static str,
    /// Code path actually timed. See the module docs.
    path: &'static str,
    mode: ConcurrencyMode,
    sequential: bool,
    tag: u8,
}

const PATH_SEQ: &str = "execute_revm_sequential";
const PATH_OCC: &str = "execute_revm_parallel/Occ";
const PATH_SF: &str = "execute_revm_parallel/SpecFence/run_sf_block";

const ENGINES: [Engine; 3] = [
    Engine {
        name: "seq",
        path: PATH_SEQ,
        mode: ConcurrencyMode::Occ,
        sequential: true,
        tag: pevm::specfence::TAG_SEQ_TX,
    },
    Engine {
        name: "occ",
        path: PATH_OCC,
        mode: ConcurrencyMode::Occ,
        sequential: false,
        tag: pevm::specfence::TAG_OCC,
    },
    Engine {
        name: "sf",
        path: PATH_SF,
        mode: ConcurrencyMode::SpecFence,
        sequential: false,
        tag: pevm::specfence::TAG_SF,
    },
];

struct Loaded {
    block_no: u64,
    block: Block<<PevmEthereum as PevmChain>::Transaction>,
    storage: InMemoryStorage,
    chain: PevmEthereum,
    n: usize,
    gas_used: u64,
}

fn load_block(
    data_dir: &Path,
    block_no: u64,
    bytecodes: Arc<Bytecodes>,
    block_hashes: Arc<BlockHashes>,
) -> Loaded {
    let dir = data_dir.join("blocks").join(block_no.to_string());
    let block: Block<<PevmEthereum as PevmChain>::Transaction> = serde_json::from_reader(
        BufReader::new(File::open(dir.join("block.json")).expect("block.json")),
    )
    .expect("parse block");
    let accounts: HashMap<alloy_primitives::Address, EvmAccount, BuildSuffixHasher> =
        serde_json::from_reader(BufReader::new(
            File::open(dir.join("pre_state.json")).expect("pre_state"),
        ))
        .expect("parse pre_state");
    let gas_used = block.header.gas_used;
    let n = n_tx(&block);
    Loaded {
        block_no,
        block,
        storage: InMemoryStorage::new(accounts, bytecodes, block_hashes),
        chain: PevmEthereum::mainnet(),
        n,
        gas_used,
    }
}

fn fresh(engine: Engine) -> Pevm {
    let mut pevm = Pevm::with_concurrency_mode(engine.mode);
    if engine.mode == ConcurrencyMode::SpecFence {
        pevm.reset_heat();
        pevm.reset_inter_prior();
    }
    pevm
}

struct RunOut {
    wall_ms: f64,
    ok: bool,
    err: String,
    est: usize,
    soft: usize,
    occ_picks: usize,
    spine_cores_max: usize,
    phase_exec_n: usize,
    phase_exec_ns: u64,
    phase_pre_ns: u64,
    phase_interp_ns: u64,
    phase_post_ns: u64,
    phase_val_ns: u64,
    reexec_entries: usize,
}

fn build_inputs(
    loaded: &Loaded,
) -> (
    <PevmEthereum as PevmChain>::EvmSpecId,
    revm::context::BlockEnv,
    Vec<<PevmEthereum as PevmChain>::EvmTx>,
) {
    let spec_id = loaded
        .chain
        .get_block_spec(&loaded.block.header)
        .expect("block spec");
    let block_env = pevm::get_block_env(&loaded.block.header, spec_id);
    let txs = match &loaded.block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs
            .iter()
            .map(|tx| loaded.chain.get_tx_env(tx).expect("tx env"))
            .collect(),
        _ => panic!("block {} has no full tx list", loaded.block_no),
    };
    (spec_id, block_env, txs)
}

fn run_once(
    pevm: &mut Pevm,
    engine: Engine,
    loaded: &Loaded,
    workers: usize,
    seq_cpu: usize,
) -> RunOut {
    pevm::specfence::inflation_set_tag(engine.tag);
    let cores = NonZeroUsize::new(workers.max(1)).unwrap();
    // Tx list and block env are inputs. Cloning them is outside the timer
    // because the REVM entry takes ownership.
    let (spec_id, block_env, txs) = build_inputs(loaded);
    let _pin = engine.sequential.then(|| AffinityGuard::pin_one(seq_cpu));
    let (wall_ms, result) = {
        let _bound = pevm::specfence::InflationBoundGuard::enter();
        let t0 = Instant::now();
        let result = if engine.sequential {
            execute_revm_sequential(&loaded.chain, &loaded.storage, spec_id, block_env, txs)
        } else {
            pevm.execute_revm_parallel(
                &loaded.chain,
                &loaded.storage,
                spec_id,
                block_env,
                txs,
                cores,
            )
        };
        (t0.elapsed().as_secs_f64() * 1000.0, result)
    };
    match result {
        Ok(_) => {
            let m = pevm.last_specfence_metrics();
            let spine = pevm.last_spine();
            RunOut {
                wall_ms,
                ok: true,
                err: String::new(),
                est: m.estimate_block_sf,
                soft: m.soft_wait_arms,
                occ_picks: m.occ_schedule_picks,
                spine_cores_max: spine.spine_cores_max,
                phase_exec_n: m.phase_exec_n,
                phase_exec_ns: m.phase_exec_ns,
                phase_pre_ns: m.phase_pre_ns,
                phase_interp_ns: m.phase_interp_ns,
                phase_post_ns: m.phase_post_ns,
                phase_val_ns: m.phase_val_ns,
                reexec_entries: m.reexec_entries,
            }
        }
        Err(e) => RunOut {
            wall_ms,
            ok: false,
            err: e.to_string(),
            est: 0,
            soft: 0,
            occ_picks: 0,
            spine_cores_max: 0,
            phase_exec_n: 0,
            phase_exec_ns: 0,
            phase_pre_ns: 0,
            phase_interp_ns: 0,
            phase_post_ns: 0,
            phase_val_ns: 0,
            reexec_entries: 0,
        },
    }
}

fn write_row(
    out: &mut dyn Write,
    loaded: &Loaded,
    workers: usize,
    engine: Engine,
    kind: &str,
    round: usize,
    row: &RunOut,
    dump: bool,
) {
    let snap = if pevm::specfence::inflation_enabled() {
        Some(pevm::specfence::inflation_drain())
    } else {
        None
    };
    let (attempts, seq, vals, beneficiary, boundary) = match &snap {
        Some(s) => (
            if dump {
                serde_json::to_value(&s.attempts).unwrap()
            } else {
                serde_json::json!([])
            },
            if dump {
                serde_json::to_value(&s.seq).unwrap()
            } else {
                serde_json::json!([])
            },
            serde_json::json!(s.vals.len()),
            s.beneficiary,
            serde_json::json!({
                "enter_to_return_ns": s.boundary.enter_to_return_ns,
                "f_pre_ns": s.boundary.f_pre_ns,
                "f_post_ns": s.boundary.f_post_ns,
                "worker_seen": s.boundary.worker_seen,
            }),
        ),
        None => (
            serde_json::json!([]),
            serde_json::json!([]),
            serde_json::json!(0),
            0,
            serde_json::json!(null),
        ),
    };
    let n_attempts = snap.as_ref().map(|s| s.attempts.len()).unwrap_or(0);
    let n_seq = snap.as_ref().map(|s| s.seq.len()).unwrap_or(0);
    let line = serde_json::json!({
        "block": loaded.block_no,
        "n_tx": loaded.n,
        "gas_used": loaded.gas_used,
        "workers": workers,
        "engine": engine.name,
        "path": engine.path,
        "kind": kind,
        "round": round,
        "wall_ms": row.wall_ms,
        "ok": row.ok,
        "err": row.err,
        "est": row.est,
        "soft": row.soft,
        "occ_picks": row.occ_picks,
        "spine_cores_max": row.spine_cores_max,
        "phase_exec_n": row.phase_exec_n,
        "phase_exec_ns": row.phase_exec_ns,
        "phase_pre_ns": row.phase_pre_ns,
        "phase_interp_ns": row.phase_interp_ns,
        "phase_post_ns": row.phase_post_ns,
        "phase_val_ns": row.phase_val_ns,
        "reexec_entries": row.reexec_entries,
        "product_parallel": !engine.sequential
            && loaded.n >= workers
            && loaded.gas_used >= 4_000_000,
        "product_gate_fallback": loaded.n < workers || loaded.gas_used < 4_000_000,
        "tps": if row.wall_ms > 0.0 {
            loaded.n as f64 / (row.wall_ms / 1000.0)
        } else {
            0.0
        },
        "n_attempts": n_attempts,
        "n_seq": n_seq,
        "n_vals": vals,
        "beneficiary": beneficiary,
        "boundary": boundary,
        "attempts": attempts,
        "seq": seq,
        "ge_1_5": false,
    });
    writeln!(out, "{line}").expect("write");
    println!(
        "ROW block={} engine={} kind={} round={} wall_ms={:.3} ok={} est={} soft={} occ_picks={} spine_cores_max={} phase_exec_n={} ge_1_5=false",
        loaded.block_no,
        engine.name,
        kind,
        round,
        row.wall_ms,
        row.ok,
        row.est,
        row.soft,
        row.occ_picks,
        row.spine_cores_max,
        row.phase_exec_n,
    );
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
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

fn engine_selected(name: &str) -> bool {
    match std::env::var("SPECFENCE_INFLATION_ENGINES") {
        Ok(s) if !s.trim().is_empty() => s.split(',').any(|p| p.trim() == name),
        _ => true,
    }
}

fn run_block(
    loaded: &Loaded,
    workers: usize,
    k: usize,
    oracle_k: usize,
    seq_cpu: usize,
    seed: u64,
    dump: bool,
    out: &mut dyn Write,
) {
    let mut rng = Rng(seed ^ loaded.block_no.wrapping_mul(0x9E37));
    let mut order = [0usize, 1, 2];
    // Every round is timed. Fresh engine each round. No untimed warm-up.
    for r in 0..k {
        rng.shuffle(&mut order);
        for &i in &order {
            let engine = ENGINES[i];
            if !engine_selected(engine.name) {
                continue;
            }
            let mut pevm = fresh(engine);
            let row = run_once(&mut pevm, engine, loaded, workers, seq_cpu);
            write_row(out, loaded, workers, engine, "timed", r, &row, dump);
        }
    }
    if oracle_k > 0 {
        for &engine in &ENGINES {
            if !engine_selected(engine.name) {
                continue;
            }
            let mut pevm = fresh(engine);
            for r in 0..oracle_k {
                let row = run_once(&mut pevm, engine, loaded, workers, seq_cpu);
                write_row(out, loaded, workers, engine, "oracle", r, &row, false);
            }
        }
    }
}

fn seqcheck(loaded: &Loaded, workers: usize, n: usize) {
    let cores = NonZeroUsize::new(workers.max(1)).unwrap();
    let mut diverge = 0usize;
    for i in 0..n {
        let mut checker = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        checker.reset_heat();
        checker.reset_inter_prior();
        let par = checker.execute(&loaded.chain, &loaded.storage, &loaded.block, cores, false);
        let seq = checker.execute(&loaded.chain, &loaded.storage, &loaded.block, cores, true);
        match (par, seq) {
            (Ok(p), Ok(s)) if p == s => {
                println!(
                    "seq=par ok block={} iter={i} commit_rejects={} reject_txs={:?}",
                    loaded.block_no,
                    pevm::specfence_commit_rejects(),
                    pevm::specfence_commit_reject_txs(),
                );
            }
            (Ok(p), Ok(s)) => {
                diverge += 1;
                println!(
                    "seq!=par block={} iter={i} commit_rejects={} reject_txs={:?}",
                    loaded.block_no,
                    pevm::specfence_commit_rejects(),
                    pevm::specfence_commit_reject_txs(),
                );
                report_diverge(&s, &p);
            }
            (Err(e), _) => println!("par err block={} iter={i} {e}", loaded.block_no),
            (_, Err(e)) => println!("seq err block={} iter={i} {e}", loaded.block_no),
        }
    }
    println!(
        "SEQCHECK block={} n={n} diverge={diverge} ge_1_5=false",
        loaded.block_no
    );
}

fn inblock_once(loaded: &Loaded, workers: usize) {
    let cores = NonZeroUsize::new(workers.max(1)).unwrap();
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    pevm.reset_heat();
    pevm.reset_inter_prior();
    for label in ["fresh", "carry"] {
        let t0 = std::time::Instant::now();
        let result = pevm.execute(&loaded.chain, &loaded.storage, &loaded.block, cores, false);
        let wall_ms = t0.elapsed().as_secs_f64() * 1e3;
        let ok = result.is_ok();
        println!(
            "INBLOCK block={} label={label} workers={workers} n={} ok={ok} wall_ms={wall_ms:.3}",
            loaded.block_no, loaded.n
        );
        print_inblock(&pevm);
    }
}

fn print_inblock(pevm: &Pevm) {
    let m = pevm.last_specfence_metrics();
    let spine = pevm.last_spine();
    let trace = pevm::specfence::inblock_snapshot();
    let incs = pevm.last_incarnations();
    println!(
        "  reexec_entries={} incarnation_gt0={} full_replay={} resolve_after_fail={} protect_n={} protect_before_opt={} replay_after_protect={}",
        m.reexec_entries,
        m.incarnation_gt0,
        m.resolve_full_replay,
        m.sf_resolve_after_fail_n,
        m.sf_protect_n,
        m.sf_protect_before_opt_n,
        m.sf_replay_after_protect_n,
    );
    println!(
        "  wait_once={} wait_suppressed={} wait_once_consume={} detect_before={} avoid_publish={} visibility_opt={} ordered_tip={}",
        m.access_wait_once,
        m.access_wait_suppressed,
        m.sf_wait_once_consume_n,
        m.sf_detect_before_n,
        m.sf_avoid_publish_n,
        m.visibility_opt,
        m.visibility_ordered_tip,
    );
    println!(
        "  spine chain_len={} armed_locs={} indep_first_cuts={} prior_radar_only={} ordered_defer={} ordered_handoff={} raw={} waw={} war={}",
        spine.chain_len,
        spine.armed_locs,
        spine.indep_first_cuts,
        spine.prior_radar_only,
        spine.ordered_defer,
        spine.ordered_handoff,
        spine.raw_n,
        spine.waw_n,
        spine.war_n,
    );
    let first_arm = if trace.first_arm_tx == usize::MAX {
        "none".to_string()
    } else {
        trace.first_arm_tx.to_string()
    };
    println!(
        "  trace arm_n={} first_arm_tx={first_arm} first_arm_ns={} consult_no_pred={} consult_opt={}",
        trace.arm_n, trace.first_arm_ns, trace.consult_no_pred, trace.consult_opt,
    );
    if incs.is_empty() {
        println!("  inc_hist empty");
        return;
    }
    let mut hist = [0usize; 5];
    for &inc in incs {
        hist[inc.min(4)] += 1;
    }
    println!(
        "  inc_hist 0={} 1={} 2={} 3={} 4+={}",
        hist[0], hist[1], hist[2], hist[3], hist[4]
    );
    let n = incs.len();
    for b in 0..10 {
        let lo = b * n / 10;
        let hi = (b + 1) * n / 10;
        let sum: usize = incs[lo..hi].iter().copied().sum();
        let gt0 = incs[lo..hi].iter().filter(|&&x| x > 0).count();
        println!("  decile {b} tx[{lo},{hi}) reexec_sum={sum} txs_gt0={gt0}");
    }
    let arm_ns = trace.first_arm_ns;
    let starts = pevm.last_tx_first_start();
    if arm_ns > 0 && starts.len() == incs.len() {
        let mut before = 0usize;
        let mut after = 0usize;
        let mut before_re = 0usize;
        let mut after_re = 0usize;
        for (tx, &inc) in incs.iter().enumerate() {
            let started = starts[tx];
            if started == 0 {
                continue;
            }
            if started < arm_ns {
                before += 1;
                if inc > 0 {
                    before_re += 1;
                }
            } else {
                after += 1;
                if inc > 0 {
                    after_re += 1;
                }
            }
        }
        println!(
            "  start_vs_first_arm before={before} before_reexec={before_re} after={after} after_reexec={after_re}"
        );
    }
}

fn brief_acct(v: Option<&Option<EvmAccount>>) -> String {
    match v {
        None => "absent".into(),
        Some(None) => "deleted".into(),
        Some(Some(a)) => format!(
            "bal={} nonce={} storage={}",
            a.balance,
            a.nonce,
            a.storage.len()
        ),
    }
}

fn report_diverge(seq: &[PevmTxExecutionResult], par: &[PevmTxExecutionResult]) {
    if seq.len() != par.len() {
        println!("  len seq={} par={}", seq.len(), par.len());
    }
    let n = seq.len().min(par.len());
    for i in 0..n {
        if seq[i] == par[i] {
            continue;
        }
        println!(
            "  first_tx={i} seq_gas={} par_gas={} seq_logs={} par_logs={} seq_status={:?} par_status={:?} seq_state={} par_state={}",
            seq[i].receipt.cumulative_gas_used,
            par[i].receipt.cumulative_gas_used,
            seq[i].receipt.logs.len(),
            par[i].receipt.logs.len(),
            seq[i].receipt.status,
            par[i].receipt.status,
            seq[i].state.len(),
            par[i].state.len(),
        );
        let mut shown = 0;
        let mut keys: Vec<_> = seq[i]
            .state
            .keys()
            .chain(par[i].state.keys())
            .copied()
            .collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            let a = seq[i].state.get(&k);
            let b = par[i].state.get(&k);
            if a != b {
                println!("  acct {k:?} seq={} par={}", brief_acct(a), brief_acct(b));
                shown += 1;
                if shown >= 6 {
                    break;
                }
            }
        }
        return;
    }
    println!("  results differ past min len or only in trailing txs");
}

fn main() {
    let which = std::env::var("SPECFENCE_INFLATION_WHICH").unwrap_or_else(|_| "scan".into());
    let data_dir = repo_root().join("data/ethereum");
    let (bytecodes, hashes) = load_shared(&data_dir);
    let workers = env_usize("SPECFENCE_COMPARE_CORES", 4);
    let k = env_usize("SPECFENCE_INFLATION_K", 10);
    let oracle_k = env_usize("SPECFENCE_INFLATION_ORACLE_K", 0);
    let seq_cpu = env_usize("SPECFENCE_INFLATION_SEQ_CPU", 0);
    let seed = env_usize("SPECFENCE_INFLATION_SEED", 1) as u64;
    let dump = std::env::var("SPECFENCE_INFLATION_DUMP").ok().as_deref() == Some("1");
    let host_cpus = std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count();
    println!(
        "HOST cpus={host_cpus} model=\"{}\" loadavg={} which={which} workers={workers} k={k} oracle_k={oracle_k} seq_cpu={seq_cpu} pin={} dump={dump} inflation={} reads={} os={} perf={} dag={} ge_1_5=false",
        host_model(),
        loadavg(),
        std::env::var("SPECFENCE_PIN_CPUS").unwrap_or_default(),
        pevm::specfence::inflation_enabled(),
        std::env::var("SPECFENCE_INFLATION_READS").unwrap_or_default(),
        std::env::var("SPECFENCE_INFLATION_OS").unwrap_or_default(),
        std::env::var("SPECFENCE_INFLATION_PERF").unwrap_or_default(),
        std::env::var("SPECFENCE_INFLATION_DAG").unwrap_or_default(),
    );

    if which == "steptrace" {
        let blocks: Vec<u64> = if let Ok(list) = std::env::var("SPECFENCE_INFLATION_BLOCKS") {
            list.split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect()
        } else {
            vec![15_274_915, 3_356_896]
        };
        let out_path = std::env::var("SPECFENCE_INFLATION_OUT")
            .unwrap_or_else(|_| "results/soft0-execute-inflation/scan/step-trace.jsonl".into());
        if let Some(parent) = Path::new(&out_path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut out = File::create(&out_path).expect("step trace out");
        let engine = ENGINES[1];
        for block_no in blocks {
            let loaded = load_block(
                &data_dir,
                block_no,
                Arc::clone(&bytecodes),
                Arc::clone(&hashes),
            );
            println!(
                "STEPTRACE block={block_no} n={} gas={} k={k} path={}",
                loaded.n, loaded.gas_used, engine.path
            );
            for r in 0..k {
                let mut pevm = fresh(engine);
                let row = run_once(&mut pevm, engine, &loaded, workers, seq_cpu);
                let txs = pevm::specfence::step_drain();
                let ben = pevm::specfence::step_beneficiary();
                let line = serde_json::json!({
                    "meta": true,
                    "kind": "step_trace",
                    "block": block_no,
                    "round": r,
                    "n_tx": loaded.n,
                    "gas_used": loaded.gas_used,
                    "wall_ms": row.wall_ms,
                    "ok": row.ok,
                    "beneficiary": ben,
                    "clock": "ExecPhase start, OCC workers, inspect_run record-only",
                    "path": engine.path,
                    "txs": txs,
                });
                writeln!(out, "{line}").unwrap();
                println!(
                    "ROW block={block_no} engine=occ kind=steptrace round={r} wall_ms={:.3} txs={} ok={} ge_1_5=false",
                    row.wall_ms,
                    txs.len(),
                    row.ok,
                );
            }
        }
        println!("wrote {out_path}");
        return;
    }

    if which == "inblock" {
        let block_no = env_usize("SPECFENCE_COMPARE_BLOCK", 15_274_915) as u64;
        let loaded = load_block(&data_dir, block_no, bytecodes, hashes);
        inblock_once(&loaded, workers);
        return;
    }

    if which == "seqcheck" {
        let block_no = env_usize("SPECFENCE_COMPARE_BLOCK", 15_274_915) as u64;
        let loaded = load_block(&data_dir, block_no, bytecodes, hashes);
        let n = env_usize("SPECFENCE_INFLATION_SEQCHECK_N", 10);
        seqcheck(&loaded, workers, n);
        return;
    }

    let blocks: Vec<u64> = if let Ok(list) = std::env::var("SPECFENCE_INFLATION_BLOCKS") {
        list.split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect()
    } else if which == "scan" {
        vec![15_274_915, 3_356_896]
    } else {
        vec![env_usize("SPECFENCE_COMPARE_BLOCK", 15_274_915) as u64]
    };
    let out_path = std::env::var("SPECFENCE_INFLATION_OUT")
        .unwrap_or_else(|_| format!("results/soft0-execute-inflation/{which}-w{workers}.jsonl"));
    if let Some(parent) = Path::new(&out_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut out = File::create(&out_path).expect("out");
    let meta = serde_json::json!({
        "meta": true,
        "host_cpus": host_cpus,
        "model": host_model(),
        "loadavg": loadavg(),
        "which": which,
        "workers": workers,
        "k": k,
        "oracle_k": oracle_k,
        "seed": seed,
        "warmup": 0,
        "blocks": blocks,
        "seq_cpu": seq_cpu,
        "pin_cpus": std::env::var("SPECFENCE_PIN_CPUS").unwrap_or_default(),
        "timed_region": "execute_revm_sequential | execute_revm_parallel",
        "paths": {"seq": PATH_SEQ, "occ": PATH_OCC, "sf": PATH_SF},
        "ge_1_5": false,
    });
    writeln!(out, "{meta}").unwrap();
    for block_no in blocks {
        let dir = data_dir.join("blocks").join(block_no.to_string());
        if !dir.join("block.json").exists() {
            println!("MISSING block={block_no}");
            continue;
        }
        let loaded = load_block(
            &data_dir,
            block_no,
            Arc::clone(&bytecodes),
            Arc::clone(&hashes),
        );
        println!(
            "BLOCK {block_no} n={} gas={} workers={workers}",
            loaded.n, loaded.gas_used
        );
        run_block(&loaded, workers, k, oracle_k, seq_cpu, seed, dump, &mut out);
    }
    println!("wrote {out_path}");
}
