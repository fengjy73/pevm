//! Sweep SpecFence@8 vs OCC@8 across every loadable ethereum block snapshot.
//!
//! ```
//! cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_all_blocks_sweep
//!
//! # Optional:
//! SPECFENCE_ALL_ITERS=1          # default 1; set 3 for slow outliers
//! SPECFENCE_ALL_PROCESS_TOP=10   # process-trace top-K worst SF/OCC (default 10; 0=off)
//! SPECFENCE_ALL_OUT=lab/results/all-blocks-sf-occ-sweep.json
//! SPECFENCE_ALL_BLOCKS=14689597,19606599   # subset override
//! ```

#![allow(missing_docs)]
#![recursion_limit = "256"]

use std::{
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
    BlockHashes, BuildSuffixHasher, Bytecodes, ConcurrencyMode, EvmAccount, InMemoryStorage, Pevm,
    chain::{PevmChain, PevmEthereum},
};
use walkdir::WalkDir;

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

fn discover_block_numbers(blocks_dir: &Path) -> Vec<u64> {
    let mut nums = Vec::new();
    for e in WalkDir::new(blocks_dir)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !e.file_type().is_dir() {
            continue;
        }
        if let Ok(n) = e.file_name().to_string_lossy().parse::<u64>() {
            nums.push(n);
        }
    }
    nums.sort_unstable();
    nums
}

fn try_load_block(
    data_dir: &Path,
    number: u64,
    bytecodes: Arc<Bytecodes>,
    block_hashes: Arc<BlockHashes>,
) -> Result<LoadedBlock, String> {
    let dir = data_dir.join("blocks").join(number.to_string());
    if !dir.is_dir() {
        return Err("no block dir".into());
    }
    let block_path = dir.join("block.json");
    let pre_path = dir.join("pre_state.json");
    if !block_path.exists() {
        return Err("missing block.json".into());
    }
    if !pre_path.exists() {
        return Err("missing pre_state.json".into());
    }
    let block: Block<<PevmEthereum as PevmChain>::Transaction> = serde_json::from_reader(
        BufReader::new(File::open(&block_path).map_err(|e| format!("open block.json: {e}"))?),
    )
    .map_err(|e| format!("parse block.json: {e}"))?;
    let accounts: HashMap<alloy_primitives::Address, EvmAccount, BuildSuffixHasher> =
        serde_json::from_reader(BufReader::new(
            File::open(&pre_path).map_err(|e| format!("open pre_state.json: {e}"))?,
        ))
        .map_err(|e| format!("parse pre_state.json: {e}"))?;
    let storage = InMemoryStorage::new(accounts, bytecodes, block_hashes);
    Ok(LoadedBlock {
        number,
        block,
        storage,
    })
}

fn n_tx(block: &Block<<PevmEthereum as PevmChain>::Transaction>) -> usize {
    match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs.len(),
        alloy_rpc_types_eth::BlockTransactions::Hashes(h) => h.len(),
        alloy_rpc_types_eth::BlockTransactions::Uncle => 0,
    }
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

