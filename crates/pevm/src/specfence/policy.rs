//! Cost-aware admit policy — multi-way EV + OptimisticRead-majority + CC-D1.
//!
//! Soft=0 actions: **OptimisticRead** vs **OrderedAdmit** (refuse +
//! wave-admit pred). OptimisticRead is the OCC-effect path on this spine —
//! not a hand-off to a second OCC runtime. Beta posteriors are **features**,
//! not `decide()` authority.
//!
//! Per hot ℓ pick min ĉ among {OptimisticRead, Win_1/2/3, Seg, FullChain}.
//! FullChain is the default only for short storage chains (14→16→17).
//! Long Basic WAW: begin plants a learned window or segmented short-hops
//! (T1/T2); next-quantum flush plants idle tail hops only (T3). Never
//! mid-execute ReadyEdge on in-flight txs. Do not freeze OrderedAdmit=K
//! from the full-shell prior.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use rustc_hash::FxBuildHasher;

use crate::{MemoryLocationHash, TxIdx};

use super::collateral::ConflictClass;

/// Small-block serial estimate below this → thin SpecFence shell (PC-S1).
const META_FLOOR_NS: f64 = 400_000.0;
/// ~2µs/tx cold serial hat (21k transfer class).
const SERIAL_NS_PER_TX: f64 = 2_000.0;
/// Thin-shell OrderedAdmit spine **cap** (not “always exactly 3”).
/// Promote/ignore may change the set; ns-EV may keep fewer.
pub(crate) const THIN_ORDERED_K: usize = 3;
/// Short-chain hop count (storage 14→16→17). Long spines never FullChain.
pub(crate) const ORDER_WINDOW_K: usize = 2;
/// T1: WindowedOrdered w∈[1, WINDOWED_W_MAX]. Win_1 is the cheap head hop.
pub(crate) const WINDOWED_K: usize = 1;
/// T1 learnable sliding window (F7 escalates 1→2→3).
pub(crate) const WINDOWED_W_MAX: usize = 3;
/// T2: txs per segment; intra-segment FullChain, inter-segment OptimisticRead.
pub(crate) const SEG_TX: usize = 4;
/// Cap planted segments so Seg cannot rebuild the PR22 prepaid wall.
const SEG_CAP: usize = 2;
/// F7: leftover hops / reexecs that force Win_1→Win_2/3 or Seg.
const LEFTOVER_WIN3: u32 = 3;
const LEFTOVER_SEG: u32 = 8;
const THIN_N_MAX: usize = 256;
/// ns-EV hysteresis: keep OrderedAdmit only if ĉ_ord + δ < ĉ_opt.
const NS_DELTA: f64 = 2_000.0;
/// Full-shell cold-start EMA priors (ns). Large blocks may keep OrderedAdmit.
const PRIOR_C_ORDERED_NS: f64 = 8_000.0;
const PRIOR_C_OPT_NS: f64 = 25_000.0;
/// Thin-block cold priors: OCC abort is cheaper than refuse/wave prepaid.
const PRIOR_C_ORDERED_THIN_NS: f64 = 18_000.0;
const PRIOR_C_OPT_THIN_NS: f64 = 6_000.0;
/// F4: consecutive FullChain losses before demote; min samples before switch.
const PREPAID_LOSE_N: u32 = 2;
const MIN_SAMPLES: f64 = 2.0;
const EMA_ALPHA: f64 = 0.20;

/// Soft=0 action on a candidate edge / cohort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmitAction {
    /// OptimisticRead — execute now; conflict pays reexec.
    OptimisticRead,
    /// OrderedAdmit — refuse while pred unfinished; steal independent work.
    OrderedAdmit,
}

/// Per-ℓ OrderedAdmit strategy (T1 Win_w / T2 Seg / Full / Opt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocStrategy {
    OptimisticRead,
    Windowed1,
    Windowed2,
    Windowed3,
    Segmented,
    FullChain,
}

impl LocStrategy {
    const fn as_u8(self) -> u8 {
        match self {
            Self::OptimisticRead => 0,
            Self::Windowed1 => 1,
            Self::Windowed2 => 2,
            Self::Windowed3 => 3,
            Self::Segmented => 4,
            Self::FullChain => 5,
        }
    }

    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Windowed1,
            2 => Self::Windowed2,
            3 => Self::Windowed3,
            4 => Self::Segmented,
            5 => Self::FullChain,
            _ => Self::OptimisticRead,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::OptimisticRead => "Opt",
            Self::Windowed1 => "Win_1",
            Self::Windowed2 => "Win_2",
            Self::Windowed3 => "Win_3",
            Self::Segmented => "Seg",
            Self::FullChain => "Full",
        }
    }

    pub(crate) const fn window_w(self) -> usize {
        match self {
            Self::Windowed1 => 1,
            Self::Windowed2 => 2,
            Self::Windowed3 => 3,
            Self::Segmented => SEG_TX.saturating_sub(1),
            Self::FullChain | Self::OptimisticRead => 0,
        }
    }

    pub(crate) const fn is_ordered(self) -> bool {
        !matches!(self, Self::OptimisticRead)
    }
}

/// Envelope / location cohort used as the B2 context key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum CohortKind {
    SameFrom = 0,
    EmptyTo = 1,
    CallWaw = 2,
    RawFan = 3,
}

impl CohortKind {
    pub(crate) const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// End-of-block learning report (the four questions in the design note).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LearnReport {
    /// Cohorts that chose OrderedAdmit this block.
    pub ordered_admit_cohorts: usize,
    /// OptimisticRead cohorts (cost-EV / K-cap / empty-to EOA).
    pub optimistic_read_cohorts: usize,
    /// Mean estimated cost of OrderedAdmit decisions (ns EMA).
    pub mean_c_ordered: f64,
    /// Mean estimated cost of the OptimisticRead counterfactual (ns EMA).
    pub mean_c_optimistic_cf: f64,
    /// OptimisticRead executions that later paid reexec (incarnation>0).
    pub optimistic_reexec: usize,
    /// Incarnation>0 txs that were never dependency-admitted.
    pub unfenced_reexec: usize,
    /// Sampled ready-set width mean (PC-W4 |Ready| bag).
    pub ready_width_mean: f64,
    /// Idle-core nanoseconds accumulated on yield / empty refuse (PC-4).
    pub idle_core_ns: u64,
    /// OptimisticRead-majority / low-meta block (ReadyEdge only on ordered-admit).
    pub optimistic_majority_block: bool,
    /// Measured refuse-path nanoseconds.
    pub refuse_ns: u64,
    /// Measured incarnation>0 execute nanoseconds.
    pub reexec_ns: u64,
    /// F1: refuse + wait attributed to OrderedAdmit locations.
    pub ordered_ns: u64,
    /// Cost-EV evaluations that kept OrderedAdmit (ĉ_ord+δ < ĉ_opt).
    pub cost_ev_keep_ordered: usize,
    /// Cost-EV evaluations that demoted to OptimisticRead.
    pub cost_ev_demote_optimistic: usize,
    /// Thin-shell K-cap demotions (eligible OrderedAdmit beyond K).
    pub k_cap_demote: usize,
    /// CC-X1: 21k commute accepts (no abort).
    pub commute_skip: usize,
    /// CC-R3: off-edge aborts parked as a batch behind a writer.
    pub batch_repair: usize,
    /// CC-D1: effective non-lazy conflict → learn/promote.
    pub conflict_promote: usize,
    /// CC-D1: lazy-noise conflict → ignorable (not unfenced_reexec→OrderedAdmit).
    pub conflict_ignore: usize,
    /// L3: OrderedAdmit prepaid ns this block (refuse + width loss).
    pub prepaid_ns: u64,
    /// L3: counterfactual OCC abort ns (measured reexec).
    pub abort_cf_ns: u64,
    /// L3: prior decay steps applied because prepaid lost.
    pub prior_decay: usize,
    /// P2: end-block HotSet / prior / D1 / learn wall (one Instant).
    pub end_block_ns: u64,
    /// M4: OptimisticRead-path tax vs OCC (admit seed + end_block + refuse).
    pub optimistic_path_tax_ns: u64,
    /// F6/F7: begin-time strategy census (not occ_aborts).
    pub opt_locs: usize,
    /// Locations that chose WindowedOrdered w=1.
    pub win1_locs: usize,
    /// Locations that chose WindowedOrdered w=2.
    pub win2_locs: usize,
    /// Locations that chose WindowedOrdered w=3.
    pub win3_locs: usize,
    /// Locations that chose segmented short FullChain (T2).
    pub seg_locs: usize,
    /// Locations that chose FullChain (short storage only on thin blocks).
    pub full_locs: usize,
    /// Dominant learned action label (`Win_3` / `Seg` / `Full` / `Opt`).
    pub chosen_strategy: String,
    /// T1 window width of the dominant Win_w (0 if not windowed).
    pub chosen_win_w: u8,
    /// Dual-path pick: ungated OCC-class issues.
    pub pick_occ_n: usize,
    /// Dual-path pick: gated (OrderedAdmit) issues.
    pub pick_gate_n: usize,
    /// Gated-not-ready holes skipped (edge constraint, not global mode).
    pub skip_gate_n: usize,
    /// Ungated OCC picks while any gate was live (P1 evidence).
    pub occ_pick_while_gated: usize,
    /// Product-path scheduler yield ns (P1). Instant-tax; not in ĉ.
    pub yield_ns: u64,
    /// Wall-clock gate stall (S2 prepaid). Same source as refuse_ns.
    pub gate_stall_ns: u64,
    /// Worker execute+validate busy ns (sum). Instant-tax; not in ĉ.
    pub worker_busy_ns: u64,
}

#[derive(Debug, Clone, Copy)]
struct OnlineStat {
    n: f64,
    mean: f64,
}

impl OnlineStat {
    const fn new(prior: f64) -> Self {
        Self {
            n: 1.0,
            mean: prior,
        }
    }

    fn update(&mut self, x: f64) {
        self.n += 1.0;
        self.mean += (x - self.mean) / self.n;
    }

    fn ema(&mut self, x: f64, alpha: f64) {
        self.mean = (1.0 - alpha) * self.mean + alpha * x.clamp(0.0, 8.0);
        self.n += 1.0;
    }

    fn ema_ns(&mut self, x: f64, alpha: f64) {
        self.mean = (1.0 - alpha) * self.mean + alpha * x.clamp(0.0, 10_000_000.0);
        self.n += 1.0;
    }
}

/// CC-D1 first-conflict note for an off-edge incarnation>0 tx.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConflictNote {
    pub location: MemoryLocationHash,
    pub peer: Option<TxIdx>,
    pub class: ConflictClass,
    pub lazy: bool,
}

