//! G7 smoke: flip 19606598→19606599 + SF/OCC@8 cores.
//!
//! ```
//! cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_g7_smoke
//! ```

#![allow(missing_docs)]

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

struct LoadedBlock {
    number: u64,
    block: Block<<PevmEthereum as PevmChain>::Transaction>,
    storage: InMemoryStorage,
}

fn load_block(
    data_dir: &Path,
    number: u64,
    bytecodes: Arc<Bytecodes>,
    block_hashes: Arc<BlockHashes>,
) -> Option<LoadedBlock> {
    let dir = data_dir.join("blocks").join(number.to_string());
    if !dir.join("block.json").exists() {
        eprintln!("skip {number}: no snapshot");
        return None;
    }
    let block: Block<<PevmEthereum as PevmChain>::Transaction> = serde_json::from_reader(
        BufReader::new(File::open(dir.join("block.json")).expect("block.json")),
    )
    .unwrap_or_else(|e| panic!("parse block {number}: {e}"));
    let accounts: HashMap<alloy_primitives::Address, EvmAccount, BuildSuffixHasher> =
        serde_json::from_reader(BufReader::new(
            File::open(dir.join("pre_state.json")).expect("pre_state.json"),
        ))
        .unwrap_or_else(|e| panic!("parse pre_state {number}: {e}"));
    let storage = InMemoryStorage::new(accounts, bytecodes, block_hashes);
    Some(LoadedBlock {
        number,
        block,
        storage,
    })
}


fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let idx = ((n as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(n - 1)]
}

fn summarize_f64(vals: &mut [f64]) -> (f64, f64, f64, f64) {
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
    (
        percentile_sorted(vals, 0.5),
        percentile_sorted(vals, 0.9),
        vals[0],
        mean,
    )
}

fn n_tx(block: &Block<<PevmEthereum as PevmChain>::Transaction>) -> usize {
    match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs.len(),
        alloy_rpc_types_eth::BlockTransactions::Hashes(h) => h.len(),
        alloy_rpc_types_eth::BlockTransactions::Uncle => 0,
    }
}

fn run_one(
    chain: &PevmEthereum,
    pevm: &mut Pevm,
    loaded: &LoadedBlock,
    cores: usize,
) -> (bool, f64, f64, usize, usize, usize) {
    let cores_nz = NonZeroUsize::new(cores.max(1)).unwrap();
    let n = n_tx(&loaded.block);
    let t0 = Instant::now();
    let result = pevm.execute(chain, &loaded.storage, &loaded.block, cores_nz, false);
    let elapsed = t0.elapsed().as_secs_f64();
    let tps = if elapsed > 0.0 { n as f64 / elapsed } else { 0.0 };
    match result {
        Ok(_) => {
            let m = pevm.last_specfence_metrics();
            (
                true,
                tps,
                elapsed * 1000.0,
                m.soft_wait_arms,
                m.wait_hard_count,
                m.occ_aborts,
            )
        }
        Err(e) => {
            eprintln!("  ERROR: {e:?}");
            (false, 0.0, 0.0, 0, 0, 0)
        }
    }
}