fn metrics_json(m: &pevm::SpecFenceMetrics, n: usize) -> serde_json::Value {
    let reexec_entries = m.evm_entries.saturating_sub(n);
    serde_json::json!({
        "soft_wait_arms": m.soft_wait_arms,
        "wait_hard": m.wait_hard_count,
        "occ_aborts": m.occ_aborts,
        "edge_bind": m.edge_bind,
        "edge_wait_for": m.edge_wait_for,
        "edge_unfenced": m.edge_unfenced,
        "avoid_broadcasts": m.avoid_broadcasts,
        "canary_probes": m.canary_probes,
        "independent_unfenced": m.independent_unfenced,
        "force_prefix_unfenced": m.force_prefix_unfenced,
        "prefer_admit": m.prefer_admit,
        "multi_spine_admit": m.multi_spine_admit,
        "quiet_fence_revoke": m.quiet_fence_revoke,
        "bind_residual": m.bind_residual,
        "canary_reopen": m.canary_reopen,
        "writer_done_learned": m.writer_done_learned,
        "detect_accesses": m.detect_accesses,
        "predicted_essential_hits": m.predicted_essential_hits,
        "pcc_fire_at_a": m.pcc_fire_at_a,
        "force_prefix_as_pi": m.force_prefix_as_pi,
        "canary_live_verb": m.canary_live_verb,
        "inc_avoid_hits": m.inc_avoid_hits,
        "h_or_wait_door": m.h_or_wait_door,
        "morph_fence_actuator": m.morph_fence_actuator,
        "writer_validated_bind_gate": m.writer_validated_bind_gate,
        "flat_edgekey_sot": m.flat_edgekey_sot,
        "writer_identity_preserved": m.writer_identity_preserved,
        "rebind_only": m.rebind_only,
        "full_restart": m.full_restart,
        "rewind_to_cp": m.rewind_to_cp,
        "evm_entries": m.evm_entries,
        "reexec_entries": reexec_entries,
        "resume_count": m.resume_count,
        "bind_hits": m.bind_hits,
        "spec_read_count": m.spec_read_count,
        "lean_mode_txs": m.lean_mode_txs,
        "hotset_size": m.hotset_size,
        "ready_steal_on_wait": m.ready_steal_on_wait,
        "wait_park_count": m.wait_park_count,
        "wait_park_ns": m.wait_park_ns,
        "force_bind_reabort": m.force_bind_reabort,
        "fanout_fr_collapse": m.fanout_fr_collapse,
        "fanout_validate_defer": m.fanout_validate_defer,
        "fanout_absorb": m.fanout_absorb,
        "tx_full_retry": m.tx_full_retry,
        "partial_retry_count": m.partial_retry_count,
        "await_at_a_arms": m.await_at_a_arms,
        "engagement_switches": m.engagement_switches,
        "journal_ff_hits": m.journal_ff_hits,
        "cold_spec_fast": m.cold_spec_fast,
        "occ_fast_first": m.occ_fast_first,
        "profile_handler_ns": m.profile_handler_ns,
        "profile_maybe_wait_ns": m.profile_maybe_wait_ns,
        "profile_validate_ns": m.profile_validate_ns,
        "profile_scheduler_ns": m.profile_scheduler_ns,
    })
}

fn classify_morph(sf: &serde_json::Value) -> &'static str {
    let m = &sf["metrics"];
    let n = sf["n_tx"].as_u64().unwrap_or(1).max(1) as f64;
    let bind = m["edge_bind"].as_u64().unwrap_or(0) as f64;
    let wait = m["edge_wait_for"].as_u64().unwrap_or(0) as f64;
    let unf = m["edge_unfenced"].as_u64().unwrap_or(0) as f64;
    let rewind = m["rewind_to_cp"].as_u64().unwrap_or(0) as f64;
    let park = m["wait_park_count"].as_u64().unwrap_or(0) as f64;
    let hot = m["hotset_size"].as_u64().unwrap_or(0) as f64;
    let bind_per = bind / n;
    let wait_per = wait / n;
    let unf_per = unf / n;
    let rewind_per = rewind / n;
    if hot <= 5.0 && wait_per < 0.05 && bind_per < 0.3 && park < 5.0 && rewind_per < 0.05 {
        "quiet"
    } else if bind_per >= 1.0 || (rewind_per >= 0.05 && unf_per >= 1.5) {
        "fan_out"
    } else if wait_per >= 0.05 || park >= 20.0 || (hot >= 100.0 && wait_per >= 0.02) {
        "spine"
    } else if bind_per >= 0.4 || rewind_per >= 0.02 {
        "mixed"
    } else {
        "quiet_ish"
    }
}

fn dominant_metric(sf: &serde_json::Value) -> String {
    let m = &sf["metrics"];
    let wall = sf["wall_ms"].as_f64().unwrap_or(0.0);
    let park_ms = m["wait_park_ns"].as_u64().unwrap_or(0) as f64 / 1e6;
    let rewind = m["rewind_to_cp"].as_u64().unwrap_or(0);
    let rebind = m["rebind_only"].as_u64().unwrap_or(0);
    let full = m["full_restart"].as_u64().unwrap_or(0);
    let bind = m["edge_bind"].as_u64().unwrap_or(0);
    let wait = m["edge_wait_for"].as_u64().unwrap_or(0);
    let soft = m["soft_wait_arms"].as_u64().unwrap_or(0);
    let idle = if wall > 0.0 {
        park_ms / (8.0 * wall)
    } else {
        0.0
    };
    if soft > 0 {
        format!("soft_wait_arms={soft}")
    } else if idle >= 0.25 {
        format!("park_idle≈{idle:.2}")
    } else if rewind >= full.max(1) && rewind > rebind {
        format!("SuffixRepair_R2 rewind={rewind}")
    } else if full > rewind && full > 10 {
        format!("full_restart={full}")
    } else if bind > wait * 3 && bind > 100 {
        format!("edge_bind={bind}")
    } else if wait > 50 {
        format!("edge_wait_for={wait}")
    } else {
        format!(
            "meta/cold bind={bind} unf={}",
            m["edge_unfenced"].as_u64().unwrap_or(0)
        )
    }
}