/// Cross-block short-edge promote (location, not a whole lazy spine).
#[derive(Debug, Clone, Copy)]
struct PromotedLoc {
    hits: u32,
    reexec_ns_ema: f64,
    /// True after an EffectiveWAW observation (abort hat if `reexec_ns` was 0).
    measured: bool,
    /// Immediate predecessor on this ℓ (reuse / mid-block seed).
    pred: TxIdx,
    /// Immediate successor on this ℓ (reuse / mid-block seed).
    succ: TxIdx,
    /// F4: FullChain lost — next begin does not FullChain this long spine.
    demoted: bool,
    /// F3: strategy chosen at last begin / plant decision.
    decision: LocStrategy,
    /// F4: EMA samples on this ℓ.
    samples: u32,
    /// F2: three-way ĉ (OptimisticRead / WindowedOrdered / FullChain).
    c_opt: f64,
    c_win: f64,
    c_full: f64,
    /// F4: consecutive FullChain losses.
    fullchain_lose: u32,
    /// F2: ĉ for Win_2 / Win_3 / Seg (Win_1 is `c_win`).
    c_win2: f64,
    c_win3: f64,
    c_seg: f64,
    /// F7: leftover reexec / unfenced hops after last decision (cross-block).
    leftover_reexec: u32,
    escalate_n: u32,
    /// F1 this-block attribution (reset at begin).
    block_reexec_ns: u64,
    block_ordered_ns: u64,
    block_reexec_n: u32,
    block_ordered_n: u32,
}

impl PromotedLoc {
    fn new() -> Self {
        Self {
            hits: 0,
            reexec_ns_ema: PRIOR_C_OPT_NS,
            measured: false,
            pred: usize::MAX,
            succ: usize::MAX,
            demoted: false,
            decision: LocStrategy::OptimisticRead,
            samples: 0,
            c_opt: PRIOR_C_OPT_THIN_NS,
            c_win: PRIOR_C_ORDERED_THIN_NS,
            c_full: PRIOR_C_ORDERED_THIN_NS,
            fullchain_lose: 0,
            c_win2: PRIOR_C_ORDERED_THIN_NS,
            c_win3: PRIOR_C_ORDERED_THIN_NS,
            c_seg: PRIOR_C_ORDERED_THIN_NS,
            leftover_reexec: 0,
            escalate_n: 0,
            block_reexec_ns: 0,
            block_ordered_ns: 0,
            block_reexec_n: 0,
            block_ordered_n: 0,
        }
    }
}

/// Per-address B2 context: p_effWAW + refuse-cost EMA + per-pair cost-EV.
#[derive(Debug, Clone, Copy)]
struct CohortStat {
    p_eff: OnlineStat,
    refuse: OnlineStat,
    c_opt: OnlineStat,
    c_ord: OnlineStat,
    prepaid_lose: u32,
}

/// Process-persistent cost calibrator (B2) + per-block ns-EV decide.
#[derive(Debug)]
pub(crate) struct CostPolicy {
    block_n: AtomicUsize,
    refuse_global: Mutex<OnlineStat>,
    reexec_global: Mutex<OnlineStat>,
    idle_global: Mutex<OnlineStat>,
    /// L1: EMA of measured refuse_ns + width_loss_ns.
    c_ord_ns: Mutex<OnlineStat>,
    /// L1: EMA of measured reexec_ns.
    c_opt_ns: Mutex<OnlineStat>,
    /// Linear weights for p_eff ≈ σ(w·x) — **feature only**, not decide().
    w_p: Mutex<[f64; 6]>,
    cohorts: DashMap<(u8, Address), CohortStat, FxBuildHasher>,
    ordered_decisions: AtomicUsize,
    optimistic_read_cohorts: AtomicUsize,
    c_ord_sum_bits: AtomicU64,
    c_opt_sum_bits: AtomicU64,
    ev_samples: AtomicUsize,
    optimistic_reexec: AtomicUsize,
    unfenced_reexec: AtomicUsize,
    optimistic_majority_block: AtomicBool,
    cost_ev_keep_ordered: AtomicUsize,
    cost_ev_demote_optimistic: AtomicUsize,
    k_cap_demote: AtomicUsize,
    refuse_ns: AtomicU64,
    reexec_ns: AtomicU64,
    width_loss_ns: AtomicU64,
    commute_skip: AtomicUsize,
    batch_repair: AtomicUsize,
    conflict_promote: AtomicUsize,
    conflict_ignore: AtomicUsize,
    /// CC-D1: first conflict ℓ per off-edge tx.
    conflicts: DashMap<TxIdx, ConflictNote, FxBuildHasher>,
    /// C4: EffectiveWAW locations eligible for a short-edge A1 (cap-limited).
    promoted: DashMap<MemoryLocationHash, PromotedLoc, FxBuildHasher>,
    /// L4: consecutive (pred, succ) pairs on a hot ℓ (beyond one stored pair).
    short_chain: DashMap<MemoryLocationHash, Vec<(TxIdx, TxIdx)>, FxBuildHasher>,
    /// L2: reexec_ns EMA keyed by location (feeds U3 / per-ℓ EV).
    loc_reexec_ns: DashMap<MemoryLocationHash, OnlineStat, FxBuildHasher>,
    /// L2: refuse/prepaid EMA keyed by location.
    loc_c_ord: DashMap<MemoryLocationHash, OnlineStat, FxBuildHasher>,
    /// Hot-path read-only EV snapshot (written at begin/end-block).
    c_opt_snap_bits: AtomicU64,
    c_ord_snap_bits: AtomicU64,
    /// L3: consecutive blocks where prepaid lost to abort_cf (process-persistent).
    prepaid_lose_streak: AtomicUsize,
    prior_decay: AtomicUsize,
    prepaid_ns: AtomicU64,
    abort_cf_ns: AtomicU64,
    /// C3: off-edge reexecs in this block (wave).
    wave_off_edge: AtomicUsize,
    wave_batch_noted: AtomicBool,
    /// L4: previous block size — reuse may seed stored (pred, succ) idx pairs.
    last_block_n: AtomicUsize,
    /// O3: (ℓ, pred, succ) recorded during abort — flush only when idle.
    pending_idle: Mutex<Vec<(MemoryLocationHash, TxIdx, TxIdx)>>,
}

impl Default for CostPolicy {
    fn default() -> Self {
        Self {
            block_n: AtomicUsize::new(0),
            refuse_global: Mutex::new(OnlineStat::new(0.22)),
            reexec_global: Mutex::new(OnlineStat::new(1.0)),
            idle_global: Mutex::new(OnlineStat::new(0.12)),
            c_ord_ns: Mutex::new(OnlineStat::new(PRIOR_C_ORDERED_NS)),
            c_opt_ns: Mutex::new(OnlineStat::new(PRIOR_C_OPT_NS)),
            // Cold-start: intercept 0.05, contract +0.9, logn mild, storage +1.1.
            w_p: Mutex::new([0.05, 0.90, 0.12, 1.10, -0.15, 0.80]),
            cohorts: DashMap::default(),
            ordered_decisions: AtomicUsize::new(0),
            optimistic_read_cohorts: AtomicUsize::new(0),
            c_ord_sum_bits: AtomicU64::new(0.0f64.to_bits()),
            c_opt_sum_bits: AtomicU64::new(0.0f64.to_bits()),
            ev_samples: AtomicUsize::new(0),
            optimistic_reexec: AtomicUsize::new(0),
            unfenced_reexec: AtomicUsize::new(0),
            optimistic_majority_block: AtomicBool::new(false),
            cost_ev_keep_ordered: AtomicUsize::new(0),
            cost_ev_demote_optimistic: AtomicUsize::new(0),
            k_cap_demote: AtomicUsize::new(0),
            refuse_ns: AtomicU64::new(0),
            reexec_ns: AtomicU64::new(0),
            width_loss_ns: AtomicU64::new(0),
            commute_skip: AtomicUsize::new(0),
            batch_repair: AtomicUsize::new(0),
            conflict_promote: AtomicUsize::new(0),
            conflict_ignore: AtomicUsize::new(0),
            conflicts: DashMap::default(),
            promoted: DashMap::default(),
            short_chain: DashMap::default(),
            loc_reexec_ns: DashMap::default(),
            loc_c_ord: DashMap::default(),
            c_opt_snap_bits: AtomicU64::new(PRIOR_C_OPT_NS.to_bits()),
            c_ord_snap_bits: AtomicU64::new(PRIOR_C_ORDERED_NS.to_bits()),
            prepaid_lose_streak: AtomicUsize::new(0),
            prior_decay: AtomicUsize::new(0),
            prepaid_ns: AtomicU64::new(0),
            abort_cf_ns: AtomicU64::new(0),
            wave_off_edge: AtomicUsize::new(0),
            wave_batch_noted: AtomicBool::new(false),
            last_block_n: AtomicUsize::new(0),
            pending_idle: Mutex::new(Vec::new()),
        }
    }
}

impl CostPolicy {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn reset(&self) {
        self.block_n.store(0, Ordering::Relaxed);
        *self.refuse_global.lock().unwrap() = OnlineStat::new(0.22);
        *self.reexec_global.lock().unwrap() = OnlineStat::new(1.0);
        *self.idle_global.lock().unwrap() = OnlineStat::new(0.12);
        *self.c_ord_ns.lock().unwrap() = OnlineStat::new(PRIOR_C_ORDERED_NS);
        *self.c_opt_ns.lock().unwrap() = OnlineStat::new(PRIOR_C_OPT_NS);
        *self.w_p.lock().unwrap() = [0.05, 0.90, 0.12, 1.10, -0.15, 0.80];
        self.cohorts.clear();
        self.conflicts.clear();
        self.promoted.clear();
        self.short_chain.clear();
        self.loc_reexec_ns.clear();
        self.loc_c_ord.clear();
        self.pending_idle.lock().unwrap().clear();
        self.prepaid_lose_streak.store(0, Ordering::Relaxed);
        self.last_block_n.store(0, Ordering::Relaxed);
        self.c_opt_snap_bits
            .store(PRIOR_C_OPT_NS.to_bits(), Ordering::Relaxed);
        self.c_ord_snap_bits
            .store(PRIOR_C_ORDERED_NS.to_bits(), Ordering::Relaxed);
        self.reset_block_counters();
    }

    fn reset_block_counters(&self) {
        self.ordered_decisions.store(0, Ordering::Relaxed);
        self.optimistic_read_cohorts.store(0, Ordering::Relaxed);
        self.c_ord_sum_bits
            .store(0.0f64.to_bits(), Ordering::Relaxed);
        self.c_opt_sum_bits
            .store(0.0f64.to_bits(), Ordering::Relaxed);
        self.ev_samples.store(0, Ordering::Relaxed);
        self.optimistic_reexec.store(0, Ordering::Relaxed);
        self.unfenced_reexec.store(0, Ordering::Relaxed);
        self.optimistic_majority_block
            .store(false, Ordering::Relaxed);
        self.cost_ev_keep_ordered.store(0, Ordering::Relaxed);
        self.cost_ev_demote_optimistic.store(0, Ordering::Relaxed);
        self.k_cap_demote.store(0, Ordering::Relaxed);
        self.refuse_ns.store(0, Ordering::Relaxed);
        self.reexec_ns.store(0, Ordering::Relaxed);
        self.width_loss_ns.store(0, Ordering::Relaxed);
        self.commute_skip.store(0, Ordering::Relaxed);
        self.batch_repair.store(0, Ordering::Relaxed);
        self.conflict_promote.store(0, Ordering::Relaxed);
        self.conflict_ignore.store(0, Ordering::Relaxed);
        self.conflicts.clear();
        self.prior_decay.store(0, Ordering::Relaxed);
        self.prepaid_ns.store(0, Ordering::Relaxed);
        self.abort_cf_ns.store(0, Ordering::Relaxed);
        self.wave_off_edge.store(0, Ordering::Relaxed);
        self.wave_batch_noted.store(false, Ordering::Relaxed);
        self.pending_idle.lock().unwrap().clear();
    }

