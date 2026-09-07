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

    // --- SF vs OCC@8 cores ---
    println!("=== G7 SF vs OCC@8 cores ===");
    let cores_blocks = [14_689_597u64, 19_606_599u64, 19_469_097u64, 19_606_598u64];
    let mut sweep_rows = Vec::new();
    for bn in cores_blocks {
        let Some(loaded) = load_block(&data_dir, bn, Arc::clone(&bytecodes), Arc::clone(&block_hashes)) else {
            continue;
        };
        for mode in ["occ", "specfence"] {
            let mut pevm = match mode {
                "occ" => Pevm::with_concurrency_mode(ConcurrencyMode::Occ),
                _ => Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence),
            };
            pevm.reset_heat();
            let (ok, tps, wall_ms, soft, wh, aborts) = run_one(&chain, &mut pevm, &loaded, 8);
            let m = pevm.last_specfence_metrics();
            let n = n_tx(&loaded.block);
            let reexec_entries = m.evm_entries.saturating_sub(n);
            println!(
                "  block={bn} mode={mode:10} ok={ok} tps={tps:.0} wall_ms={wall_ms:.1} soft={soft} wait_hard={wh} abort_rate={:.3} lean={} evm={} reexec={} fb_reabort={} sw_ok={} sw_reabort={} park_k={}",
                if n == 0 { 0.0 } else { aborts as f64 / n as f64 },
                m.lean_mode_txs,
                m.evm_entries,
                reexec_entries,
                m.force_bind_reabort,
                m.soft_wait_wake_ok,
                m.soft_wait_wake_reabort,
                m.park_resume_at_k,
            );
            sweep_rows.push(serde_json::json!({
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
                // V5 dig hooks (interpreter-seconds / rewind / SoftWait / ForceBind)
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
                "absolute_jump_applied": m.absolute_jump_applied,
                "absolute_jump_fallback": m.absolute_jump_fallback,
                "tx_full_retry": m.tx_full_retry,
                "wait_park_count": m.wait_park_count,
                "wait_park_ns": m.wait_park_ns,
                "ready_steal_on_wait": m.ready_steal_on_wait,
            }));
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
        "v8_mean_reference": 0.325,
        "mean_sf_occ": mean,
        "ratios": ratios,
        "rows": sweep_rows,
    })).unwrap()).unwrap();
    println!("wrote {sweep_path:?}");
}