fn run_mode(
    chain: &PevmEthereum,
    loaded: &LoadedBlock,
    mode: &str,
    cores: usize,
    iters: usize,
    process_trace: bool,
) -> serde_json::Value {
    let n = n_tx(&loaded.block);
    let cores_nz = NonZeroUsize::new(cores.max(1)).unwrap();
    let mut walls = Vec::with_capacity(iters);
    let mut tpss = Vec::with_capacity(iters);
    let mut last_metrics = None;
    let mut last_process = None;
    let mut ok_all = true;
    let mut last_err: Option<String> = None;

    for i in 0..iters {
        let mut pevm = match mode {
            "occ" => Pevm::with_concurrency_mode(ConcurrencyMode::Occ),
            _ => Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence),
        };
        pevm.reset_heat();
        pevm.reset_inter_prior();
        if process_trace && mode == "specfence" && i + 1 == iters {
            pevm.set_finegrain_trace(true);
        }
        let t0 = Instant::now();
        let result = pevm.execute(chain, &loaded.storage, &loaded.block, cores_nz, false);
        let elapsed = t0.elapsed().as_secs_f64();
        let wall_ms = elapsed * 1000.0;
        let tps = if elapsed > 0.0 {
            n as f64 / elapsed
        } else {
            0.0
        };
        match result {
            Ok(_) => {
                walls.push(wall_ms);
                tpss.push(tps);
                let m = pevm.last_specfence_metrics().clone();
                if mode == "specfence" && i + 1 == iters {
                    // Always keep process snapshot for decision-field contingencies;
                    // full process_summary still gated by process_trace / process_top.
                    last_process = Some(pevm.last_exec_process().clone());
                }
                last_metrics = Some(m);
            }
            Err(e) => {
                ok_all = false;
                last_err = Some(format!("{e:?}"));
                eprintln!("  ERROR block={} mode={mode}: {e:?}", loaded.number);
            }
        }
    }

    let (wall_med, wall_p90, wall_min, wall_mean) = if walls.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        summarize_f64(&mut walls)
    };
    let (tps_med, tps_p90, tps_min, tps_mean) = if tpss.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        // tps: report median of observed; p90 of tps is optimistic — keep for completeness
        summarize_f64(&mut tpss)
    };
    let metrics = last_metrics
        .as_ref()
        .map(|m| metrics_json(m, n))
        .unwrap_or_else(|| serde_json::json!({}));
    let mut row = serde_json::json!({
        "block": loaded.number,
        "mode": mode,
        "cores": cores,
        "iters": iters,
        "ok": ok_all && last_err.is_none(),
        "error": last_err,
        "n_tx": n,
        "gas_used": loaded.block.header.gas_used,
        "tps": tps_med,
        "tps_mean": tps_mean,
        "tps_p90": tps_p90,
        "tps_min": tps_min,
        "wall_ms": wall_med,
        "wall_ms_median": wall_med,
        "wall_ms_p90": wall_p90,
        "wall_ms_min": wall_min,
        "wall_ms_mean": wall_mean,
        "metrics": metrics,
    });
    if let Some(proc) = last_process {
        row["decision_fields"] = serde_json::to_value(&proc.decision_fields).unwrap_or_default();
        if process_trace {
            row["process_summary"] = serde_json::json!({
                "unfenced_total": proc.unfenced_total,
                "wait_for_total": proc.wait_for_total,
                "bind_total": proc.bind_total,
                "unfenced_after_avoid_total": proc.unfenced_after_avoid_total,
                "force_prefix_none_unfenced": proc.force_prefix_none_unfenced,
                "independent_unfenced_total": proc.independent_unfenced_total,
                "unfenced_after_fence_on_hot_l": proc.unfenced_after_fence_on_hot_l,
                "reason_histogram": proc.reason_histogram,
                "hot_fanout_l": proc.hot_fanout_l,
                "per_tx_len": proc.per_tx.len(),
            });
        }
    }
    row
}