    pub(crate) fn begin_block(&self, n: usize) {
        let prev = self.block_n.load(Ordering::Relaxed);
        if prev > 0 {
            self.last_block_n.store(prev, Ordering::Relaxed);
        }
        self.block_n.store(n, Ordering::Relaxed);
        self.reset_block_counters();
        let serial_hat = n as f64 * SERIAL_NS_PER_TX;
        let thin = n > 0 && n <= THIN_N_MAX && serial_hat < META_FLOOR_NS;
        self.optimistic_majority_block
            .store(thin, Ordering::Relaxed);
        // Hot-path snapshot: thin cold defaults A0 (abort cheaper than prepaid).
        let (c_opt, c_ord) = if thin {
            (PRIOR_C_OPT_THIN_NS, PRIOR_C_ORDERED_THIN_NS)
        } else {
            (
                self.c_opt_ns.lock().unwrap().mean.max(1.0),
                self.c_ord_ns.lock().unwrap().mean.max(1.0),
            )
        };
        self.c_opt_snap_bits
            .store(c_opt.to_bits(), Ordering::Relaxed);
        self.c_ord_snap_bits
            .store(c_ord.to_bits(), Ordering::Relaxed);
        for mut e in self.promoted.iter_mut() {
            e.block_reexec_ns = 0;
            e.block_ordered_ns = 0;
            e.block_reexec_n = 0;
            e.block_ordered_n = 0;
        }
    }

    #[inline]
    pub(crate) fn is_optimistic_majority_block(&self) -> bool {
        self.optimistic_majority_block.load(Ordering::Relaxed)
    }

    /// Thin / optimistic-majority: plant OrderedAdmit only when a measured pair
    /// or ℓ EV says prepaid is cheaper. Cold start stays OptimisticRead.
    pub(crate) fn should_seed_thin_ordered(&self) -> bool {
        if !self.is_optimistic_majority_block() {
            return true;
        }
        for e in self.promoted.iter() {
            if self.is_promoted(*e.key()) {
                return true;
            }
        }
        for e in self.cohorts.iter() {
            let s = e.value();
            if (s.c_opt.n >= 2.0 || s.c_ord.n >= 2.0)
                && s.c_ord.mean + NS_DELTA < s.c_opt.mean.max(1.0)
            {
                return true;
            }
        }
        false
    }

    #[inline]
    pub(crate) fn thin_ordered_k(&self) -> usize {
        if self.is_optimistic_majority_block() {
            THIN_ORDERED_K
        } else {
            usize::MAX
        }
    }

    /// T1/T2: hops to plant on `ℓ` at begin (Seg returns intra-segment count).
    ///
    /// Short chains (≤`ORDER_WINDOW_K`) stay FullChain (storage 14→16→17).
    /// Long thin spines pick min ĉ among {Opt, Win_1/2/3, Seg}; FullChain is
    /// banned after consecutive losses / demote (PR22 1.40ms prepaid wall).
    pub(crate) fn hops_to_plant(&self, location: MemoryLocationHash, n_pairs: usize) -> usize {
        Self::hops_for_strategy(self.loc_strategy(location, n_pairs), n_pairs)
    }

    #[inline]
    fn hops_for_strategy(strategy: LocStrategy, n_pairs: usize) -> usize {
        match strategy {
            LocStrategy::OptimisticRead => 0,
            LocStrategy::Windowed1 => WINDOWED_K.min(n_pairs),
            LocStrategy::Windowed2 => 2.min(n_pairs),
            LocStrategy::Windowed3 => WINDOWED_W_MAX.min(n_pairs),
            LocStrategy::Segmented => Self::segmented_hop_count(n_pairs),
            LocStrategy::FullChain => n_pairs,
        }
    }

    /// Intra-segment hops for `n_pairs` consecutive edges (SEG_TX writers / seg).
    fn segmented_hop_count(n_pairs: usize) -> usize {
        if n_pairs == 0 {
            return 0;
        }
        let n_tx = (n_pairs + 1).min(SEG_CAP * SEG_TX);
        let full_segs = n_tx / SEG_TX;
        let rem = n_tx % SEG_TX;
        full_segs * SEG_TX.saturating_sub(1) + rem.saturating_sub(1)
    }

    /// T1/T2: which stored pairs to plant for `strategy`.
    pub(crate) fn select_pairs_for_strategy(
        strategy: LocStrategy,
        pairs: &[(TxIdx, TxIdx)],
    ) -> Vec<(TxIdx, TxIdx)> {
        let mut pairs = pairs.to_vec();
        pairs.sort_unstable_by_key(|(pred, _)| *pred);
        pairs.dedup();
        match strategy {
            LocStrategy::OptimisticRead => Vec::new(),
            LocStrategy::Windowed1 => pairs.into_iter().take(1).collect(),
            LocStrategy::Windowed2 => pairs.into_iter().take(2).collect(),
            LocStrategy::Windowed3 => pairs.into_iter().take(WINDOWED_W_MAX).collect(),
            LocStrategy::FullChain => pairs,
            LocStrategy::Segmented => intra_segment_pairs(&pairs),
        }
    }

    /// T1 window width for next-quantum hops on `ℓ`.
    pub(crate) fn window_w_of(&self, location: MemoryLocationHash, n_pairs: usize) -> usize {
        self.loc_strategy(location, n_pairs).window_w()
    }

    /// F3: record the begin-time ℓ action (eligibility for later cost).
    pub(crate) fn note_hops_decision(&self, location: MemoryLocationHash, n_pairs: usize) {
        let strategy = self.loc_strategy(location, n_pairs);
        if strategy.is_ordered() {
            self.cost_ev_keep_ordered.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
        }
        if let Some(mut e) = self.promoted.get_mut(&location) {
            e.decision = strategy;
        }
    }

    /// CC-L4: per-ℓ strategy. Hot path reads only snapshots / promoted EMA.
    pub(crate) fn loc_strategy(&self, location: MemoryLocationHash, n_pairs: usize) -> LocStrategy {
        if n_pairs == 0 || !self.is_promoted(location) {
            return LocStrategy::OptimisticRead;
        }
        // CC-L4/L5: short storage-class chain defaults FullChain.
        if n_pairs <= ORDER_WINDOW_K && !self.loc_demoted(location) {
            return LocStrategy::FullChain;
        }
        if !self.is_optimistic_majority_block() {
            return LocStrategy::FullChain;
        }
        // Long spine: never FullChain (PR22 1.40ms prepaid wall).
        self.f7_pick(location, n_pairs)
    }

    /// F2/F7: min-EV among Opt / Win_w / Seg. Full banned on long thin spines.
    fn f7_pick(&self, location: MemoryLocationHash, n_pairs: usize) -> LocStrategy {
        let costs = self.six_way_costs(location, n_pairs);
        let (measured, samples, leftover, escalate_n, last) = self.promoted.get(&location).map_or(
            (false, 0.0, 0, 0, LocStrategy::OptimisticRead),
            |s| {
                (
                    s.measured,
                    s.samples as f64,
                    s.leftover_reexec,
                    s.escalate_n,
                    s.decision,
                )
            },
        );
        // F4: stay OptimisticRead until a measured abort or n₀ samples.
        if !measured && samples < MIN_SAMPLES {
            return LocStrategy::OptimisticRead;
        }
        // S3: a winning OrderedAdmit hop (leftover/reexec cleared) sticks.
        // Do not drop to Opt just because prepaid wall > 0 and abort_cf = 0.
        if measured && leftover == 0 && last.is_ordered() {
            return last;
        }
        let c_opt = costs[0];
        // After a measured EffectiveWAW, OrderedAdmit is the sandwich default
        // unless OptimisticRead is clearly cheaper.
        if measured
            && n_pairs > ORDER_WINDOW_K
            && leftover < LEFTOVER_WIN3
            && escalate_n == 0
            && c_opt + NS_DELTA < costs[1].min(costs[2]).min(costs[3]).min(costs[4])
        {
            return LocStrategy::OptimisticRead;
        }
        // S3: leftover after Opt/Win_1 prefers the lower hop (Win_2).
        // Win_3 only after Win_2 was tried and leftover reexec remains
        // (escalate_n≥2). Seg only after that (escalate_n≥2 ∧ leftover≥SEG).
        let escalate = leftover >= LEFTOVER_WIN3 || escalate_n >= 1;
        let want_seg = escalate_n >= 2 && leftover >= LEFTOVER_SEG;
        let want_win3 = escalate_n >= 2;
        let candidates: &[LocStrategy] = if want_seg {
            &[
                LocStrategy::Windowed2,
                LocStrategy::Windowed3,
                LocStrategy::Segmented,
            ]
        } else if escalate && want_win3 {
            &[LocStrategy::Windowed3, LocStrategy::Windowed2]
        } else if escalate {
            &[LocStrategy::Windowed2, LocStrategy::Windowed3]
        } else {
            &[
                LocStrategy::OptimisticRead,
                LocStrategy::Windowed1,
                LocStrategy::Windowed2,
                LocStrategy::Windowed3,
            ]
        };
        let mut best = candidates[0];
        let mut best_c = cost_of(&costs, best);
        for &a in &candidates[1..] {
            let c = cost_of(&costs, a);
            if c + NS_DELTA < best_c {
                best = a;
                best_c = c;
            }
        }
        // Tie: leftover tail prefers lower hop unless Win_2 already failed.
        if escalate {
            let near: Vec<LocStrategy> = candidates
                .iter()
                .copied()
                .filter(|&a| cost_of(&costs, a) <= best_c + NS_DELTA)
                .collect();
            if want_seg && near.contains(&LocStrategy::Segmented) {
                return LocStrategy::Segmented;
            }
            if want_win3 && near.contains(&LocStrategy::Windowed3) {
                return LocStrategy::Windowed3;
            }
            if near.contains(&LocStrategy::Windowed2) {
                return LocStrategy::Windowed2;
            }
            if near.contains(&LocStrategy::Windowed3) {
                return LocStrategy::Windowed3;
            }
        }
        best
    }

