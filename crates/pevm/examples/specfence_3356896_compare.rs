//! SpecFence Parallel Spine (SF-PS) vs harness OCC
//! baseline on Ethereum mainnet block 3356896 (Soft=0). Compare is measurement
//! only — not a protocol fork.
//!
//! PRIMARY wall is the **learned** SpecFence state: one `Pevm` reused across
//! iters. Reports cold iter0 and reuse median (iters 1..N-1).
//! `SPECFENCE_COLD_EACH_ITER=1` restores new-Pevm-per-iter (cold-only).
//!
//! ```
//! SPECFENCE_COMPARE_ITERS=5 cargo run -p pevm --release \
//!   --config 'profile.release.lto=false' --example specfence_3356896_compare
//! ```

#![allow(missing_docs)]
#![recursion_limit = "256"]

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
use serde::Serialize;

const BLOCK: u64 = 3_356_896;
const DEFAULT_CORES: usize = 8;
const DEFAULT_ITERS: usize = 5;

/// Tip-DAG independents that PR15 taxed at begin_block (same-from lazy + dual empty-to).
const TAXED_INDEP: &[usize] = &[
    6, 7, 8, 9, 10, 11, 12, 13, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86,
];
const MAIN_CHAIN: &[usize] = &[
    4, 31, 66, 67, 69, 70, 93, 96, 103, 115, 131, 132, 135, 138, 141, 166, 171,
];
const STORAGE_141617: &[usize] = &[14, 16, 17];

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

fn n_tx(block: &Block<<PevmEthereum as PevmChain>::Transaction>) -> usize {
    match &block.transactions {
        alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs.len(),
        alloy_rpc_types_eth::BlockTransactions::Hashes(h) => h.len(),
        alloy_rpc_types_eth::BlockTransactions::Uncle => 0,
    }
}

fn median(mut vals: Vec<f64>) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    vals[vals.len() / 2]
}

#[derive(Serialize)]
struct IterRow {
    mode: String,
    i: usize,
    wall_ms: f64,
    occ_aborts: usize,
    refuse_admit: usize,
    wait_for_dependency: usize,
    wait_for_full_abort: usize,
    soft_wait_arms: usize,
    incarnation_gt0: usize,
    reexec_entries: usize,
    unfenced_reexec: usize,
    ready_width_mean: f64,
    idle_core_ns: u64,
    ordered_admit_cohorts: usize,
    optimistic_read_cohorts: usize,
    edge_ordered_admit: usize,
    edge_optimistic_read: usize,
    refuse_ns: u64,
    reexec_ns: u64,
    optimistic_majority_block: bool,
    cost_ev_keep_ordered: usize,
    cost_ev_demote_optimistic: usize,
    commute_skip: usize,
    batch_repair: usize,
    conflict_promote: usize,
    conflict_ignore: usize,
    prepaid_ns: u64,
    abort_cf_ns: u64,
    prior_decay: usize,
    admit_seed_begin_ns: u64,
    end_block_ns: u64,
    optimistic_path_tax_ns: u64,
    ordered_ns: u64,
    chosen_strategy: String,
    chosen_win_w: u8,
    chosen_w_cap: u8,
    chosen_seg_len: u8,
    unique_win_w: u8,
    unique_seg_len: u8,
    explore_budget: u8,
    ungated_occ_n: usize,
    pick_gate_n: usize,
    skip_gate_n: usize,
    ungated_occ_while_gated: usize,
    yield_ns: u64,
    gate_stall_ns: u64,
    worker_busy_ns: u64,
    win1_locs: usize,
    win2_locs: usize,
    win3_locs: usize,
    seg_locs: usize,
    full_locs: usize,
    defer_locs: usize,
    arm_switch_n: usize,
    explore_n: usize,
    win2_deviate_n: usize,
    bandit_c_opt: f64,
    bandit_c_win1: f64,
    bandit_c_win2: f64,
    bandit_c_win3: f64,
    bandit_c_defer: f64,
    selected_arms: String,
    detect_resolve_double_charge_n: usize,
    sys_reexec_n: usize,
    covering_n: usize,
    chosen_cover_window: u8,
    sf_schedule_picks: usize,
    occ_schedule_picks: usize,
    visibility_opt: usize,
    visibility_wait_released: usize,
    visibility_ordered_tip: usize,
    resolve_commit: usize,
    resolve_partial_rebind: usize,
    resolve_partial_rewind: usize,
    resolve_ordered_replay: usize,
    resolve_full_replay: usize,
    runnable_set_width_mean: f64,
    steal_n: usize,
    refuse_fill_n: usize,
    idle_core_frac: f64,
    resolve_apply_n: usize,
    mid_promote_n: usize,
    mid_promote_veto_n: usize,
    learn_e1_n: usize,
    learn_e2_n: usize,
    learn_e3_n: usize,
    learn_e4_n: usize,
    learn_e5_n: usize,
    learn_e6_n: usize,
    prior_plant_n: usize,
    began_from_prior: bool,
    begin_blocked: Vec<usize>,
    taxed_indep_blocked: Vec<usize>,
    main_inc_gt0: Vec<usize>,
    storage_inc_gt0: Vec<usize>,
    off_edge_inc_gt0: Vec<usize>,
    edge_4_31: bool,
    ordered_defer: usize,
    ordered_handoff: usize,
    retain_keeps: usize,
    tip_already: usize,
    chain_len: usize,
    estimate_block_sf: usize,
    spine_access: usize,
    spine_yield_ok: usize,
    spine_yield_deadlock: usize,
    spine_raw: usize,
    spine_waw: usize,
    spine_war: usize,
    spine_armed: usize,
    spine_pins: usize,
    spine_revoked: usize,
    spine_radar: usize,
}