fn write_checkpoint(path: &Path, doc: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(doc).unwrap()).unwrap();
    fs::rename(&tmp, path).unwrap();
}

fn main() {
    let data_dir = repo_root().join("data/ethereum");
    let blocks_dir = data_dir.join("blocks");
    let out_path = std::env::var("SPECFENCE_ALL_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("lab/results/all-blocks-sf-occ-sweep.json"));
    let iters = std::env::var("SPECFENCE_ALL_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let process_top = std::env::var("SPECFENCE_ALL_PROCESS_TOP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10usize);
    let block_override: Option<Vec<u64>> = std::env::var("SPECFENCE_ALL_BLOCKS")
        .ok()
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect());

    let numbers = block_override.unwrap_or_else(|| discover_block_numbers(&blocks_dir));
    eprintln!(
        "all-blocks sweep: {} candidates, iters={iters}, process_top={process_top}, out={}",
        numbers.len(),
        out_path.display()
    );

    let (bytecodes, block_hashes) = load_shared(&data_dir);
    let chain = PevmEthereum::mainnet();

    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut skipped: Vec<serde_json::Value> = Vec::new();
    let mut loaded_ok = 0usize;

    for (idx, bn) in numbers.iter().copied().enumerate() {
        eprintln!("[{}/{}] loading block {bn} …", idx + 1, numbers.len());
        match try_load_block(
            &data_dir,
            bn,
            Arc::clone(&bytecodes),
            Arc::clone(&block_hashes),
        ) {
            Ok(loaded) => {
                loaded_ok += 1;
                eprintln!(
                    "  loaded n_tx={} gas={}",
                    n_tx(&loaded.block),
                    loaded.block.header.gas_used
                );
                for mode in ["occ", "specfence"] {
                    let row = run_mode(&chain, &loaded, mode, 8, iters, false);
                    eprintln!(
                        "  {mode:10} ok={} tps={:.0} wall_ms={:.1} soft={} aborts={} bind={} wait={} unf={} rewind={} rebind={} full={}",
                        row["ok"],
                        row["tps"].as_f64().unwrap_or(0.0),
                        row["wall_ms"].as_f64().unwrap_or(0.0),
                        row["metrics"]["soft_wait_arms"].as_u64().unwrap_or(0),
                        row["metrics"]["occ_aborts"].as_u64().unwrap_or(0),
                        row["metrics"]["edge_bind"].as_u64().unwrap_or(0),
                        row["metrics"]["edge_wait_for"].as_u64().unwrap_or(0),
                        row["metrics"]["edge_unfenced"].as_u64().unwrap_or(0),
                        row["metrics"]["rewind_to_cp"].as_u64().unwrap_or(0),
                        row["metrics"]["rebind_only"].as_u64().unwrap_or(0),
                        row["metrics"]["full_restart"].as_u64().unwrap_or(0),
                    );
                    rows.push(row);
                }
            }
            Err(reason) => {
                eprintln!("  SKIP {bn}: {reason}");
                skipped.push(serde_json::json!({
                    "block": bn,
                    "reason": reason,
                }));
            }
        }

        // Checkpoint after each block so a hang does not lose prior rows.
        let doc = serde_json::json!({
            "test": "all ethereum blocks SF vs OCC@8",
            "head": option_env!("SPECFENCE_BUILD_HEAD").unwrap_or("f74f875"),
            "cores": 8,
            "iters": iters,
            "coverage": {
                "candidates": numbers.len(),
                "loaded": loaded_ok,
                "skipped": skipped.len(),
            },
            "skipped": skipped,
            "rows": rows,
            "block_pairs": [],
            "status": "running",
        });
        write_checkpoint(&out_path, &doc);
    }

    // Build per-block pair summary
    let mut pairs: Vec<serde_json::Value> = Vec::new();
    for bn in &numbers {
        let sf = rows.iter().find(|r| {
            r["block"].as_u64() == Some(*bn) && r["mode"] == "specfence" && r["ok"] == true
        });
        let occ = rows
            .iter()
            .find(|r| r["block"].as_u64() == Some(*bn) && r["mode"] == "occ" && r["ok"] == true);
        match (sf, occ) {
            (Some(sf), Some(occ)) => {
                let sf_tps = sf["tps"].as_f64().unwrap_or(0.0);
                let occ_tps = occ["tps"].as_f64().unwrap_or(0.0);
                let sf_wall = sf["wall_ms"].as_f64().unwrap_or(0.0);
                let occ_wall = occ["wall_ms"].as_f64().unwrap_or(0.0);
                let sf_occ = if occ_tps > 0.0 { sf_tps / occ_tps } else { 0.0 };
                let wall_ratio = if occ_wall > 0.0 {
                    sf_wall / occ_wall
                } else {
                    0.0
                };
                let morph = classify_morph(sf);
                let dominant = dominant_metric(sf);
                pairs.push(serde_json::json!({
                    "block": bn,
                    "n_tx": sf["n_tx"],
                    "gas_used": sf["gas_used"],
                    "sf_tps": sf_tps,
                    "occ_tps": occ_tps,
                    "sf_occ": sf_occ,
                    "sf_wall_ms": sf_wall,
                    "occ_wall_ms": occ_wall,
                    "wall_ratio": wall_ratio,
                    "morph_heuristic": morph,
                    "dominant_metric": dominant,
                    "soft_wait_arms": sf["metrics"]["soft_wait_arms"],
                    "edge_bind": sf["metrics"]["edge_bind"],
                    "edge_wait_for": sf["metrics"]["edge_wait_for"],
                    "edge_unfenced": sf["metrics"]["edge_unfenced"],
                    "avoid_broadcasts": sf["metrics"]["avoid_broadcasts"],
                    "rebind_only": sf["metrics"]["rebind_only"],
                    "full_restart": sf["metrics"]["full_restart"],
                    "rewind_to_cp": sf["metrics"]["rewind_to_cp"],
                    "occ_aborts_sf": sf["metrics"]["occ_aborts"],
                    "occ_aborts_occ": occ["metrics"]["occ_aborts"],
                    "hotset_size": sf["metrics"]["hotset_size"],
                    "wait_park_count": sf["metrics"]["wait_park_count"],
                }));
            }
            _ => {}
        }
    }
    pairs.sort_by(|a, b| {
        a["sf_occ"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&b["sf_occ"].as_f64().unwrap_or(0.0))
            .unwrap()
    });

    // Process-trace worst SF/OCC (lowest ratio)
    let mut process_blocks: Vec<u64> = pairs
        .iter()
        .take(process_top)
        .filter_map(|p| p["block"].as_u64())
        .collect();
    let mut process_rows = Vec::new();
    if process_top > 0 && !process_blocks.is_empty() {
        eprintln!(
            "=== process-trace top {} worst SF/OCC: {:?} ===",
            process_blocks.len(),
            process_blocks
        );
        for bn in process_blocks.drain(..) {
            match try_load_block(
                &data_dir,
                bn,
                Arc::clone(&bytecodes),
                Arc::clone(&block_hashes),
            ) {
                Ok(loaded) => {
                    let row = run_mode(&chain, &loaded, "specfence", 8, 1, true);
                    let pname = format!("all-blocks-process-{bn}.json");
                    let ppath = out_path.parent().unwrap().join(&pname);
                    fs::write(&ppath, serde_json::to_string_pretty(&row).unwrap()).ok();
                    eprintln!("wrote process {}", ppath.display());
                    process_rows.push(serde_json::json!({
                        "block": bn,
                        "path": pname,
                        "process_summary": row.get("process_summary").cloned().unwrap_or(serde_json::Value::Null),
                        "wall_ms": row["wall_ms"],
                        "metrics": row["metrics"],
                    }));
                }
                Err(e) => eprintln!("process-trace skip {bn}: {e}"),
            }
        }
    }

    // Aggregate stats
    let mut sf_occ_vals: Vec<f64> = pairs.iter().filter_map(|p| p["sf_occ"].as_f64()).collect();
    let mut wall_ratios: Vec<f64> = pairs
        .iter()
        .filter_map(|p| p["wall_ratio"].as_f64())
        .collect();
    let mut sf_tps: Vec<f64> = pairs.iter().filter_map(|p| p["sf_tps"].as_f64()).collect();
    let mut occ_tps: Vec<f64> = pairs.iter().filter_map(|p| p["occ_tps"].as_f64()).collect();
    let mut sf_walls: Vec<f64> = pairs
        .iter()
        .filter_map(|p| p["sf_wall_ms"].as_f64())
        .collect();
    let mut occ_walls: Vec<f64> = pairs
        .iter()
        .filter_map(|p| p["occ_wall_ms"].as_f64())
        .collect();

    let (sf_occ_med, sf_occ_p90, sf_occ_min, sf_occ_mean) = if sf_occ_vals.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        summarize_f64(&mut sf_occ_vals)
    };
    // For ratios where lower is worse: also report p10 as "worst side"
    let sf_occ_p10 = {
        let mut v: Vec<f64> = pairs.iter().filter_map(|p| p["sf_occ"].as_f64()).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        percentile_sorted(&v, 0.1)
    };
    let (wall_r_med, wall_r_p90, wall_r_min, wall_r_mean) = if wall_ratios.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        summarize_f64(&mut wall_ratios)
    };
    let (sf_tps_med, sf_tps_p90, _, sf_tps_mean) = if sf_tps.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        summarize_f64(&mut sf_tps)
    };
    let (occ_tps_med, occ_tps_p90, _, occ_tps_mean) = if occ_tps.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        summarize_f64(&mut occ_tps)
    };
    let (sf_wall_med, sf_wall_p90, _, _) = if sf_walls.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        summarize_f64(&mut sf_walls)
    };
    let (occ_wall_med, occ_wall_p90, _, _) = if occ_walls.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        summarize_f64(&mut occ_walls)
    };

    let mut morph_counts: HashMap<&'static str, usize> = HashMap::default();
    for p in &pairs {
        let m = p["morph_heuristic"].as_str().unwrap_or("?");
        *morph_counts
            .entry(match m {
                "quiet" => "quiet",
                "quiet_ish" => "quiet_ish",
                "fan_out" => "fan_out",
                "spine" => "spine",
                "mixed" => "mixed",
                _ => "other",
            })
            .or_default() += 1;
    }

    let soft_nonzero: Vec<u64> = pairs
        .iter()
        .filter(|p| p["soft_wait_arms"].as_u64().unwrap_or(0) > 0)
        .filter_map(|p| p["block"].as_u64())
        .collect();

    let worst10: Vec<_> = pairs.iter().take(10).cloned().collect();
    let best10: Vec<_> = pairs.iter().rev().take(10).cloned().collect();

    let summary = serde_json::json!({
        "n_pairs": pairs.len(),
        "sf_occ": {
            "median": sf_occ_med,
            "p10_worst": sf_occ_p10,
            "p90": sf_occ_p90,
            "min": sf_occ_min,
            "mean": sf_occ_mean,
        },
        "wall_ratio_sf_over_occ": {
            "median": wall_r_med,
            "p90": wall_r_p90,
            "min": wall_r_min,
            "mean": wall_r_mean,
        },
        "sf_tps": {"median": sf_tps_med, "p90": sf_tps_p90, "mean": sf_tps_mean},
        "occ_tps": {"median": occ_tps_med, "p90": occ_tps_p90, "mean": occ_tps_mean},
        "sf_wall_ms": {"median": sf_wall_med, "p90": sf_wall_p90},
        "occ_wall_ms": {"median": occ_wall_med, "p90": occ_wall_p90},
        "morph_counts": morph_counts,
        "soft_wait_arms_nonzero_blocks": soft_nonzero,
        "worst10_sf_occ": worst10,
        "best10_sf_occ": best10,
    });

    let doc = serde_json::json!({
        "test": "all ethereum blocks SF vs OCC@8",
        "head": option_env!("SPECFENCE_BUILD_HEAD").unwrap_or("f74f875"),
        "cores": 8,
        "iters": iters,
        "coverage": {
            "candidates": numbers.len(),
            "loaded": loaded_ok,
            "skipped": skipped.len(),
            "ok_pairs": pairs.len(),
        },
        "skipped": skipped,
        "summary": summary,
        "block_pairs": pairs,
        "process_top": process_rows,
        "rows": rows,
        "status": "complete",
    });
    write_checkpoint(&out_path, &doc);
    eprintln!(
        "DONE loaded={loaded_ok}/{} skipped={} pairs={} median_sf_occ={sf_occ_med:.3} mean={sf_occ_mean:.3} -> {}",
        numbers.len(),
        skipped.len(),
        pairs.len(),
        out_path.display()
    );
}