    /// F2: ĉ for Opt / Win_1 / Win_2 / Win_3 / Seg / Full.
    fn six_way_costs(&self, location: MemoryLocationHash, n_pairs: usize) -> [f64; 6] {
        if let Some(s) = self.promoted.get(&location)
            && s.samples >= 1
        {
            return [
                s.c_opt.max(s.reexec_ns_ema).max(1.0),
                s.c_win.max(1.0),
                s.c_win2.max(1.0),
                s.c_win3.max(1.0),
                s.c_seg.max(1.0),
                s.c_full.max(1.0),
            ];
        }
        let abort = self
            .promoted
            .get(&location)
            .map(|s| s.reexec_ns_ema)
            .or_else(|| self.loc_reexec_ns.get(&location).map(|s| s.mean))
            .unwrap_or_else(|| f64::from_bits(self.c_opt_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        let stall = self
            .loc_c_ord
            .get(&location)
            .map(|s| s.mean)
            .unwrap_or_else(|| f64::from_bits(self.c_ord_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        let stall = if self.is_optimistic_majority_block() {
            stall.max(PRIOR_C_ORDERED_THIN_NS)
        } else {
            stall
        };
        [
            abort,
            stall,
            stall,
            stall,
            stall,
            n_pairs.max(1) as f64 * stall,
        ]
    }

    /// Leftover-aware: cost(W) = W·stall + (n−W)·abort vs cost(A0) = n·abort.
    /// Prefix plant loses iff stall ≥ abort. Thin stall is the A1 prior — a
    /// 16-writer wait-for is not 18µs of refuse; it is pred-execute overlap
    /// loss, so thin long spines default to A0 (OCC abort).
    fn long_spine_window_loses(&self, location: MemoryLocationHash, n_pairs: usize) -> bool {
        if n_pairs <= ORDER_WINDOW_K {
            return false;
        }
        self.spine_ordered_loses(location, n_pairs)
            || self.spine_ordered_loses(location, ORDER_WINDOW_K)
    }

    /// ĉ_ordered_spine = hops × ĉ_A1 vs loc abort EMA (not hops×abort).
    /// Full 16-writer prepaid is hops×18µs; one abort sample is tens of µs.
    pub(crate) fn spine_ordered_loses(&self, location: MemoryLocationHash, hops: usize) -> bool {
        if hops == 0 {
            return false;
        }
        let c_ord = self
            .loc_c_ord
            .get(&location)
            .map(|s| s.mean)
            .unwrap_or_else(|| f64::from_bits(self.c_ord_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        // Thin: refuse prior understates wait-for-pred stall (31 blocked on 4).
        let stall = if self.is_optimistic_majority_block() {
            c_ord.max(PRIOR_C_ORDERED_THIN_NS)
        } else {
            c_ord
        };
        let c_abort = self
            .promoted
            .get(&location)
            .map(|s| s.reexec_ns_ema)
            .or_else(|| self.loc_reexec_ns.get(&location).map(|s| s.mean))
            .unwrap_or_else(|| f64::from_bits(self.c_opt_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        let leftover = hops.saturating_sub(1) as f64 * c_abort.max(PRIOR_C_OPT_THIN_NS);
        let ordered_spine = hops as f64 * stall + leftover;
        ordered_spine + NS_DELTA >= c_abort * hops.max(1) as f64
    }

    #[inline]
    pub(crate) fn loc_demoted(&self, location: MemoryLocationHash) -> bool {
        self.promoted.get(&location).is_some_and(|s| s.demoted)
    }

    pub(crate) fn promoted_locations(&self) -> Vec<MemoryLocationHash> {
        self.promoted.iter().map(|e| *e.key()).collect()
    }

    /// D1-shaped writer lists from stored short-edge pairs (edge_4_31 without MV walk).
    pub(crate) fn writer_orders_from_pairs(&self) -> Vec<(MemoryLocationHash, Vec<TxIdx>)> {
        let mut map: hashbrown::HashMap<MemoryLocationHash, Vec<TxIdx>> = hashbrown::HashMap::new();
        for (loc, pred, succ) in self.promoted_short_pairs() {
            let v = map.entry(loc).or_default();
            v.push(pred);
            v.push(succ);
        }
        let mut out: Vec<_> = map
            .into_iter()
            .map(|(loc, mut w)| {
                w.sort_unstable();
                w.dedup();
                (loc, w)
            })
            .collect();
        out.sort_by_key(|(loc, _)| *loc);
        out
    }

    /// Stored consecutive pairs on `ℓ` (consensus order).
    pub(crate) fn pairs_of(&self, location: MemoryLocationHash) -> Vec<(TxIdx, TxIdx)> {
        let mut out = Vec::new();
        if let Some(s) = self.promoted.get(&location)
            && s.pred < s.succ
        {
            out.push((s.pred, s.succ));
        }
        if let Some(e) = self.short_chain.get(&location) {
            for &(pred, succ) in e.value() {
                if pred < succ {
                    out.push((pred, succ));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// O3: record a pair to plant only if the successor is still idle.
    pub(crate) fn queue_idle_edge(&self, location: MemoryLocationHash, pred: TxIdx, succ: TxIdx) {
        if pred >= succ {
            return;
        }
        self.pending_idle
            .lock()
            .unwrap()
            .push((location, pred, succ));
    }

    pub(crate) fn take_pending_idle(&self) -> Vec<(MemoryLocationHash, TxIdx, TxIdx)> {
        std::mem::take(&mut *self.pending_idle.lock().unwrap())
    }

    fn mark_loc_demoted(&self, location: MemoryLocationHash) {
        if let Some(mut e) = self.promoted.get_mut(&location) {
            e.demoted = true;
        }
    }

    #[inline]
    pub(crate) fn block_n(&self) -> usize {
        self.block_n.load(Ordering::Relaxed)
    }

    /// L1 ns-EV: keep A1 iff ĉ_A1 + δ < ĉ_A0. Structural vetoes stay (PC-2).
    /// Beta/`p_eff` are features only — not decide() authority.
    ///
    /// Thin / optimistic_majority_block: default A0. Cold start may be A1=0. Keep
    /// ordered only when a **measured** per-pair (or per-ℓ) prior proves
    /// prepaid cheaper than abort — not the full-shell 8µs-vs-25µs prior.
    pub(crate) fn choose(
        &self,
        kind: CohortKind,
        addr: Address,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> AdmitAction {
        if cohort_len < 2 {
            return AdmitAction::OptimisticRead;
        }
        // PC-2: envelope empty-to on an EOA is LazyRecipient — never A1.
        if kind == CohortKind::EmptyTo && !is_contract {
            self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::OptimisticRead;
        }
        // 2-tx same-from pairs stay A0 (3356896 has ~60; refuse is pure meta).
        if kind == CohortKind::SameFrom && cohort_len < 3 {
            self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::OptimisticRead;
        }
        // A0-majority: 2-tx calldata pairs stay A0 (K reserved for ≥3 spines).
        if self.is_optimistic_majority_block() && kind == CohortKind::CallWaw && cohort_len < 3 {
            self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::OptimisticRead;
        }
        let _ = p_beta; // feature only — used in p_eff for reports, not decide().
        let keep = if self.is_optimistic_majority_block() {
            self.pair_ev_prefers_ordered(kind, addr)
        } else {
            let (c_opt, c_ord) = self.ns_ev_snap();
            self.record_ev(c_opt, c_ord);
            let proven = self
                .cohorts
                .get(&(kind.as_u8(), addr))
                .is_some_and(|s| s.p_eff.mean >= 0.40 && s.p_eff.n >= 2.0);
            proven || c_ord + NS_DELTA < c_opt
        };
        if keep {
            self.cost_ev_keep_ordered.fetch_add(1, Ordering::Relaxed);
            self.ordered_decisions.fetch_add(1, Ordering::Relaxed);
            AdmitAction::OrderedAdmit
        } else {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
            self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
            AdmitAction::OptimisticRead
        }
    }

    #[inline]
    pub(crate) fn choose_ordered(
        &self,
        kind: CohortKind,
        addr: Address,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> bool {
        self.choose(kind, addr, cohort_len, is_contract, p_beta) == AdmitAction::OrderedAdmit
    }

    #[inline]
    fn ns_ev_snap(&self) -> (f64, f64) {
        let c_opt = f64::from_bits(self.c_opt_snap_bits.load(Ordering::Relaxed)).max(1.0);
        let c_ord = f64::from_bits(self.c_ord_snap_bits.load(Ordering::Relaxed)).max(1.0);
        (c_opt, c_ord)
    }

    /// L2: per address-pair EV. Thin cold (no measured samples) → A0.
    fn pair_ev_prefers_ordered(&self, kind: CohortKind, addr: Address) -> bool {
        let Some(s) = self.cohorts.get(&(kind.as_u8(), addr)) else {
            return false;
        };
        // Need measured abort/prepaid samples — p_eff alone does not freeze A1.
        let measured = s.c_opt.n >= 2.0 || s.c_ord.n >= 2.0;
        if !measured {
            return false;
        }
        let c_opt = s.c_opt.mean.max(1.0);
        let c_ord = s.c_ord.mean.max(1.0);
        self.record_ev(c_opt, c_ord);
        c_ord + NS_DELTA < c_opt
    }

    /// Score for A0-majority K-cap (storage/call WAW > contract empty-to).
    pub(crate) fn ordered_score(kind: CohortKind, cohort_len: usize, is_contract: bool) -> i64 {
        let class = match kind {
            CohortKind::RawFan => 4000,
            CohortKind::CallWaw => 3000,
            CohortKind::EmptyTo if is_contract => 2000,
            CohortKind::SameFrom => 1000,
            CohortKind::EmptyTo => 0,
        };
        class + cohort_len as i64
    }

    pub(crate) fn note_k_cap_demote(&self) {
        let a1 = self.ordered_decisions.load(Ordering::Relaxed);
        if a1 > 0 {
            self.ordered_decisions.fetch_sub(1, Ordering::Relaxed);
        }
        self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
        self.k_cap_demote.fetch_add(1, Ordering::Relaxed);
    }

    fn p_eff(
        &self,
        kind: CohortKind,
        addr: Address,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> f64 {
        if let Some(st) = self.cohorts.get(&(kind.as_u8(), addr))
            && st.p_eff.n >= 3.0
        {
            return st.p_eff.mean.clamp(0.02, 0.95);
        }
        let x = self.features(kind, cohort_len, is_contract, p_beta);
        let w = *self.w_p.lock().unwrap();
        let z: f64 = w.iter().zip(x.iter()).map(|(a, b)| a * b).sum();
        sigmoid(z).clamp(0.02, 0.95)
    }

    fn features(
        &self,
        kind: CohortKind,
        cohort_len: usize,
        is_contract: bool,
        p_beta: f64,
    ) -> [f64; 6] {
        let n = self.block_n() as f64;
        let logn = ((cohort_len as f64).max(1.0)).ln();
        let storage = matches!(kind, CohortKind::CallWaw | CohortKind::RawFan) as u8 as f64;
        let small = if n > 0.0 && n <= 256.0 { 1.0 } else { 0.0 };
        let contract = if is_contract || matches!(kind, CohortKind::CallWaw | CohortKind::RawFan) {
            1.0
        } else {
            0.0
        };
        [1.0, contract, logn, storage, small, p_beta.clamp(0.0, 1.0)]
    }

    fn refuse_cost(&self, kind: CohortKind, addr: Address, cohort_len: usize) -> f64 {
        let base = self
            .cohorts
            .get(&(kind.as_u8(), addr))
            .map(|s| s.refuse.mean)
            .unwrap_or_else(|| self.refuse_global.lock().unwrap().mean);
        // Short chains: refuse + width almost always lose to one cheap abort.
        let short = if cohort_len < 3 && !matches!(kind, CohortKind::CallWaw | CohortKind::RawFan) {
            0.18
        } else {
            0.0
        };
        // PC-5 small-block: inflate A1 meta so lazy cohorts stay A0.
        let n = self.block_n() as f64;
        let lean = if n > 0.0 && n <= 256.0 { 1.15 } else { 1.0 };
        (base * lean + short).clamp(0.08, 1.5)
    }

    fn width_loss(&self, cohort_len: usize) -> f64 {
        let idle = self.idle_global.lock().unwrap().mean;
        let cores = 8.0;
        idle * ((cohort_len.saturating_sub(1) as f64) / cores).min(1.0)
    }

    fn record_ev(&self, e_a0: f64, e_a1: f64) {
        add_f64(&self.c_opt_sum_bits, e_a0);
        add_f64(&self.c_ord_sum_bits, e_a1);
        self.ev_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// B2: observe effective vs lazy WAW on a cohort (write-set Detect).
    pub(crate) fn note_eff_waw(&self, kind: CohortKind, addr: Address, effective: bool) {
        let thin = self.is_optimistic_majority_block();
        let mut e = self
            .cohorts
            .entry((kind.as_u8(), addr))
            .or_insert(CohortStat {
                p_eff: OnlineStat::new(if effective { 0.55 } else { 0.12 }),
                refuse: OnlineStat::new(0.22),
                c_opt: OnlineStat::new(if thin {
                    PRIOR_C_OPT_THIN_NS
                } else {
                    PRIOR_C_OPT_NS
                }),
                c_ord: OnlineStat::new(if thin {
                    PRIOR_C_ORDERED_THIN_NS
                } else {
                    PRIOR_C_ORDERED_NS
                }),
                prepaid_lose: 0,
            });
        e.p_eff.ema(if effective { 1.0 } else { 0.0 }, 0.25);
        // L4: no w_p SGD on the Detect hot path — pair EV is enough.
        let _ = effective;
    }

    /// L4: refuse ns is an atomic add; EMA flush is end-block.
    pub(crate) fn note_refuse_ns(&self, ns: u64) {
        if ns == 0 {
            return;
        }
        self.refuse_ns.fetch_add(ns, Ordering::Relaxed);
    }

    /// L1: measured incarnation>0 execute ns (atomic; EMA at end-block / loc).
    pub(crate) fn note_reexec_ns(&self, ns: u64) {
        self.note_reexec_ns_at(None, ns);
    }

    /// L2: reexec_ns EMA on a conflict ℓ (or global if `loc` is None).
    pub(crate) fn note_reexec_ns_at(&self, loc: Option<MemoryLocationHash>, ns: u64) {
        if ns == 0 {
            return;
        }
        self.reexec_ns.fetch_add(ns, Ordering::Relaxed);
        self.optimistic_reexec.fetch_add(1, Ordering::Relaxed);
        if let Some(loc) = loc {
            self.loc_reexec_ns
                .entry(loc)
                .or_insert(OnlineStat::new(PRIOR_C_OPT_NS))
                .ema_ns(ns as f64, EMA_ALPHA);
            if let Some(mut e) = self.promoted.get_mut(&loc) {
                e.block_reexec_ns = e.block_reexec_ns.saturating_add(ns);
                e.block_reexec_n = e.block_reexec_n.saturating_add(1);
            }
        }
    }

    /// F1/F3: refuse/wait ns attributed to the ℓ that decided OrderedAdmit.
    pub(crate) fn note_loc_ordered_ns(&self, location: MemoryLocationHash, ns: u64) {
        if ns == 0 {
            return;
        }
        if let Some(mut e) = self.promoted.get_mut(&location) {
            e.block_ordered_ns = e.block_ordered_ns.saturating_add(ns);
            e.block_ordered_n = e.block_ordered_n.saturating_add(1);
        }
        self.loc_c_ord
            .entry(location)
            .or_insert(OnlineStat::new(PRIOR_C_ORDERED_NS))
            .ema_ns(ns as f64, EMA_ALPHA);
    }

    /// L4: idle-core ns attributed as A1 width loss (atomic; EMA at end-block).
    pub(crate) fn note_width_loss_ns(&self, ns: u64) {
        if ns == 0 {
            return;
        }
        self.width_loss_ns.fetch_add(ns, Ordering::Relaxed);
    }

    /// B2: observed refuse / reexec cost sample. Instant idle must **not**
    /// enter width_loss / prepaid / ĉ (PROFILE taught Win_3→Win_1 trains).
    pub(crate) fn note_cost_sample(&self, refuse_unit: f64, reexec: bool, idle_ns: u64) {
        let _ = idle_ns;
        if refuse_unit > 0.0 {
            self.refuse_global.lock().unwrap().ema(refuse_unit, 0.15);
        }
        if reexec {
            self.reexec_global.lock().unwrap().ema(1.0, 0.20);
        }
    }

    pub(crate) fn note_commute_skip(&self) {
        self.commute_skip.fetch_add(1, Ordering::Relaxed);
    }

    /// Probe-star early release: commute absorbed and no measured reexec —
    /// cost-EV demotes remaining EmptyTo ordered-admit waiters.
    #[inline]
    pub(crate) fn should_release_probe_star(&self) -> bool {
        self.is_optimistic_majority_block()
            && self.commute_skip.load(Ordering::Relaxed) >= 8
            && self.reexec_ns.load(Ordering::Relaxed) == 0
    }

    pub(crate) fn note_batch_repair(&self) {
        self.batch_repair.fetch_add(1, Ordering::Relaxed);
    }

    /// CC-D1: first conflict ℓ + peer + class on an off-edge inc>0 tx.
    pub(crate) fn note_conflict_ell(
        &self,
        tx: TxIdx,
        location: MemoryLocationHash,
        peer: Option<TxIdx>,
        class: ConflictClass,
        lazy: bool,
    ) {
        self.conflicts.entry(tx).or_insert(ConflictNote {
            location,
            peer,
            class,
            lazy,
        });
    }

    /// C3: one off-edge reexec in this wave. First time count ≥ 2 → batch_repair.
    pub(crate) fn note_wave_off_edge_reexec(&self) {
        let n = self.wave_off_edge.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= 2 && !self.wave_batch_noted.swap(true, Ordering::Relaxed) {
            self.note_batch_repair();
        }
    }

    /// C4: EffectiveWAW → short-edge promote (location only, not a lazy spine).
    ///
    /// L1: `reexec_ns == 0` still sets `measured=true` using the location EMA or
    /// the abort hat. Passing 0 must not lock the edge unpromoted for the block.
    pub(crate) fn promote_short_edge(&self, location: MemoryLocationHash, reexec_ns: u64) {
        let measured_ns = if reexec_ns > 0 {
            reexec_ns as f64
        } else {
            self.loc_reexec_ns
                .get(&location)
                .map(|s| s.mean)
                .filter(|m| *m > 1.0)
                .unwrap_or(PRIOR_C_OPT_NS)
        };
        let mut e = self.promoted.entry(location).or_insert(PromotedLoc::new());
        e.hits = e.hits.saturating_add(1);
        e.reexec_ns_ema = (1.0 - EMA_ALPHA) * e.reexec_ns_ema + EMA_ALPHA * measured_ns;
        e.measured = true;
        drop(e);
        self.note_conflict_promote();
        self.note_reexec_ns_at(Some(location), reexec_ns);
    }

    /// L2/L4: remember the immediate (pred, succ) pair on a hot ℓ.
    pub(crate) fn note_short_pair(&self, location: MemoryLocationHash, pred: TxIdx, succ: TxIdx) {
        if pred >= succ {
            return;
        }
        let mut e = self.promoted.entry(location).or_insert(PromotedLoc::new());
        if e.pred == usize::MAX || pred < e.pred {
            e.pred = pred;
            e.succ = succ;
        }
        drop(e);
        let mut chain = self.short_chain.entry(location).or_default();
        if !chain.iter().any(|&(p, s)| p == pred && s == succ) {
            chain.push((pred, succ));
        }
    }

    /// Post-publish (end-block): persist consecutive D1 pairs on already
    /// promoted ℓ. Completes 4→31→66→… after the first block without
    /// inserting ReadyEdges mid-execute.
    pub(crate) fn note_promoted_writer_orders(&self, orders: &[(MemoryLocationHash, Vec<TxIdx>)]) {
        for (loc, writers) in orders {
            if writers.len() < 2 || !self.is_promoted(*loc) {
                continue;
            }
            for pair in writers.windows(2) {
                self.note_short_pair(*loc, pair[0], pair[1]);
            }
        }
    }

    /// L4: stored short-edge pairs whose idx still match this block size.
    pub(crate) fn promoted_short_pairs(&self) -> Vec<(MemoryLocationHash, TxIdx, TxIdx)> {
        let n = self.block_n();
        let prev = self.last_block_n.load(Ordering::Relaxed);
        let same_shape = n > 0 && (prev == 0 || prev == n);
        if !same_shape {
            return Vec::new();
        }
        let mut out = Vec::new();
        for e in self.promoted.iter() {
            let s = *e.value();
            if s.hits >= 1 && s.measured && s.pred < s.succ && s.succ < n {
                out.push((*e.key(), s.pred, s.succ));
            }
        }
        for e in self.short_chain.iter() {
            let loc = *e.key();
            if !self.is_promoted(loc) {
                continue;
            }
            for &(pred, succ) in e.value() {
                if pred < succ && succ < n {
                    out.push((loc, pred, succ));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// C3: keep a short edge when ĉ_reexec (ℓ EMA) > ĉ_ordered.
    /// First EffectiveWAW / stored pair is enough proof on a thin block.
    pub(crate) fn loc_ev_prefers_ordered(&self, location: MemoryLocationHash) -> bool {
        let c_reexec = self
            .promoted
            .get(&location)
            .map(|s| s.reexec_ns_ema)
            .or_else(|| self.loc_reexec_ns.get(&location).map(|s| s.mean))
            .unwrap_or(PRIOR_C_OPT_NS)
            .max(1.0);
        let c_ord = self
            .loc_c_ord
            .get(&location)
            .map(|s| s.mean)
            .unwrap_or_else(|| f64::from_bits(self.c_ord_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        c_ord + NS_DELTA < c_reexec
    }

    /// C1/C2/C3: gate a short ReadyEdge after an effective publish.
    ///
    /// `has_earlier` — D1 already saw a prior writer (4→31).
    /// `hint_later` — envelope successors still unpublished (14→16, 31→66).
    pub(crate) fn should_gate_short_after_write(
        &self,
        location: MemoryLocationHash,
        has_earlier: bool,
        hint_later: usize,
    ) -> bool {
        if self.is_promoted(location) {
            let n_pairs = self
                .short_chain
                .get(&location)
                .map(|c| c.len())
                .unwrap_or(0);
            // Long thin spine: never mid-execute ReadyEdge (CC-L1 is validate /
            // next-quantum). Mid-execute plant rebuilt the prepaid wall.
            if n_pairs > ORDER_WINDOW_K && self.is_optimistic_majority_block() {
                return false;
            }
            return true;
        }
        if !has_earlier && hint_later < 2 {
            return false;
        }
        if self.is_optimistic_majority_block() && !self.can_add_short_loc(location) {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
            return false;
        }
        // C2: first effective write with a known successor, or 2nd writer on ℓ.
        // C3: loc EV / abort hat must still beat prepaid.
        let keep = has_earlier || hint_later >= 2 || self.loc_ev_prefers_ordered(location);
        if keep {
            self.cost_ev_keep_ordered.fetch_add(1, Ordering::Relaxed);
            self.ordered_decisions.fetch_add(1, Ordering::Relaxed);
        } else {
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
        }
        keep
    }

    fn can_add_short_loc(&self, location: MemoryLocationHash) -> bool {
        if self.promoted.contains_key(&location) {
            return true;
        }
        let n = self
            .promoted
            .iter()
            .filter(|e| e.measured && e.hits >= 1)
            .count();
        n < THIN_ORDERED_K
    }

    /// C5: one real short edge (not an envelope cohort).
    pub(crate) fn note_short_edge_admit(&self) {
        self.ordered_decisions.fetch_add(1, Ordering::Relaxed);
        self.cost_ev_keep_ordered.fetch_add(1, Ordering::Relaxed);
    }

    /// C4: commute / LazyNoise → ignore (do not raise unfenced_reexec→A1).
    pub(crate) fn ignore_conflict(&self, location: Option<MemoryLocationHash>) {
        self.note_conflict_ignore();
        if let Some(loc) = location {
            // Successful commute lowers the promote weight on this ℓ.
            if let Some(mut e) = self.promoted.get_mut(&loc)
                && e.hits > 0
            {
                e.hits -= 1;
            }
        }
    }

    #[inline]
    pub(crate) fn is_promoted(&self, location: MemoryLocationHash) -> bool {
        let Some(s) = self.promoted.get(&location) else {
            return false;
        };
        let n_pairs = self
            .short_chain
            .get(&location)
            .map(|c| c.len())
            .unwrap_or(0);
        // Commute/ignore must not evict a measured long Basic spine (F7).
        // F4 demote bans FullChain only — Win_w / Seg may still plant.
        if s.measured && n_pairs > ORDER_WINDOW_K {
            return true;
        }
        if s.hits < 1 {
            return false;
        }
        // Demote bans FullChain, not the hot-ℓ identity (WindowedOrdered may win).
        if s.demoted && self.is_optimistic_majority_block() && !s.measured {
            return false;
        }
        if !self.is_optimistic_majority_block() {
            return true;
        }
        if !s.measured {
            return false;
        }
        // C3: per-ℓ abort EMA vs prepaid snap (not the thin global 6µs A0 prior).
        let c_ord = self
            .loc_c_ord
            .get(&location)
            .map(|e| e.mean)
            .unwrap_or_else(|| f64::from_bits(self.c_ord_snap_bits.load(Ordering::Relaxed)))
            .max(1.0);
        c_ord + NS_DELTA < s.reexec_ns_ema
    }

    pub(crate) fn conflict_of(&self, tx: TxIdx) -> Option<ConflictNote> {
        self.conflicts.get(&tx).map(|e| *e)
    }

    pub(crate) fn note_conflict_promote(&self) {
        self.conflict_promote.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_conflict_ignore(&self) {
        self.conflict_ignore.fetch_add(1, Ordering::Relaxed);
    }

    /// D4: off-edge incarnation>0 — miss-Detect, raise p_eff on the location class.
    pub(crate) fn note_unfenced_reexec(&self, kind: CohortKind, addr: Address) {
        self.bump_unfenced_reexec();
        self.note_eff_waw(kind, addr, true);
    }

    pub(crate) fn bump_unfenced_reexec(&self) {
        self.unfenced_reexec.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn note_optimistic_reexec(&self) {
        self.optimistic_reexec.fetch_add(1, Ordering::Relaxed);
        self.reexec_global.lock().unwrap().ema(1.0, 0.20);
    }

    /// L2 test / end-block helper: write a measured address-pair EV sample.
    pub(crate) fn note_pair_measured_ev(
        &self,
        kind: CohortKind,
        addr: Address,
        c_opt: f64,
        c_ord: f64,
    ) {
        let thin = self.is_optimistic_majority_block();
        let mut e = self
            .cohorts
            .entry((kind.as_u8(), addr))
            .or_insert(CohortStat {
                p_eff: OnlineStat::new(0.55),
                refuse: OnlineStat::new(0.22),
                c_opt: OnlineStat::new(if thin {
                    PRIOR_C_OPT_THIN_NS
                } else {
                    PRIOR_C_OPT_NS
                }),
                c_ord: OnlineStat::new(if thin {
                    PRIOR_C_ORDERED_THIN_NS
                } else {
                    PRIOR_C_ORDERED_NS
                }),
                prepaid_lose: 0,
            });
        e.c_opt = OnlineStat {
            n: 3.0,
            mean: c_opt.max(1.0),
        };
        e.c_ord = OnlineStat {
            n: 3.0,
            mean: c_ord.max(1.0),
        };
    }

    /// L3: flush EMAs, compare prepaid vs abort counterfactual, decay if prepaid loses.
    pub(crate) fn end_block_learn(&self) {
        let refuse = self.refuse_ns.load(Ordering::Relaxed);
        let reexec = self.reexec_ns.load(Ordering::Relaxed);
        // S2: prepaid is wall-clock gate stall only. Instant idle / width
        // must not enter ĉ (PROFILE Win_3→Win_1).
        let prepaid = refuse;
        let abort_cf = reexec;
        self.prepaid_ns.store(prepaid, Ordering::Relaxed);
        self.abort_cf_ns.store(abort_cf, Ordering::Relaxed);
        if refuse > 0 {
            self.c_ord_ns
                .lock()
                .unwrap()
                .ema_ns(refuse as f64, EMA_ALPHA);
        }
        if reexec > 0 {
            self.c_opt_ns
                .lock()
                .unwrap()
                .ema_ns(reexec as f64, EMA_ALPHA);
        }
        // Successful Detect / a single leftover miss must not decay to Win_1.
        // Only an abort *train* plus prepaid wall is a real Detect loss.
        let unfenced = self.unfenced_reexec.load(Ordering::Relaxed);
        let prepaid_lost = prepaid > abort_cf && prepaid > 0 && unfenced >= 4;
        if prepaid_lost {
            let n = self.prepaid_lose_streak.fetch_add(1, Ordering::Relaxed) + 1;
            if n >= PREPAID_LOSE_N as usize {
                self.decay_ordered_priors();
            }
            // O2/L1: long spines whose ordered EV lost → next begin plants 0.
            self.demote_long_spines();
        } else {
            self.prepaid_lose_streak.store(0, Ordering::Relaxed);
        }
        // L-E: abort_cf without prepaid → raise *short* edges. Long spines
        // that O2 demoted stay A0 — those aborts are the cheaper path.
        if abort_cf > prepaid {
            let long: hashbrown::HashSet<MemoryLocationHash> = self
                .short_chain
                .iter()
                .filter(|e| e.value().len() > ORDER_WINDOW_K)
                .map(|e| *e.key())
                .collect();
            for mut e in self.promoted.iter_mut() {
                if e.measured {
                    e.hits = e.hits.saturating_add(1);
                    if !long.contains(e.key()) {
                        e.demoted = false;
                    }
                }
            }
        }
        // F1–F7: per-ℓ reward = −(reexec_ns+ordered_ns+refuse_share); update
        // Opt/Win_w/Seg/Full ĉ; leftover tail escalates Win_1→Win_2/3 or Seg.
        self.update_loc_counterfactuals(prepaid, abort_cf);
        // L-D: refresh thin snap from flushed EMAs so next begin can seed.
        // Use per-tx abort hat, not the whole-block reexec sum.
        let c_opt = if abort_cf > 0 {
            self.c_opt_ns.lock().unwrap().mean.max(PRIOR_C_OPT_NS)
        } else {
            self.c_opt_ns.lock().unwrap().mean.max(1.0)
        };
        let c_ord = self.c_ord_ns.lock().unwrap().mean.max(1.0);
        if !self.is_optimistic_majority_block() || abort_cf > 0 {
            self.c_opt_snap_bits
                .store(c_opt.to_bits(), Ordering::Relaxed);
            self.c_ord_snap_bits
                .store(c_ord.to_bits(), Ordering::Relaxed);
        }
    }

    fn update_loc_counterfactuals(&self, prepaid: u64, abort_cf: u64) {
        let n_hot = self.promoted.iter().filter(|e| e.measured).count().max(1) as u64;
        let refuse_share = prepaid / n_hot;
        for mut e in self.promoted.iter_mut() {
            if !e.measured && e.block_reexec_ns == 0 && e.block_ordered_ns == 0 {
                continue;
            }
            let actual = e
                .block_reexec_ns
                .saturating_add(e.block_ordered_ns.saturating_add(refuse_share));
            let x = actual.max(1) as f64;
            // F3: credit the decision-time action, not the later abort site.
            match e.decision {
                LocStrategy::OptimisticRead => {
                    e.c_opt = (1.0 - EMA_ALPHA) * e.c_opt + EMA_ALPHA * x;
                }
                LocStrategy::Windowed1 => {
                    e.c_win = (1.0 - EMA_ALPHA) * e.c_win + EMA_ALPHA * x;
                }
                LocStrategy::Windowed2 => {
                    e.c_win2 = (1.0 - EMA_ALPHA) * e.c_win2 + EMA_ALPHA * x;
                }
                LocStrategy::Windowed3 => {
                    e.c_win3 = (1.0 - EMA_ALPHA) * e.c_win3 + EMA_ALPHA * x;
                }
                LocStrategy::Segmented => {
                    e.c_seg = (1.0 - EMA_ALPHA) * e.c_seg + EMA_ALPHA * x;
                }
                LocStrategy::FullChain => {
                    e.c_full = (1.0 - EMA_ALPHA) * e.c_full + EMA_ALPHA * x;
                }
            }
            // F2: unused OptimisticRead would have paid at least this abort.
            if e.decision != LocStrategy::OptimisticRead && abort_cf > 0 {
                let cf = (abort_cf as f64).max(e.reexec_ns_ema).max(1.0);
                e.c_opt = e.c_opt.max(cf);
            }
            if !matches!(
                e.decision,
                LocStrategy::Windowed1 | LocStrategy::Windowed2 | LocStrategy::Windowed3
            ) && e.block_ordered_n > 0
            {
                e.c_win =
                    (1.0 - EMA_ALPHA) * e.c_win + EMA_ALPHA * (e.block_ordered_ns.max(1) as f64);
            }
            e.samples = e.samples.saturating_add(1);
            let n_pairs = self.short_chain.get(e.key()).map(|c| c.len()).unwrap_or(0);
            let planted = Self::hops_for_strategy(e.decision, n_pairs);
            let leftover_hops = n_pairs.saturating_sub(planted) as u32;
            // Opt + a long unplanted spine must escalate (cold 4→31…171).
            // After OrderedAdmit, leftover is measured reexec only.
            if e.decision == LocStrategy::OptimisticRead && leftover_hops >= LEFTOVER_WIN3 {
                e.leftover_reexec = leftover_hops.max(e.block_reexec_n);
                e.escalate_n = e.escalate_n.saturating_add(1);
            } else if e.block_reexec_n >= 2 {
                e.leftover_reexec = e.block_reexec_n;
                e.escalate_n = e.escalate_n.saturating_add(1);
            } else if e.block_reexec_n == 0 && e.decision.is_ordered() {
                e.leftover_reexec = 0;
                e.escalate_n = 0;
            } else {
                e.leftover_reexec = e.block_reexec_n;
            }
            if e.decision == LocStrategy::FullChain && n_pairs > ORDER_WINDOW_K {
                if e.c_full + NS_DELTA >= e.c_opt.min(e.c_win) {
                    e.fullchain_lose = e.fullchain_lose.saturating_add(1);
                    if e.fullchain_lose >= PREPAID_LOSE_N {
                        e.demoted = true;
                    }
                } else {
                    e.fullchain_lose = 0;
                }
            }
        }
    }

    fn decay_ordered_priors(&self) {
        self.prior_decay.fetch_add(1, Ordering::Relaxed);
        for mut e in self.cohorts.iter_mut() {
            e.p_eff.ema(0.0, 0.35);
            let raised = e.c_ord.mean * 1.25 + PRIOR_C_ORDERED_THIN_NS * 0.15;
            e.c_ord.ema_ns(raised, EMA_ALPHA);
            e.prepaid_lose = e.prepaid_lose.saturating_add(1);
        }
        for mut e in self.promoted.iter_mut() {
            if e.hits > 0 {
                e.hits -= 1;
            }
        }
    }

    /// O2: when prepaid lost, demote locations whose stored chain is longer
    /// than the window (16-writer Basic WAW). Storage trio (2 hops) stays.
    fn demote_long_spines(&self) {
        let long: Vec<MemoryLocationHash> = self
            .short_chain
            .iter()
            .filter(|e| e.value().len() > ORDER_WINDOW_K)
            .filter(|e| self.spine_ordered_loses(*e.key(), e.value().len()))
            .map(|e| *e.key())
            .collect();
        for loc in long {
            self.mark_loc_demoted(loc);
            self.cost_ev_demote_optimistic
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn take_report(&self, ready_width_mean: f64, idle_core_ns: u64) -> LearnReport {
        let n = self.ev_samples.load(Ordering::Relaxed).max(1) as f64;
        let refuse = self.refuse_ns.load(Ordering::Relaxed);
        let ordered: u64 = self.promoted.iter().map(|e| e.block_ordered_ns).sum();
        let mut census = [0usize; 6];
        for e in self.promoted.iter() {
            if e.hits < 1 {
                continue;
            }
            let i = e.decision.as_u8() as usize;
            if i < census.len() {
                census[i] += 1;
            }
        }
        // Long-spine action is the F7 story; storage Full must not hide Win_w/Seg.
        let dominant = if census[4] > 0 {
            LocStrategy::Segmented
        } else if census[3] > 0 {
            LocStrategy::Windowed3
        } else if census[2] > 0 {
            LocStrategy::Windowed2
        } else if census[1] > 0 {
            LocStrategy::Windowed1
        } else if census[5] > 0 {
            LocStrategy::FullChain
        } else {
            LocStrategy::OptimisticRead
        };
        LearnReport {
            ordered_admit_cohorts: self.ordered_decisions.load(Ordering::Relaxed),
            optimistic_read_cohorts: self.optimistic_read_cohorts.load(Ordering::Relaxed),
            mean_c_ordered: {
                let ema = self.c_ord_ns.lock().unwrap().mean;
                if ema > 0.0 {
                    ema
                } else {
                    f64::from_bits(self.c_ord_sum_bits.load(Ordering::Relaxed)) / n
                }
            },
            mean_c_optimistic_cf: {
                let ema = self.c_opt_ns.lock().unwrap().mean;
                if ema > 0.0 {
                    ema
                } else {
                    f64::from_bits(self.c_opt_sum_bits.load(Ordering::Relaxed)) / n
                }
            },
            optimistic_reexec: self.optimistic_reexec.load(Ordering::Relaxed),
            unfenced_reexec: self.unfenced_reexec.load(Ordering::Relaxed),
            ready_width_mean,
            idle_core_ns,
            optimistic_majority_block: self.is_optimistic_majority_block(),
            refuse_ns: refuse,
            reexec_ns: self.reexec_ns.load(Ordering::Relaxed),
            ordered_ns: ordered.max(refuse),
            cost_ev_keep_ordered: self.cost_ev_keep_ordered.load(Ordering::Relaxed),
            cost_ev_demote_optimistic: self.cost_ev_demote_optimistic.load(Ordering::Relaxed),
            k_cap_demote: self.k_cap_demote.load(Ordering::Relaxed),
            commute_skip: self.commute_skip.load(Ordering::Relaxed),
            batch_repair: self.batch_repair.load(Ordering::Relaxed),
            conflict_promote: self.conflict_promote.load(Ordering::Relaxed),
            conflict_ignore: self.conflict_ignore.load(Ordering::Relaxed),
            prepaid_ns: self.prepaid_ns.load(Ordering::Relaxed),
            abort_cf_ns: self.abort_cf_ns.load(Ordering::Relaxed),
            prior_decay: self.prior_decay.load(Ordering::Relaxed),
            end_block_ns: 0,
            optimistic_path_tax_ns: 0,
            opt_locs: census[0],
            win1_locs: census[1],
            win2_locs: census[2],
            win3_locs: census[3],
            seg_locs: census[4],
            full_locs: census[5],
            chosen_strategy: dominant.label().to_string(),
            chosen_win_w: dominant.window_w().min(255) as u8,
            pick_occ_n: 0,
            pick_gate_n: 0,
            skip_gate_n: 0,
            occ_pick_while_gated: 0,
            yield_ns: 0,
            gate_stall_ns: 0,
            worker_busy_ns: 0,
        }
    }

    pub(crate) fn set_end_block_ns(&self, report: &mut LearnReport, ns: u64) {
        report.end_block_ns = ns;
    }
}

fn cost_of(costs: &[f64; 6], strategy: LocStrategy) -> f64 {
    costs[strategy.as_u8() as usize]
}

/// T2: keep hops whose endpoints share a SEG_TX writer-bucket.
fn intra_segment_pairs(pairs: &[(TxIdx, TxIdx)]) -> Vec<(TxIdx, TxIdx)> {
    let mut writers: Vec<TxIdx> = pairs.iter().flat_map(|(a, b)| [*a, *b]).collect();
    writers.sort_unstable();
    writers.dedup();
    let idx = |t: TxIdx| writers.binary_search(&t).ok();
    pairs
        .iter()
        .copied()
        .filter(|&(pred, succ)| match (idx(pred), idx(succ)) {
            (Some(a), Some(b)) => {
                a / SEG_TX == b / SEG_TX && a / SEG_TX < SEG_CAP && b / SEG_TX < SEG_CAP
            }
            _ => false,
        })
        .collect()
}

fn sigmoid(z: f64) -> f64 {
    let z = z.clamp(-12.0, 12.0);
    1.0 / (1.0 + (-z).exp())
}

fn add_f64(slot: &AtomicU64, x: f64) {
    let mut cur = slot.load(Ordering::Relaxed);
    loop {
        let next = (f64::from_bits(cur) + x).to_bits();
        match slot.compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(v) => cur = v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_empty_to_eoa_is_a0() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x9e);
        assert!(
            !p.choose_ordered(CohortKind::EmptyTo, addr, 12, false, 0.10),
            "lazy EOA empty-to must be A0 (PC-2/PC-5)"
        );
    }

    #[test]
    fn thin_cold_start_defaults_a0() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        assert!(
            !p.choose_ordered(CohortKind::EmptyTo, addr, 16, true, 0.30),
            "L1: thin cold start must not freeze A1 on contract empty-to"
        );
        let storage = Address::repeat_byte(0xed);
        assert!(
            !p.choose_ordered(CohortKind::CallWaw, storage, 3, true, 0.25),
            "L1: thin cold start must not freeze A1 on short calldata WAW"
        );
    }

    #[test]
    fn full_shell_contract_empty_to_hot_payee_is_a1() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0x20);
        assert!(
            p.choose_ordered(CohortKind::EmptyTo, addr, 16, true, 0.30),
            "full-shell contract empty-to n=16 stays A1"
        );
    }

    #[test]
    fn full_shell_calldata_short_waw_is_a1() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0xed);
        assert!(
            p.choose_ordered(CohortKind::CallWaw, addr, 3, true, 0.25),
            "full-shell storage 14-17 class stays A1"
        );
    }

    #[test]
    fn two_tx_same_from_stays_a0() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x56);
        assert!(
            !p.choose_ordered(CohortKind::SameFrom, addr, 2, false, 0.20),
            "2-tx same-from must stay A0"
        );
    }

    #[test]
    fn thin_two_tx_callwaw_stays_a0() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0xab);
        assert!(
            !p.choose_ordered(CohortKind::CallWaw, addr, 2, true, 0.25),
            "thin-shell 2-tx calldata stays A0; K reserved for ≥3 spines"
        );
    }

    #[test]
    fn b2_raises_p_after_effective_waw() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0x20);
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        assert!(
            p.choose_ordered(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "full-shell: after effective WAW observations A1 remains available"
        );
    }

    #[test]
    fn thin_promotes_only_when_pair_ev_wins() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        assert!(
            !p.choose_ordered(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "thin: p_eff alone must not freeze A1"
        );
        p.note_pair_measured_ev(CohortKind::EmptyTo, addr, 80_000.0, 8_000.0);
        assert!(
            p.choose_ordered(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "thin: measured ĉ_A1+δ < ĉ_A0 promotes a short edge"
        );
    }

    #[test]
    fn optimistic_majority_block_on_small_block() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(
            p.is_optimistic_majority_block(),
            "n=176 serial_hat < meta floor"
        );
        assert_eq!(
            p.thin_ordered_k(),
            THIN_ORDERED_K,
            "K is a cap on thin-shell"
        );
        p.begin_block(4096);
        assert!(
            !p.is_optimistic_majority_block(),
            "large n keeps full shell"
        );
        assert_eq!(p.thin_ordered_k(), usize::MAX);
    }

    #[test]
    fn thin_ordered_k_is_cap_not_exact_set() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert_eq!(p.thin_ordered_k(), 3);
        // Promote does not freeze A1 at exactly 3 — it can add a short edge
        // that later admit ranks under the same cap.
        p.promote_short_edge(0x32be, 4_000);
        assert!(p.is_promoted(0x32be));
        p.ignore_conflict(Some(0x32be));
        assert!(!p.is_promoted(0x32be), "ignore can drop a promoted ℓ");
    }

    #[test]
    fn batch_repair_fires_on_second_off_edge_reexec() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.note_wave_off_edge_reexec();
        assert_eq!(p.take_report(0.0, 0).batch_repair, 0);
        p.note_wave_off_edge_reexec();
        assert_eq!(
            p.take_report(0.0, 0).batch_repair,
            1,
            "C3: second off-edge abort in the wave is one batch_repair"
        );
    }

    #[test]
    fn cost_ev_demote_optimistics_when_refuse_dominates() {
        let p = CostPolicy::new();
        p.begin_block(176);
        for _ in 0..8 {
            p.note_refuse_ns(80_000);
            p.note_width_loss_ns(20_000);
        }
        for _ in 0..2 {
            p.note_reexec_ns(4_000);
        }
        let addr = Address::repeat_byte(0x77);
        assert!(
            !p.choose_ordered(CohortKind::EmptyTo, addr, 8, true, 0.10),
            "ĉ_A1+δ >= ĉ_A0 must demote an unproven cohort"
        );
    }

    #[test]
    fn probe_star_releases_when_commute_absorbs() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(!p.should_release_probe_star(), "cold block keeps Detect");
        for _ in 0..8 {
            p.note_commute_skip();
        }
        assert!(
            p.should_release_probe_star(),
            "commute-absorbed EmptyTo with reexec_ns=0 demotes leftover waiters"
        );
    }

    #[test]
    fn cost_ev_keeps_proven_contract_spine() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0x20);
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        for _ in 0..8 {
            p.note_refuse_ns(80_000);
        }
        assert!(
            p.choose_ordered(CohortKind::EmptyTo, addr, 16, true, 0.20),
            "full-shell proven effective WAW stays A1 (do not drop 0x209c class)"
        );
    }

    #[test]
    fn thin_prepaid_loss_decays_prior() {
        let p = CostPolicy::new();
        p.begin_block(176);
        let addr = Address::repeat_byte(0x20);
        p.note_pair_measured_ev(CohortKind::EmptyTo, addr, 80_000.0, 8_000.0);
        assert!(p.choose_ordered(CohortKind::EmptyTo, addr, 16, true, 0.20));
        p.note_refuse_ns(50_000);
        p.note_width_loss_ns(10_000);
        for _ in 0..4 {
            p.bump_unfenced_reexec();
        }
        // abort_cf = 0 and unfenced train → prepaid loses
        p.end_block_learn();
        p.begin_block(176);
        p.note_refuse_ns(50_000);
        for _ in 0..4 {
            p.bump_unfenced_reexec();
        }
        p.end_block_learn();
        let r = p.take_report(0.0, 0);
        assert!(
            r.prior_decay >= 1,
            "L3: two prepaid-losing blocks with unfenced must decay prior: {r:?}"
        );
        assert!(r.prepaid_ns > r.abort_cf_ns);
    }

    #[test]
    fn successful_detect_prepaid_does_not_decay() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.note_refuse_ns(80_000);
        p.note_width_loss_ns(1_000_000);
        // unfenced=0: Detect succeeded — Instant idle must not teach Win_1.
        p.end_block_learn();
        let r = p.take_report(0.0, 1_000_000);
        assert_eq!(
            r.prior_decay, 0,
            "S2: prepaid with unfenced=0 must not decay: {r:?}"
        );
        assert_eq!(
            r.prepaid_ns, 80_000,
            "S2: prepaid is wall-clock refuse, not idle Instant"
        );
    }

    #[test]
    fn thin_cold_should_not_seed_a1() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(
            !p.should_seed_thin_ordered(),
            "L1: thin cold start must not seed A1"
        );
        p.promote_short_edge(0x32be, 80_000);
        assert!(
            p.should_seed_thin_ordered(),
            "L1: measured expensive abort may seed a short edge"
        );
    }

    #[test]
    fn thin_zero_ns_promote_uses_abort_hat() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 0);
        assert!(
            p.is_promoted(0x32be),
            "L1: promote_short_edge(ℓ, 0) must set measured and gate when abort hat wins"
        );
        p.promote_short_edge(0x32be, 80_000);
        assert!(
            p.is_promoted(0x32be),
            "thin: measured expensive abort must keep the short edge"
        );
    }

    #[test]
    fn thin_write_set_gates_known_successors() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(
            p.should_gate_short_after_write(0xedba, false, 2),
            "C2: first effective write with ≥2 later hint txs must promote"
        );
        assert!(
            p.should_gate_short_after_write(0x32be, true, 0),
            "C1: earlier writer on ℓ must raise the immediate successor"
        );
        assert!(
            !p.should_gate_short_after_write(0xabc, false, 0),
            "unique writer without a successor stays A0"
        );
    }

    #[test]
    fn reuse_promoted_pair_survives_next_begin() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        p.note_short_pair(0x32be, 4, 31);
        p.end_block_learn();
        p.begin_block(176);
        assert!(
            p.should_seed_thin_ordered(),
            "L4: reuse must raise short-edge A1"
        );
        let pairs = p.promoted_short_pairs();
        assert!(
            pairs
                .iter()
                .any(|&(loc, pred, succ)| loc == 0x32be && pred == 4 && succ == 31),
            "L4: stored 4→31 pair must survive begin: {pairs:?}"
        );
    }

    #[test]
    fn end_block_persists_consecutive_writer_order() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        p.note_promoted_writer_orders(&[(0x32be, vec![4, 31, 66, 67]), (0xabc, vec![1, 2])]);
        p.end_block_learn();
        p.begin_block(176);
        let pairs = p.promoted_short_pairs();
        assert!(
            pairs
                .iter()
                .any(|&(l, a, b)| l == 0x32be && a == 4 && b == 31)
                && pairs
                    .iter()
                    .any(|&(l, a, b)| l == 0x32be && a == 31 && b == 66)
                && pairs
                    .iter()
                    .any(|&(l, a, b)| l == 0x32be && a == 66 && b == 67),
            "post-publish D1 must persist the full main-chain short edges: {pairs:?}"
        );
        assert!(
            !pairs.iter().any(|&(l, _, _)| l == 0xabc),
            "unpromoted ERC-20 slots must not enter short_chain"
        );
    }

    #[test]
    fn hops_to_plant_windows_long_spine_keeps_storage() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.promote_short_edge(0xedba, 20_000);
        p.note_short_pair(0xedba, 14, 16);
        p.note_short_pair(0xedba, 16, 17);
        let first = p.loc_strategy(0x32be, 4);
        assert!(
            first == LocStrategy::Windowed1 || first == LocStrategy::Windowed2,
            "CC-L3/L4: long thin spine plants Win_w, not FullChain: {first:?}"
        );
        assert!(
            p.hops_to_plant(0x32be, 4) < 4,
            "CC-L3/L4: long thin spine must not FullChain"
        );
        assert_eq!(
            p.hops_to_plant(0xedba, 2),
            2,
            "CC-L5: storage trio stays FullChain"
        );
        assert!(
            p.spine_ordered_loses(0x32be, 16),
            "O2: 16-writer prepaid must lose to one abort sample"
        );
        assert!(
            !p.should_gate_short_after_write(0x32be, true, 0),
            "CC-L1: write-set must not mid-execute gate a long spine"
        );
        assert!(
            p.should_gate_short_after_write(0xedba, true, 0),
            "CC-L5: storage write-set still raises the short edge"
        );
    }

    #[test]
    fn prepaid_loss_demotes_long_spine_not_storage() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.promote_short_edge(0xedba, 20_000);
        p.note_short_pair(0xedba, 14, 16);
        p.note_short_pair(0xedba, 16, 17);
        p.note_refuse_ns(80_000);
        p.note_width_loss_ns(20_000);
        for _ in 0..4 {
            p.bump_unfenced_reexec();
        }
        // abort_cf = 0 and unfenced train → prepaid loses
        p.end_block_learn();
        p.begin_block(176);
        assert!(
            p.loc_demoted(0x32be),
            "F4: long spine FullChain-demotes after prepaid lose"
        );
        assert!(
            p.hops_to_plant(0x32be, 4) < 4 && p.loc_strategy(0x32be, 4) != LocStrategy::FullChain,
            "F4: demote bans FullChain; Win_w / Seg may still plant a window"
        );
        assert!(
            !p.loc_demoted(0xedba),
            "CC-L5: storage trio must not demote with the long spine"
        );
        assert_eq!(p.hops_to_plant(0xedba, 2), 2);
    }

    #[test]
    fn three_way_ev_prefers_windowed_after_measured_abort() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        assert_eq!(p.loc_strategy(0x32be, 3), LocStrategy::Windowed1);
        p.note_hops_decision(0x32be, 3);
        p.note_reexec_ns_at(Some(0x32be), 40_000);
        p.end_block_learn();
        p.begin_block(176);
        let reuse = p.loc_strategy(0x32be, 3);
        assert!(
            reuse.is_ordered() && reuse != LocStrategy::FullChain,
            "F2/F7: reuse stays OrderedAdmit (Win_w or Seg), not Full: {reuse:?}"
        );
    }