#[derive(Serialize)]
struct CompareSummary {
    occ_median_ms: f64,
    sf_cold_ms: Option<f64>,
    sf_reuse_median_ms: Option<f64>,
    sf_all_median_ms: f64,
    primary_sf_ms: f64,
    primary_is_reuse: bool,
    sf_le_occ: bool,
    soft: u8,
    n_tx: usize,
    cores: usize,
    occ_tps: f64,
    sf_tps: f64,
    ratio: f64,
    ratio_ge_1_5: bool,
    estimate_block_sf: usize,
    occ_schedule_picks: usize,
    soft_wait_arms: usize,
    spine_access: usize,
    spine_yield_ok: usize,
    spine_yield_deadlock: usize,
    spine_raw: usize,
    spine_waw: usize,
    spine_war: usize,
    spine_armed: usize,
    spine_pins: usize,
    spine_revoked: usize,
    spine_radar: usize,
    ordered_defer: usize,
    ordered_handoff: usize,
    retain_keeps: usize,
    tip_already: usize,
    chain_len: usize,
}

fn inc_gt0_in(incs: &[usize], set: &[usize]) -> Vec<usize> {
    set.iter()
        .copied()
        .filter(|&t| incs.get(t).copied().unwrap_or(0) > 0)
        .collect()
}

fn writers_have_4_31(orders: &[(u64, Vec<usize>)]) -> bool {
    orders.iter().any(|(_, w)| {
        let i4 = w.iter().position(|&t| t == 4);
        let i31 = w.iter().position(|&t| t == 31);
        match (i4, i31) {
            (Some(a), Some(b)) => a < b,
            _ => false,
        }
    })
}

fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let nf = n as f64;
    let mx = xs.iter().sum::<f64>() / nf;
    let my = ys.iter().sum::<f64>() / nf;
    let mut num = 0.0;
    let mut dx = 0.0;
    let mut dy = 0.0;
    for i in 0..n {
        let a = xs[i] - mx;
        let b = ys[i] - my;
        num += a * b;
        dx += a * a;
        dy += b * b;
    }
    let den = (dx * dy).sqrt();
    if den == 0.0 { 0.0 } else { num / den }
}