fn main() {
    let data_dir = repo_root().join("data/ethereum");
    let (bytecodes, block_hashes) = load_shared(&data_dir);
    let chain = PevmEthereum::mainnet();
    let out_dir = repo_root().join("lab/results");
    std::fs::create_dir_all(&out_dir).ok();
    // Dig naming: SPECFENCE_G7_TAG=v5-bottleneck-lean → v5-bottleneck-lean-sf-occ.json
    let tag = std::env::var("SPECFENCE_G7_TAG").unwrap_or_default();
    let flip_name = if tag.is_empty() {
        "g7-flip-smoke.json".to_string()
    } else {
        format!("{tag}-flip.json")
    };
    let sweep_name = if tag.is_empty() {
        "g7-sf-occ-smoke.json".to_string()
    } else {
        format!("{tag}-sf-occ.json")
    };
    let softwait_disabled = std::env::var_os("SPECFENCE_DISABLE_SOFTWAIT").is_some_and(|v| {
        let s = v.to_string_lossy();
        s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
    });
    println!(
        "G7 dig tag={:?} softwait_disabled={softwait_disabled}",
        if tag.is_empty() { "default".into() } else { tag.clone() }
    );

    // --- Flip smoke: quiet 598 → mixed 599 on same Pevm (InterBlockPrior α flip) ---
    println!("=== G7 flip smoke 19606598 → 19606599 ===");
    let mut pevm = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    pevm.reset_heat();
    pevm.reset_inter_prior();
    let mut flip_rows = Vec::new();
    for bn in [19_606_598u64, 19_606_599u64] {
        let Some(loaded) = load_block(&data_dir, bn, Arc::clone(&bytecodes), Arc::clone(&block_hashes)) else {
            continue;
        };
        let (ok, tps, _ms, soft, wh, aborts) = run_one(&chain, &mut pevm, &loaded, 8);
        let flips = pevm.inter_prior_flip_count();
        println!(
            "  block={bn} ok={ok} tps={tps:.0} soft_wait_arms={soft} wait_hard={wh} aborts={aborts} flip_count={flips}"
        );
        flip_rows.push(serde_json::json!({
            "block": bn,
            "ok": ok,
            "tps": tps,
            "soft_wait_arms": soft,
            "wait_hard": wh,
            "occ_aborts": aborts,
            "inter_prior_flip_count": flips,
        }));
    }
    let flip_path = out_dir.join(&flip_name);
    std::fs::write(&flip_path, serde_json::to_string_pretty(&serde_json::json!({
        "test": "quiet→mixed flip",
        "blocks": [19606598, 19606599],
        "rows": flip_rows,
        "pass_criteria": "SoftWaits must not stick from quiet→mixed; flip α path exercised (flip_count may be ≥0 depending on morph KL)",
    })).unwrap()).unwrap();
    println!("wrote {flip_path:?}");

    // --- SF vs OCC@8 cores (SPECFENCE_G7_ITERS=N → median+p90 over N runs, default 1) ---
    let iters = std::env::var("SPECFENCE_G7_ITERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    println!("=== G7 SF vs OCC@8 cores iters={iters} ===");
    let cores_blocks = [14_689_597u64, 19_606_599u64, 19_469_097u64, 19_606_598u64];
    let mut sweep_rows = Vec::new();
    let mut multi_summaries = Vec::new();
    for bn in cores_blocks {
        let Some(loaded) = load_block(&data_dir, bn, Arc::clone(&bytecodes), Arc::clone(&block_hashes)) else {
            continue;
        };
        for mode in ["occ", "specfence"] {
            let mut walls = Vec::with_capacity(iters);
            let mut softs = Vec::with_capacity(iters);
            let mut aborts_v = Vec::with_capacity(iters);
            let mut last_row = None;
            for i in 0..iters {
                let mut pevm = match mode {
                    "occ" => Pevm::with_concurrency_mode(ConcurrencyMode::Occ),
                    _ => Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence),
                };
                pevm.reset_heat();
                pevm.reset_inter_prior();
                let (ok, tps, wall_ms, soft, wh, aborts) = run_one(&chain, &mut pevm, &loaded, 8);
                let m = pevm.last_specfence_metrics();
                let n = n_tx(&loaded.block);
                let reexec_entries = m.evm_entries.saturating_sub(n);
                walls.push(wall_ms);
                softs.push(soft as f64);
                aborts_v.push(aborts as f64);
                if iters == 1 || i + 1 == iters {
                    println!(
                        "  block={bn} mode={mode:10} iter={}/{} ok={ok} tps={tps:.0} wall_ms={wall_ms:.1} soft={soft} wait_hard={wh} abort_rate={:.3} lean={} evm={} reexec={} fb_reabort={} rebind={} cold={} occ_fast={} steal={} park_k={} mw_ms={:.1} h_ms={:.1} v_ms={:.1} park_ms={:.1} sw_ms={:.1} ea_ms={:.1} bo_ms={:.1} parks={}",
                        i + 1,
                        iters,
                        if n == 0 { 0.0 } else { aborts as f64 / n as f64 },
                        m.lean_mode_txs,
                        m.evm_entries,
                        reexec_entries,
                        m.force_bind_reabort,
                        m.rebind_only,
                        m.cold_spec_fast,
                        m.occ_fast_first,
                        m.ready_steal_on_wait,
                        m.park_resume_at_k,
                        m.profile_maybe_wait_ns as f64 / 1e6,
                        m.profile_handler_ns as f64 / 1e6,
                        m.profile_validate_ns as f64 / 1e6,
                        m.wait_park_ns as f64 / 1e6,
                        m.park_ns_softwait as f64 / 1e6,
                        m.park_ns_early_abort as f64 / 1e6,
                        m.park_ns_blocking_other as f64 / 1e6,
                        m.wait_park_count,
                    );
                }
                last_row = Some(serde_json::json!({
                    "block": bn,
                    "mode": mode,
                    "cores": 8,
                    "ok": ok,
                    "n_tx": n,
                    "tps": tps,
                    "wall_ms": wall_ms,
                    "soft_wait_arms": soft,
                    "wait_hard": wh,
                    "occ_aborts": aborts,
                    "lean_mode_txs": m.lean_mode_txs,
                    "hotset_size": m.hotset_size,
                    "bind_hits": m.bind_hits,
                    "spec_read_count": m.spec_read_count,
                    "evm_entries": m.evm_entries,
                    "reexec_entries": reexec_entries,
                    "rewind_to_cp": m.rewind_to_cp,
                    "resume_count": m.resume_count,
                    "journal_ff_entries": m.journal_ff_entries,
                    "journal_ff_hits": m.journal_ff_hits,
                    "park_resume_at_k": m.park_resume_at_k,
                    "park_resume_full_retry": m.park_resume_full_retry,
                    "full_restart": m.full_restart,
                    "partial_retry_count": m.partial_retry_count,
                    "force_bind_reabort": m.force_bind_reabort,
                    "soft_wait_wake_ok": m.soft_wait_wake_ok,
                    "soft_wait_wake_reabort": m.soft_wait_wake_reabort,
                    "rebind_only": m.rebind_only,
                    "cold_spec_fast": m.cold_spec_fast,
                    "occ_fast_first": m.occ_fast_first,
                    "profile_handler_ns": m.profile_handler_ns,
                    "profile_maybe_wait_ns": m.profile_maybe_wait_ns,
                    "profile_validate_ns": m.profile_validate_ns,
                    "profile_scheduler_ns": m.profile_scheduler_ns,
                    "absolute_jump_applied": m.absolute_jump_applied,
                    "absolute_jump_fallback": m.absolute_jump_fallback,
                    "tx_full_retry": m.tx_full_retry,
                    "wait_park_count": m.wait_park_count,
                    "wait_park_ns": m.wait_park_ns,
                    "ready_steal_on_wait": m.ready_steal_on_wait
                }));
                if let Some(row) = last_row.as_mut() {
                    row["park_count_softwait"] = serde_json::json!(m.park_count_softwait);
                    row["park_ns_softwait"] = serde_json::json!(m.park_ns_softwait);
                    row["park_count_early_abort"] = serde_json::json!(m.park_count_early_abort);
                    row["park_ns_early_abort"] = serde_json::json!(m.park_ns_early_abort);
                    row["park_count_blocking_other"] = serde_json::json!(m.park_count_blocking_other);
                    row["park_ns_blocking_other"] = serde_json::json!(m.park_ns_blocking_other);
                }
            }
            let (wall_med, wall_p90, wall_min, wall_mean) = summarize_f64(&mut walls);
            let (soft_med, _, _, _) = summarize_f64(&mut softs);
            let (abort_med, _, _, _) = summarize_f64(&mut aborts_v);
            if iters > 1 {
                println!(
                    "  block={bn} mode={mode:10} SUMMARY n={iters} wall_ms median={wall_med:.1} p90={wall_p90:.1} min={wall_min:.1} mean={wall_mean:.1} soft_med={soft_med:.0} abort_med={abort_med:.0}"
                );
            }
            if let Some(mut row) = last_row {
                row["wall_ms_median"] = serde_json::json!(wall_med);
                row["wall_ms_p90"] = serde_json::json!(wall_p90);
                row["wall_ms_min"] = serde_json::json!(wall_min);
                row["wall_ms_mean"] = serde_json::json!(wall_mean);
                row["iters"] = serde_json::json!(iters);
                row["soft_wait_arms_median"] = serde_json::json!(soft_med);
                row["occ_aborts_median"] = serde_json::json!(abort_med);
                if mode == "specfence" && bn == 14_689_597 {
                    multi_summaries.push(serde_json::json!({
                        "block": bn,
                        "mode": mode,
                        "iters": iters,
                        "wall_ms_median": wall_med,
                        "wall_ms_p90": wall_p90,
                        "wall_ms_min": wall_min,
                        "wall_ms_mean": wall_mean,
                        "soft_wait_arms_median": soft_med,
                        "occ_aborts_median": abort_med,
                    }));
                }
                sweep_rows.push(row);
            }
        }
    }
    // Ratios
    let mut ratios = Vec::new();
    for bn in cores_blocks {
        let occ = sweep_rows.iter().find(|r| r["block"] == bn && r["mode"] == "occ");
        let sf = sweep_rows.iter().find(|r| r["block"] == bn && r["mode"] == "specfence");
        if let (Some(o), Some(s)) = (occ, sf) {
            let ot = o["tps"].as_f64().unwrap_or(0.0);
            let st = s["tps"].as_f64().unwrap_or(0.0);
            let ratio = if ot > 0.0 { st / ot } else { 0.0 };
            ratios.push(serde_json::json!({"block": bn, "sf_occ": ratio, "sf_tps": st, "occ_tps": ot}));
            println!("  SF/OCC@8 block={bn}: {ratio:.3}");
        }
    }
    let mean = if ratios.is_empty() {
        0.0
    } else {
        ratios.iter().map(|r| r["sf_occ"].as_f64().unwrap()).sum::<f64>() / ratios.len() as f64
    };
    println!("  mean SF/OCC@8 = {mean:.3} (v8 baseline ~0.325 on different block set)");
    let sweep_path = out_dir.join(&sweep_name);
    std::fs::write(&sweep_path, serde_json::to_string_pretty(&serde_json::json!({
        "test": "SF vs OCC@8 architecture cores",
        "tag": if tag.is_empty() { serde_json::Value::Null } else { serde_json::json!(tag) },
        "softwait_disabled": softwait_disabled,
        "iters": iters,
        "v8_mean_reference": 0.325,
        "mean_sf_occ": mean,
        "ratios": ratios,
        "multi_run_597": multi_summaries,
        "rows": sweep_rows,
    })).unwrap()).unwrap();
    println!("wrote {sweep_path:?}");
}