    #[test]
    fn f7_leftover_prefers_win2_first() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [
            (4, 31),
            (31, 66),
            (66, 67),
            (67, 69),
            (69, 70),
            (70, 93),
            (93, 96),
            (96, 103),
        ] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.note_hops_decision(0x32be, 8);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.end_block_learn();
        p.begin_block(176);
        assert_eq!(
            p.loc_strategy(0x32be, 8),
            LocStrategy::Windowed2,
            "S3: first leftover escalate is Win_2, not Win_3"
        );
    }

    #[test]
    fn f7_escalates_win1_leftover_to_win3_or_seg() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [
            (4, 31),
            (31, 66),
            (66, 67),
            (67, 69),
            (69, 70),
            (70, 93),
            (93, 96),
            (96, 103),
        ] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.note_hops_decision(0x32be, 8);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.end_block_learn();
        p.begin_block(176);
        let s = p.loc_strategy(0x32be, 8);
        assert!(
            matches!(
                s,
                LocStrategy::Windowed2 | LocStrategy::Windowed3 | LocStrategy::Segmented
            ),
            "F7: leftover tail after Opt/Win_1 must escalate, got {s:?}"
        );
        assert_ne!(
            s,
            LocStrategy::FullChain,
            "F7: Full stays banned on long spine"
        );
        let hops = p.hops_to_plant(0x32be, 8);
        assert!(
            hops >= 2 && hops < 8,
            "T1/T2: escalate plants a window or segments, not full-spine ({hops})"
        );
    }

    #[test]
    fn segmented_pairs_keep_intra_segment_gaps() {
        let pairs = [
            (4, 31),
            (31, 66),
            (66, 67),
            (67, 69),
            (69, 70),
            (70, 93),
            (93, 96),
        ];
        let got = CostPolicy::select_pairs_for_strategy(LocStrategy::Segmented, &pairs);
        assert!(
            got.contains(&(4, 31)) && got.contains(&(31, 66)) && got.contains(&(66, 67)),
            "T2: first segment is fully ordered: {got:?}"
        );
        assert!(
            !got.contains(&(67, 69)),
            "T2: segment boundary stays OptimisticRead: {got:?}"
        );
        assert!(
            got.contains(&(69, 70)),
            "T2: next segment still plants intra hops: {got:?}"
        );
    }
}