fn median_ms(vals: &mut [u64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.sort_unstable();
    vals[vals.len() / 2] as f64 / 1e6
}

fn hist_compact(hist: &[usize; 32]) -> String {
    let mut s = String::new();
    for (k, &n) in hist.iter().enumerate() {
        if n == 0 || k == 0 {
            continue;
        }
        if !s.is_empty() {
            s.push(',');
        }
        if k == 31 {
            s.push_str(&format!(">={k}:{n}"));
        } else {
            s.push_str(&format!("{k}:{n}"));
        }
    }
    if s.is_empty() { "-".to_string() } else { s }
}

/// Longest writer list is the shared-location chain. Head is its smallest index.
fn print_focus(pevm: &Pevm, mode_name: &str, i: usize, n: usize, m: &pevm::SpecFenceMetrics) {
    let starts = pevm.last_tx_first_start();
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for (tx, &ns) in starts.iter().enumerate() {
        if ns > 0 {
            xs.push(tx as f64);
            ys.push(ns as f64);
        }
    }
    let corr = pearson(&xs, &ys);
    let q = (n / 10).max(1);
    let mut low = Vec::new();
    let mut high = Vec::new();
    for (tx, &ns) in starts.iter().enumerate() {
        if ns == 0 {
            continue;
        }
        if tx < q {
            low.push(ns);
        }
        if tx + q >= n {
            high.push(ns);
        }
    }
    let (loc, chain) = pevm
        .sticky_crit_chain()
        .filter(|(_, w)| w.len() >= 32)
        .or_else(|| {
            pevm.last_location_writers()
                .iter()
                .max_by_key(|(_, w)| w.len())
                .map(|(h, w)| (*h, w.clone()))
        })
        .unwrap_or((0, Vec::new()));
    let head = chain.iter().copied().min();
    let tail = chain.iter().copied().max();
    let start_ms = |tx: Option<usize>| {
        tx.and_then(|t| starts.get(t).copied())
            .filter(|ns| *ns > 0)
            .map(|ns| ns as f64 / 1e6)
            .unwrap_or(0.0)
    };
    println!(
        "  focus {mode_name}[{i}] full={} full_from_0={} prefix={} fail_k_n={} fail_k_min={} fail_k_max={} hist={} chain={loc:016x} chain_n={} head_tx={} head_ms={:.3} tail_tx={} tail_ms={:.3} span_ms={:.3} corr={corr:.3} low_q_ms={:.3} high_q_ms={:.3} explore={} began_prior={} detect_a={} avoid_b={} resolve_c={} early_tip={} est_block={} raw_ab={} raw_c={} war_ab={} war_c={} waw_ab={} waw_c={} chain_ab={} chain_c={} protect={} pbo={} replay_after={}",
        m.resolve_full_replay,
        m.full_from_zero,
        m.prefix_resume_n,
        m.fail_k_n,
        m.fail_k_min,
        m.fail_k_max,
        hist_compact(&m.fail_k_hist),
        chain.len(),
        head.unwrap_or(0),
        start_ms(head),
        tail.unwrap_or(0),
        start_ms(tail),
        (start_ms(tail) - start_ms(head)).max(0.0),
        median_ms(&mut low),
        median_ms(&mut high),
        m.explore_n,
        m.began_from_prior as u8,
        m.sf_detect_before_n,
        m.sf_avoid_publish_n,
        m.sf_resolve_after_fail_n,
        m.sf_early_tip_n,
        m.estimate_block_sf,
        m.sf_raw_avoid_n,
        m.sf_raw_late_n,
        m.sf_war_avoid_n,
        m.sf_war_late_n,
        m.sf_waw_avoid_n,
        m.sf_waw_late_n,
        m.sf_chain_avoid_n,
        m.sf_chain_late_n,
        m.sf_protect_n,
        m.sf_protect_before_opt_n,
        m.sf_replay_after_protect_n,
    );
}

fn run_once(
    pevm: &mut Pevm,
    mode_name: &str,
    i: usize,
    chain: &PevmEthereum,
    storage: &InMemoryStorage,
    block: &Block<<PevmEthereum as PevmChain>::Transaction>,
    cores_nz: NonZeroUsize,
    n: usize,
) -> IterRow {
    let t0 = Instant::now();
    let result = pevm.execute(chain, storage, block, cores_nz, false);
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    match result {
        Ok(_) => {
            let m = pevm.last_specfence_metrics();
            let incs = pevm.last_incarnations().to_vec();
            let begin = pevm.last_begin_blocked().to_vec();
            let taxed: Vec<usize> = begin
                .iter()
                .copied()
                .filter(|t| TAXED_INDEP.contains(t))
                .collect();
            let edge_4_31 = writers_have_4_31(pevm.last_location_writers());
            let learn = pevm.last_learn_report().clone();
            let off_edge: Vec<usize> = incs
                .iter()
                .enumerate()
                .filter(|&(t, inc)| {
                    *inc > 0
                        && !MAIN_CHAIN.contains(&t)
                        && !STORAGE_141617.contains(&t)
                        && ![20, 109, 149].contains(&t)
                })
                .map(|(t, _)| t)
                .collect();
            println!(
                "  {mode_name}[{i}] ok wall_ms={wall_ms:.3} tps={:.0} occ_aborts={} inc>0={} reexec={} refuse_admit={} wait_for_dependency={} soft_wait_arms={} idle_ns={} ready_width={:.2} ordered_admit={} optimistic_read={} edge_oa={} edge_or={} refuse_ns={} reexec_ns={} ordered_ns={} prepaid_ns={} abort_cf_ns={} prior_decay={} opt_maj={} ev_keep={} ev_demote={} commute={} batch={} d1_prom={} d1_ign={} taxed_begin={} edge_4_31={} unfenced_reexec={} double_charge={} sys_reexec={} covering={} cover_window={} learn={} win_w={} w_cap={} seg_len={} uniq_w/s={}/{} expl_bud={} win1/2/3/seg/full/defer={}/{}/{}/{}/{}/{} arms={} switch={} explore={} win2_dev={} c_opt/w1/w2/w3/def={:.0}/{:.0}/{:.0}/{:.0}/{:.0} main_inc={:?} storage_inc={:?} admit_seed_ns={} end_block_ns={} opt_path_tax_ns={} ungated_occ={} pick_gate={} skip_gate={} ungated_occ_while_gated={} yield_ns={} gate_stall_ns={} busy_ns={} sf_picks={} occ_picks={} vis_opt/wait/tip={}/{}/{} resolve_c/rebind/rewind/ord/full={}/{}/{}/{}/{} rset_w={:.2}",
                n as f64 / (wall_ms / 1000.0),
                m.occ_aborts,
                m.incarnation_gt0,
                m.reexec_entries,
                m.refuse_admit,
                m.wait_for_dependency,
                m.soft_wait_arms,
                m.idle_core_ns,
                m.ready_width_mean,
                m.ordered_admit_cohorts,
                m.optimistic_read_cohorts,
                m.edge_ordered_admit,
                m.edge_optimistic_read,
                m.refuse_ns,
                m.reexec_ns,
                learn.ordered_ns,
                learn.prepaid_ns,
                learn.abort_cf_ns,
                learn.prior_decay,
                m.optimistic_majority_block,
                m.cost_ev_keep_ordered,
                m.cost_ev_demote_optimistic,
                m.commute_skip,
                m.batch_repair,
                m.conflict_promote,
                m.conflict_ignore,
                taxed.len(),
                edge_4_31,
                m.unfenced_reexec,
                learn.detect_resolve_double_charge_n,
                learn.sys_reexec_n,
                learn.covering_n,
                learn.chosen_cover_window,
                learn.chosen_strategy,
                learn.chosen_win_w,
                learn.chosen_w_cap,
                learn.chosen_seg_len,
                learn.unique_win_w,
                learn.unique_seg_len,
                learn.explore_budget,
                learn.win1_locs,
                learn.win2_locs,
                learn.win3_locs,
                learn.seg_locs,
                learn.full_locs,
                learn.defer_locs,
                learn.selected_arms,
                learn.arm_switch_n,
                learn.explore_n,
                learn.win2_deviate_n,
                learn.bandit_c_opt,
                learn.bandit_c_win1,
                learn.bandit_c_win2,
                learn.bandit_c_win3,
                learn.bandit_c_defer,
                inc_gt0_in(&incs, MAIN_CHAIN),
                inc_gt0_in(&incs, STORAGE_141617),
                m.admit_seed_begin_ns,
                learn.end_block_ns,
                learn.optimistic_path_tax_ns,
                learn.ungated_occ_n,
                learn.pick_gate_n,
                learn.skip_gate_n,
                learn.ungated_occ_while_gated,
                learn.yield_ns,
                learn.gate_stall_ns,
                learn.worker_busy_ns,
                m.sf_schedule_picks,
                m.occ_schedule_picks,
                m.visibility_opt,
                m.visibility_wait_released,
                m.visibility_ordered_tip,
                m.resolve_commit,
                m.resolve_partial_rebind,
                m.resolve_partial_rewind,
                m.resolve_ordered_replay,
                m.resolve_full_replay,
                m.runnable_set_width_mean
            );
            println!(
                "  v3 {mode_name}[{i}] wait_once={} wait_suppressed={} never_wait={} prefix_resume={} journal_ff_hits={} resolve_rewind={}",
                m.access_wait_once,
                m.access_wait_suppressed,
                m.access_never_wait,
                m.prefix_resume_n,
                m.journal_ff_hits,
                m.resolve_partial_rewind
            );
            let spine = pevm.last_spine();
            if mode_name == "specfence" {
                print_focus(pevm, mode_name, i, n, &m);
                println!(
                    "  spine {mode_name}[{i}] access={} yield_ok={} yield_deadlock={} raw={} waw={} war={} armed={} pins={} revoked={} radar={} defer={} handoff={} retain={} tip_already={} chain={} est_block={} soft={} occ_picks={} spine_cores_max={} spine_cores_end={} handoff_claims={} claim_denied={} exact_wakes={} idle_parks={} help={} steal={} idle_spins={} local_pops={} steal_top_ns={} end_block_ns={} wake_miss={} admit_seed_ns={} join_wait_ns={} join_mark_origin_ns={} idle_ns={} heal_ns={} post_exec_validate_ns={} post_exec_in_span_ns={} span_head={} span_tail={} span_end_ns={} last_exec_ns={} last_val_ns={} quiet_true_ns={} last_exit_ns={} ps_exec_ns={} ps_val_ns={} ps_heal_ns={} ps_yield_ns={} ps_park_ns={} ps_steal_ns={} ps_exec_n={} ps_idle_n={} se_unstarted={} se_unfinished={} se_owed={} se_running={} se_pending={} se_indep={} se_bits={} qf_or={} first_cut={}",
                    spine.access_events,
                    spine.yield_waits_ok,
                    spine.yield_deadlocks,
                    spine.raw_n,
                    spine.waw_n,
                    spine.war_n,
                    spine.armed_locs,
                    spine.retained_pins,
                    spine.prior_revoked,
                    spine.prior_radar_only,
                    spine.ordered_defer,
                    spine.ordered_handoff,
                    spine.retain_keeps,
                    spine.tip_already,
                    spine.chain_len,
                    m.estimate_block_sf,
                    m.soft_wait_arms,
                    m.occ_schedule_picks,
                    spine.spine_cores_max,
                    spine.spine_cores_end,
                    spine.handoff_claims,
                    spine.claim_denied,
                    spine.exact_wakes,
                    spine.idle_parks,
                    spine.help_releases,
                    spine.steal_n,
                    spine.idle_spins,
                    spine.seed_owner_local_pops,
                    spine.steal_top_ns,
                    spine.end_block_ns,
                    spine.exact_wake_missed_nopark,
                    spine.admit_seed_ns,
                    spine.join_wait_ns,
                    spine.join_mark_origin_ns,
                    spine.idle_ns,
                    spine.heal_ns,
                    spine.post_exec_validate_ns,
                    spine.post_exec_in_span_ns,
                    spine.span_head,
                    spine.span_tail,
                    spine.span_end_origin_ns,
                    spine.last_exec_origin_ns,
                    spine.last_validate_origin_ns,
                    spine.quiet_true_origin_ns,
                    spine.last_exit_origin_ns,
                    spine.post_span_exec_ns,
                    spine.post_span_validate_ns,
                    spine.post_span_heal_ns,
                    spine.post_span_yield_ns,
                    spine.post_span_park_ns,
                    spine.post_span_steal_ns,
                    spine.post_span_exec_n,
                    spine.post_span_idle_n,
                    spine.span_end_not_started,
                    spine.span_end_unfinished,
                    spine.span_end_owed,
                    spine.span_end_running,
                    spine.span_end_pending,
                    spine.span_end_indep,
                    spine.span_end_false_bits,
                    spine.quiet_false_or,
                    spine.indep_first_cuts,
                );
                println!(
                    "  cutprobe {mode_name}[{i}] cut_exec_ns={} cut_exec_n={} cut_detect_ns={} cut_skip_ns={} cut_mv_ns={} cut_code_ns={} cut_finish_ns={} cut_keep_n={} cut_skip_n={} other_exec_ns={} other_exec_n={}",
                    spine.cut_exec_ns,
                    spine.cut_exec_n,
                    spine.cut_detect_ns,
                    spine.cut_skip_ns,
                    spine.cut_mv_ns,
                    spine.cut_code_ns,
                    spine.cut_finish_ns,
                    spine.cut_keep_n,
                    spine.cut_skip_n,
                    spine.other_exec_ns,
                    spine.other_exec_n,
                );
            }
            IterRow {
                mode: mode_name.to_string(),
                i,
                wall_ms,
                occ_aborts: m.occ_aborts,
                refuse_admit: m.refuse_admit,
                wait_for_dependency: m.wait_for_dependency,
                wait_for_full_abort: m.wait_for_full_abort,
                soft_wait_arms: m.soft_wait_arms,
                incarnation_gt0: m.incarnation_gt0,
                reexec_entries: m.reexec_entries,
                unfenced_reexec: m.unfenced_reexec,
                ready_width_mean: m.ready_width_mean,
                idle_core_ns: m.idle_core_ns,
                ordered_admit_cohorts: m.ordered_admit_cohorts,
                optimistic_read_cohorts: m.optimistic_read_cohorts,
                edge_ordered_admit: m.edge_ordered_admit,
                edge_optimistic_read: m.edge_optimistic_read,
                refuse_ns: m.refuse_ns,
                reexec_ns: m.reexec_ns,
                optimistic_majority_block: m.optimistic_majority_block,
                cost_ev_keep_ordered: m.cost_ev_keep_ordered,
                cost_ev_demote_optimistic: m.cost_ev_demote_optimistic,
                commute_skip: m.commute_skip,
                batch_repair: m.batch_repair,
                conflict_promote: m.conflict_promote,
                conflict_ignore: m.conflict_ignore,
                prepaid_ns: learn.prepaid_ns,
                abort_cf_ns: learn.abort_cf_ns,
                prior_decay: learn.prior_decay,
                admit_seed_begin_ns: m.admit_seed_begin_ns,
                end_block_ns: learn.end_block_ns,
                optimistic_path_tax_ns: learn.optimistic_path_tax_ns,
                ordered_ns: learn.ordered_ns,
                chosen_strategy: learn.chosen_strategy,
                chosen_win_w: learn.chosen_win_w,
                chosen_w_cap: learn.chosen_w_cap,
                chosen_seg_len: learn.chosen_seg_len,
                unique_win_w: learn.unique_win_w,
                unique_seg_len: learn.unique_seg_len,
                explore_budget: learn.explore_budget,
                ungated_occ_n: learn.ungated_occ_n,
                pick_gate_n: learn.pick_gate_n,
                skip_gate_n: learn.skip_gate_n,
                ungated_occ_while_gated: learn.ungated_occ_while_gated,
                yield_ns: learn.yield_ns,
                gate_stall_ns: learn.gate_stall_ns,
                worker_busy_ns: learn.worker_busy_ns,
                win1_locs: learn.win1_locs,
                win2_locs: learn.win2_locs,
                win3_locs: learn.win3_locs,
                seg_locs: learn.seg_locs,
                full_locs: learn.full_locs,
                defer_locs: learn.defer_locs,
                arm_switch_n: learn.arm_switch_n,
                explore_n: learn.explore_n,
                win2_deviate_n: learn.win2_deviate_n,
                bandit_c_opt: learn.bandit_c_opt,
                bandit_c_win1: learn.bandit_c_win1,
                bandit_c_win2: learn.bandit_c_win2,
                bandit_c_win3: learn.bandit_c_win3,
                bandit_c_defer: learn.bandit_c_defer,
                selected_arms: learn.selected_arms,
                detect_resolve_double_charge_n: learn.detect_resolve_double_charge_n,
                sys_reexec_n: learn.sys_reexec_n,
                covering_n: learn.covering_n,
                chosen_cover_window: learn.chosen_cover_window,
                sf_schedule_picks: m.sf_schedule_picks,
                occ_schedule_picks: m.occ_schedule_picks,
                visibility_opt: m.visibility_opt,
                visibility_wait_released: m.visibility_wait_released,
                visibility_ordered_tip: m.visibility_ordered_tip,
                resolve_commit: m.resolve_commit,
                resolve_partial_rebind: m.resolve_partial_rebind,
                resolve_partial_rewind: m.resolve_partial_rewind,
                resolve_ordered_replay: m.resolve_ordered_replay,
                resolve_full_replay: m.resolve_full_replay,
                runnable_set_width_mean: m.runnable_set_width_mean,
                steal_n: m.steal_n,
                refuse_fill_n: m.refuse_fill_n,
                idle_core_frac: m.idle_core_frac,
                resolve_apply_n: m.resolve_apply_n,
                mid_promote_n: m.mid_promote_n,
                mid_promote_veto_n: m.mid_promote_veto_n,
                learn_e1_n: m.learn_e1_n,
                learn_e2_n: m.learn_e2_n,
                learn_e3_n: m.learn_e3_n,
                learn_e4_n: m.learn_e4_n,
                learn_e5_n: m.learn_e5_n,
                learn_e6_n: m.learn_e6_n,
                prior_plant_n: m.prior_plant_n,
                began_from_prior: m.began_from_prior,
                begin_blocked: begin,
                taxed_indep_blocked: taxed,
                main_inc_gt0: inc_gt0_in(&incs, MAIN_CHAIN),
                storage_inc_gt0: inc_gt0_in(&incs, STORAGE_141617),
                off_edge_inc_gt0: off_edge,
                edge_4_31,
                ordered_defer: spine.ordered_defer,
                ordered_handoff: spine.ordered_handoff,
                retain_keeps: spine.retain_keeps,
                tip_already: spine.tip_already,
                chain_len: spine.chain_len,
                estimate_block_sf: m.estimate_block_sf,
                spine_access: spine.access_events,
                spine_yield_ok: spine.yield_waits_ok,
                spine_yield_deadlock: spine.yield_deadlocks,
                spine_raw: spine.raw_n,
                spine_waw: spine.waw_n,
                spine_war: spine.war_n,
                spine_armed: spine.armed_locs,
                spine_pins: spine.retained_pins,
                spine_revoked: spine.prior_revoked,
                spine_radar: spine.prior_radar_only,
            }
        }
        Err(e) => {
            eprintln!("  {mode_name}[{i}] ERROR {e:?}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let data_dir = repo_root().join("data/ethereum");
    let block_no = std::env::var("SPECFENCE_COMPARE_BLOCK")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(BLOCK);
    let dir = data_dir.join("blocks").join(block_no.to_string());
    if !dir.join("block.json").exists() {
        eprintln!("missing {dir:?}/block.json — cannot compare");
        std::process::exit(2);
    }
    let (bytecodes, block_hashes) = load_shared(&data_dir);
    let block: Block<<PevmEthereum as PevmChain>::Transaction> = serde_json::from_reader(
        BufReader::new(File::open(dir.join("block.json")).expect("block.json")),
    )
    .expect("parse block");
    let accounts: HashMap<alloy_primitives::Address, EvmAccount, BuildSuffixHasher> =
        serde_json::from_reader(BufReader::new(
            File::open(dir.join("pre_state.json")).expect("pre_state.json"),
        ))
        .expect("parse pre_state");
    let storage = InMemoryStorage::new(accounts, bytecodes, block_hashes);
    let chain = PevmEthereum::mainnet();
    let cores = std::env::var("SPECFENCE_COMPARE_CORES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CORES);
    let iters = std::env::var("SPECFENCE_COMPARE_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_ITERS)
        .max(1);
    let cold_each = std::env::var("SPECFENCE_COLD_EACH_ITER").is_ok();
    let cores_nz = NonZeroUsize::new(cores.max(1)).unwrap();
    let n = n_tx(&block);
    println!(
        "block={block_no} n={n} cores={cores} iters={iters} Soft=0 reuse_sf={} (PRIMARY=learned)",
        !cold_each
    );

    let mut rows: Vec<IterRow> = Vec::with_capacity(iters * 2);
    let mut occ_walls = Vec::new();
    let mut sf_walls = Vec::new();
    let mut last_occ = None;
    let mut last_sf = None;

    let mut occ = Pevm::with_concurrency_mode(ConcurrencyMode::Occ);
    let mut sf = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
    sf.reset_heat();
    sf.reset_inter_prior();

    // Interleave OCC / SpecFence so CPU warmup does not gift one mode
    // a colder first-half (measurement only — not a protocol fork).
    for i in 0..iters {
        occ = Pevm::with_concurrency_mode(ConcurrencyMode::Occ);
        let occ_row = run_once(&mut occ, "occ", i, &chain, &storage, &block, cores_nz, n);
        occ_walls.push(occ_row.wall_ms);
        last_occ = Some(occ_row.occ_aborts);
        rows.push(occ_row);

        if cold_each {
            sf = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
            sf.reset_heat();
            sf.reset_inter_prior();
        }
        let sf_row = run_once(
            &mut sf,
            "specfence",
            i,
            &chain,
            &storage,
            &block,
            cores_nz,
            n,
        );
        sf_walls.push(sf_row.wall_ms);
        last_sf = Some((
            sf_row.refuse_admit,
            sf_row.incarnation_gt0,
            sf_row.reexec_entries,
            sf_row.wait_for_dependency,
            sf_row.occ_aborts,
            sf_row.soft_wait_arms,
            sf_row.idle_core_ns,
            sf_row.ready_width_mean,
            sf_row.edge_ordered_admit,
            sf_row.edge_optimistic_read,
            sf_row.optimistic_majority_block,
            sf_row.commute_skip,
            sf_row.batch_repair,
            sf_row.admit_seed_begin_ns,
            sf_row.unfenced_reexec,
            sf_row.ordered_admit_cohorts,
            sf_row.learn_e1_n,
            sf_row.learn_e2_n,
            sf_row.learn_e3_n,
            sf_row.learn_e4_n,
            sf_row.learn_e5_n,
            sf_row.learn_e6_n,
            sf_row.mid_promote_n,
            sf_row.mid_promote_veto_n,
            sf_row.prior_plant_n,
            sf_row.began_from_prior,
            sf_row.steal_n,
            sf_row.refuse_fill_n,
            sf_row.resolve_apply_n,
            sf_row.explore_n,
        ));
        rows.push(sf_row);
    }

    let occ_med = median(occ_walls.clone());
    let sf_all_med = median(sf_walls.clone());
    let sf_cold = sf_walls.first().copied();
    let sf_reuse_med = if sf_walls.len() >= 2 {
        Some(median(sf_walls[1..].to_vec()))
    } else {
        None
    };
    let primary_is_reuse = !cold_each && sf_reuse_med.is_some();
    let primary_sf = if primary_is_reuse {
        sf_reuse_med.unwrap()
    } else {
        sf_all_med
    };
    let sf_le_occ = primary_sf <= occ_med + 1e-9;

    if let Some(aborts) = last_occ {
        println!("  occ median_wall_ms={occ_med:.3} last occ_aborts={aborts}");
    }
    if let Some(m) = last_sf {
        println!(
            "  specfence all_median_ms={sf_all_med:.3} cold_ms={:.3} reuse_median_ms={} last refuse_admit={} inc>0={} reexec={} wait_for_dependency={} occ_aborts={} soft_wait_arms={} idle_ns={} ready_width={:.2} edge_oa={} edge_or={} opt_maj={} commute={} batch={} admit_seed_ns={} unfenced={} ordered_admit={}",
            sf_cold.unwrap_or(0.0),
            sf_reuse_med
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| "n/a".into()),
            m.0,
            m.1,
            m.2,
            m.3,
            m.4,
            m.5,
            m.6,
            m.7,
            m.8,
            m.9,
            m.10,
            m.11,
            m.12,
            m.13,
            m.14,
            m.15
        );
        println!(
            "  learn e1={} e2={} e3={} e4={} e5={} e6={} mid_promote={} mid_promote_veto={} prior_plant={} began_from_prior={} steal={} refuse_fill={} resolve_apply={} explore_n={}",
            m.16, m.17, m.18, m.19, m.20, m.21, m.22, m.23, m.24, m.25, m.26, m.27, m.28, m.29
        );
    }

    let occ_tps = if occ_med > 0.0 {
        n as f64 / (occ_med / 1000.0)
    } else {
        0.0
    };
    let sf_tps = if primary_sf > 0.0 {
        n as f64 / (primary_sf / 1000.0)
    } else {
        0.0
    };
    let ratio = if occ_tps > 0.0 { sf_tps / occ_tps } else { 0.0 };
    let last_sf_metrics = rows.iter().rev().find(|r| r.mode == "specfence");
    let est = last_sf_metrics.map(|r| r.estimate_block_sf).unwrap_or(0);
    let occ_picks = last_sf_metrics.map(|r| r.occ_schedule_picks).unwrap_or(0);
    let soft_arms = last_sf_metrics.map(|r| r.soft_wait_arms).unwrap_or(0);
    let spine_access = last_sf_metrics.map(|r| r.spine_access).unwrap_or(0);
    let spine_yield_ok = last_sf_metrics.map(|r| r.spine_yield_ok).unwrap_or(0);
    let spine_yield_deadlock = last_sf_metrics.map(|r| r.spine_yield_deadlock).unwrap_or(0);
    let spine_raw = last_sf_metrics.map(|r| r.spine_raw).unwrap_or(0);
    let spine_waw = last_sf_metrics.map(|r| r.spine_waw).unwrap_or(0);
    let spine_war = last_sf_metrics.map(|r| r.spine_war).unwrap_or(0);
    let spine_armed = last_sf_metrics.map(|r| r.spine_armed).unwrap_or(0);
    let spine_pins = last_sf_metrics.map(|r| r.spine_pins).unwrap_or(0);
    let spine_revoked = last_sf_metrics.map(|r| r.spine_revoked).unwrap_or(0);
    let spine_radar = last_sf_metrics.map(|r| r.spine_radar).unwrap_or(0);
    let ordered_defer = last_sf_metrics.map(|r| r.ordered_defer).unwrap_or(0);
    let ordered_handoff = last_sf_metrics.map(|r| r.ordered_handoff).unwrap_or(0);
    let retain_keeps = last_sf_metrics.map(|r| r.retain_keeps).unwrap_or(0);
    let tip_already = last_sf_metrics.map(|r| r.tip_already).unwrap_or(0);
    let chain_len = last_sf_metrics.map(|r| r.chain_len).unwrap_or(0);

    println!(
        "summary Soft=0 occ_median_ms={occ_med:.3} sf_cold_ms={} sf_reuse_median_ms={} primary_sf_ms={primary_sf:.3} primary={} sf_le_occ={sf_le_occ}",
        sf_cold
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".into()),
        sf_reuse_med
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".into()),
        if primary_is_reuse { "reuse" } else { "cold" }
    );
    println!(
        "TPS_SUMMARY block={block_no} n={n} cores={cores} iters={iters} occ_tps={occ_tps:.1} sf_tps={sf_tps:.1} ratio={ratio:.3} ge_1_5={} occ_ms={occ_med:.3} sf_ms={primary_sf:.3} est_block={est} soft={soft_arms} occ_picks={occ_picks} access={spine_access} yield_ok={spine_yield_ok} yield_deadlock={spine_yield_deadlock} raw={spine_raw} waw={spine_waw} war={spine_war} armed={spine_armed} pins={spine_pins} revoked={spine_revoked} radar={spine_radar} defer={ordered_defer} handoff={ordered_handoff} retain={retain_keeps} tip_already={tip_already} chain={chain_len} spine_cores_max={} spine_cores_end={} handoff_claims={} claim_denied={} exact_wakes={} idle_parks={} help={} steal={} idle_spins={} local_pops={} steal_top_ns={} end_block_ns={} wake_miss={} admit_seed_ns={} join_wait_ns={} join_mark_origin_ns={} idle_ns={} heal_ns={} post_exec_validate_ns={} post_exec_in_span_ns={} span_head={} span_tail={} first_cut={}",
        ratio >= 1.5,
        sf.last_spine().spine_cores_max,
        sf.last_spine().spine_cores_end,
        sf.last_spine().handoff_claims,
        sf.last_spine().claim_denied,
        sf.last_spine().exact_wakes,
        sf.last_spine().idle_parks,
        sf.last_spine().help_releases,
        sf.last_spine().steal_n,
        sf.last_spine().idle_spins,
        sf.last_spine().seed_owner_local_pops,
        sf.last_spine().steal_top_ns,
        sf.last_spine().end_block_ns,
        sf.last_spine().exact_wake_missed_nopark,
        sf.last_spine().admit_seed_ns,
        sf.last_spine().join_wait_ns,
        sf.last_spine().join_mark_origin_ns,
        sf.last_spine().idle_ns,
        sf.last_spine().heal_ns,
        sf.last_spine().post_exec_validate_ns,
        sf.last_spine().post_exec_in_span_ns,
        sf.last_spine().span_head,
        sf.last_spine().span_tail,
        sf.last_spine().indep_first_cuts,
    );

    let summary = CompareSummary {
        occ_median_ms: occ_med,
        sf_cold_ms: sf_cold,
        sf_reuse_median_ms: sf_reuse_med,
        sf_all_median_ms: sf_all_med,
        primary_sf_ms: primary_sf,
        primary_is_reuse,
        sf_le_occ,
        soft: 0,
        n_tx: n,
        cores,
        occ_tps,
        sf_tps,
        ratio,
        ratio_ge_1_5: ratio >= 1.5,
        estimate_block_sf: est,
        occ_schedule_picks: occ_picks,
        soft_wait_arms: soft_arms,
        spine_access,
        spine_yield_ok,
        spine_yield_deadlock,
        spine_raw,
        spine_waw,
        spine_war,
        spine_armed,
        spine_pins,
        spine_revoked,
        spine_radar,
        ordered_defer,
        ordered_handoff,
        retain_keeps,
        tip_already,
        chain_len,
    };

    if std::env::var("SPECFENCE_COMPARE_CHECK").is_ok() {
        let mut checker = Pevm::with_concurrency_mode(ConcurrencyMode::SpecFence);
        let par = checker
            .execute(&chain, &storage, &block, cores_nz, false)
            .expect("parallel");
        let seq = checker
            .execute(&chain, &storage, &block, cores_nz, true)
            .expect("sequential");
        if par != seq {
            eprintln!("seq!=par block={block_no} n={n}");
            std::process::exit(2);
        }
        println!("seq=par ok block={block_no}");
    }

    if let Ok(path) = std::env::var("SPECFENCE_COMPARE_JSON") {
        let f = File::create(&path).expect("compare json");
        serde_json::to_writer_pretty(
            f,
            &serde_json::json!({
                "block": block_no,
                "n_tx": n,
                "rows": rows,
                "summary": summary
            }),
        )
        .expect("write json");
        println!("wrote {path}");
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"rows": rows, "summary": summary}))
                .unwrap()
        );
    }
}
