//! Cost-aware admit policy — one per-ℓ arm mouth + wall-consequence learn.
//!
//! Soft=0 actions: **OptimisticRead** vs **OrderedAdmit** (refuse +
//! wave-admit pred). OptimisticRead is the OCC-effect path on this spine —
//! not a hand-off to a second OCC runtime. Beta / morph / leftover counts
//! are **features**, not `decide()` authority.
//!
//! Begin plant / hops / short-edge hints read **only** `select_arm(ℓ)`:
//! Opt | OrderedWindow(w) | Seg(seg_len) | Full (short only) | DeferPlant.
//! Candidates are **generated** from the posterior (`w*`, `s*`, `w±1`
//! neighborhood) — not a frozen 7-slot enum. Cold explores; hot is greedy
//! min-ĉ. Reward is −(gate_stall_wall + reexec_ns + measurable shell).
//! Instant idle never enters ĉ. Long spines never FullChain. Never
//! mid-execute ReadyEdge.
//!
//! Systematic reexec on an Opt/Defer leftover (multi-incarnation / high
//! `reexec_ns` / unfenced train) is a **CC feedback signal**: the next
//! begin re-opens a **light** covering OrderedWindow/Seg — the minimal
//! `w` / Seg that absorbs the train. T3 slides leftover idle hops only
//! while the loc still leaks; proven cover leaves leftover OCC (S1).
//! Leftover is not “OCC forever,” and covering is not a default
//! `w = n_pairs−1` nail. A leftover train on a hat-width window grows
//! `w_need` (U) up to a cores-scaled train hat — still not full-spine.
//! ĉ is overlap-aware (Detect makespan, not hops×stall) vs OCC
//! whole-spine; a prefix shorter than `w_need` plus OCC tail is
//! double-pay. Systematic reexec must not nail Opt (L4). Independent
//! holes stay ungated (S1).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use rustc_hash::FxBuildHasher;

use crate::{MemoryLocationHash, TxIdx};

use super::collateral::ConflictClass;

/// Thin-shell plant-location **safety cap** (not “always exactly 3”).
///
/// O8: `META_FLOOR_NS` / `SERIAL_NS_PER_TX` used to classify thin via a
/// serial-hat vs 400µs floor. That nail distorted EV (176·2µs=352µs always
/// thin). Thin is now `n ≤ THIN_N_MAX` — a structural safety bound, not ĉ.
pub(crate) const THIN_ORDERED_K: usize = 3;
/// Short-chain Full safety bound. Long spines never FullChain.
pub(crate) const ORDER_WINDOW_K: usize = 2;
/// Fat-block n (S-lazy / S-mixed). Soft-cap begin holes; skip EmptyTo plant.
pub(crate) const FAT_N: usize = 512;
/// Unknown long chain on a fat block with no EffectiveWAW → treat as lazy.
const LAZY_CHAIN_PAIR_FLOOR: usize = 32;
/// Runaway-plant hat (safety, not a strategy nail like WINDOWED_W_MAX=3).
const WINDOW_SAFETY_HAT: usize = 32;
/// Seg length hat (safety, not SEG_TX=4).
const SEG_LEN_SAFETY_HAT: usize = 16;
/// Planted-segment hat (safety; online `seg_cap` is the effective bound).
const SEG_CAP_SAFETY_HAT: usize = 4;
/// Samples on the last arm before a loc can go hot (E1).
const HOT_ARM_N: f64 = 3.0;
/// Blocks observed before a loc can go hot (E1).
const HOT_LOC_N: u32 = 3;
/// Relative SE above this stays cold (ĉ confidence still wide).
const HOT_REL_SE: f64 = 0.45;
const THIN_N_MAX: usize = 256;
/// ns-EV hysteresis for cohort choose() (not the loc arm mouth).
const NS_DELTA: f64 = 2_000.0;
/// Cohort / snap cold priors (learnable; not a loc-arm nail).
const PRIOR_C_ORDERED_NS: f64 = 8_000.0;
const PRIOR_C_OPT_NS: f64 = 25_000.0;
const PRIOR_C_ORDERED_THIN_NS: f64 = 18_000.0;
const PRIOR_C_OPT_THIN_NS: f64 = 6_000.0;
/// Wide loc-arm prior — UCB / samples walk this; not a frozen 8k nail.
const PRIOR_C_WIDE_NS: f64 = 12_000.0;
/// Long-spine cold: Opt slightly cheaper so first begin stays overlap.
const PRIOR_C_OPT_COLD_NS: f64 = 9_000.0;
/// Short-chain cold: Full slightly cheaper (storage 14→16→17 class).
const PRIOR_C_FULL_SHORT_NS: f64 = 8_000.0;
/// L6: α = 1/(n + LEARN_OFFSET). Not a global EMA_ALPHA.
const LEARN_OFFSET: f64 = 2.0;
/// Process-level prior decay only after a confident prepaid+unfenced loss.
const PREPAID_LOSE_CONF: u32 = 3;
/// Default cores when `begin_block` is used without an explicit concurrency level.
const DEFAULT_CORES: usize = 8;
/// Telemetry census bins (Opt / Win1 / Win2 / Win≥3 / Seg / Full / Defer).
const CENSUS_N: usize = 7;

/// Soft=0 action on a candidate edge / cohort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmitAction {
    /// OptimisticRead — execute now; conflict pays reexec.
    OptimisticRead,
    /// OrderedAdmit — refuse while pred unfinished; steal independent work.
    OrderedAdmit,
}

/// Per-ℓ arm (G1–G3): Opt | OrderedWindow(w) | Seg(seg_len) | Full(short) | Defer.
///
/// `w` / `seg_len` are structural parameters, not a frozen {Win_1,2,3} / SEG_TX=4
/// table. The generator proposes `w*` / `s*` from the posterior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LocStrategy {
    OptimisticRead,
    /// Prefix window of `w` consecutive hops (`w ∈ [1, w_cap(ℓ)]`).
    OrderedWindow {
        w: u8,
    },
    /// Intra-segment hops of `seg_len` writers; inter-segment OptimisticRead.
    Segmented {
        seg_len: u8,
    },
    FullChain,
    /// Do not begin-plant; rely on Resolve. Independent of leftover ifs.
    DeferPlant,
}

impl LocStrategy {
    #[inline]
    pub(crate) const fn win(w: usize) -> Self {
        let w = if w == 0 {
            1
        } else if w > 255 {
            255
        } else {
            w
        };
        Self::OrderedWindow { w: w as u8 }
    }

    #[inline]
    pub(crate) const fn seg(seg_len: usize) -> Self {
        let s = if seg_len < 2 {
            2
        } else if seg_len > 255 {
            255
        } else {
            seg_len
        };
        Self::Segmented { seg_len: s as u8 }
    }

    fn census_bin(self) -> usize {
        match self {
            Self::OptimisticRead => 0,
            Self::OrderedWindow { w: 1 } => 1,
            Self::OrderedWindow { w: 2 } => 2,
            Self::OrderedWindow { .. } => 3,
            Self::Segmented { .. } => 4,
            Self::FullChain => 5,
            Self::DeferPlant => 6,
        }
    }

    /// Tie-break: fewer prepaid hops first (ĉ-equal Win_1 beats Win_3).
    const fn tie_key(self) -> u16 {
        match self {
            Self::OptimisticRead => 0,
            Self::DeferPlant => 1,
            Self::OrderedWindow { w } => 10 + w as u16,
            Self::Segmented { seg_len } => 300 + seg_len as u16,
            Self::FullChain => 500,
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            Self::OptimisticRead => "Opt".to_string(),
            Self::OrderedWindow { w } => format!("Win_{w}"),
            Self::Segmented { seg_len } => format!("Seg_{seg_len}"),
            Self::FullChain => "Full".to_string(),
            Self::DeferPlant => "Defer".to_string(),
        }
    }

    pub(crate) const fn window_w(self) -> usize {
        match self {
            Self::OrderedWindow { w } => w as usize,
            Self::Segmented { seg_len } => (seg_len as usize).saturating_sub(1),
            Self::FullChain | Self::OptimisticRead | Self::DeferPlant => 0,
        }
    }

    pub(crate) const fn seg_len(self) -> usize {
        match self {
            Self::Segmented { seg_len } => seg_len as usize,
            _ => 0,
        }
    }

    pub(crate) const fn is_ordered(self) -> bool {
        !matches!(self, Self::OptimisticRead | Self::DeferPlant)
    }

    /// E3: Seg / Full / wide windows are high-prepaid — only cold or crisis.
    const fn is_high_prepaid(self, n_pairs: usize) -> bool {
        match self {
            Self::FullChain => n_pairs > 1,
            Self::Segmented { .. } => true,
            Self::OrderedWindow { w } => (w as usize) > 2 && n_pairs > ORDER_WINDOW_K,
            _ => false,
        }
    }
}

/// CC object class of a location (C1/C2). Lazy writer chains are never
/// OrderedAdmit objects; real Basic / storage keep light-cover reexec→CC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum LocObject {
    Unknown = 0,
    /// `basic_lazy` / LazyRecipient / LazySender writer chain.
    Lazy = 1,
    /// Real Basic WAW (Data, not lazy-accumulate).
    Basic = 2,
    /// Storage / code_hash.
    Storage = 3,
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
    /// Locations that chose DeferPlant (no begin plant).
    pub defer_locs: usize,
    /// Dominant learned action label (`Win_2` / `Seg_5` / `Defer` / `Full` / `Opt`).
    pub chosen_strategy: String,
    /// T1 window width of the dominant OrderedWindow (0 if not windowed).
    pub chosen_win_w: u8,
    /// Online `w_cap(ℓ)` of the telemetry loc (G1 evidence).
    pub chosen_w_cap: u8,
    /// Posterior `seg_len*` of the telemetry loc (0 if Seg unused; G2).
    pub chosen_seg_len: u8,
    /// Distinct OrderedWindow `w` values selected this block.
    pub unique_win_w: u8,
    /// Distinct Seg `seg_len` values selected this block.
    pub unique_seg_len: u8,
    /// E2: remaining hot-ℓ explore slots at end-block (0 = all hot exploited).
    pub explore_budget: u8,
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
    /// L7: ℓ whose selected arm ≠ last block (reuse adaptivity).
    pub arm_switch_n: usize,
    /// L7: selections that were UCB-explore, not greedy min-ĉ.
    pub explore_n: usize,
    /// L7: long-spine picks that were not PR27-hard Win_2.
    pub win2_deviate_n: usize,
    /// L7: mean ĉ of the dominant long-spine / first measured ℓ.
    pub bandit_c_opt: f64,
    /// Win_1 ĉ of the telemetry loc.
    pub bandit_c_win1: f64,
    /// Win_2 ĉ of the telemetry loc.
    pub bandit_c_win2: f64,
    /// Win_3 ĉ of the telemetry loc.
    pub bandit_c_win3: f64,
    /// Seg ĉ of the telemetry loc.
    pub bandit_c_seg: f64,
    /// Full ĉ of the telemetry loc.
    pub bandit_c_full: f64,
    /// DeferPlant ĉ of the telemetry loc.
    pub bandit_c_defer: f64,
    /// L7: `loc:arm` census (proves arms move across reuse).
    pub selected_arms: String,
    /// O1: locations that paid Detect prefix + leftover OCC in the same block.
    pub double_pay_n: usize,
    /// R1: locations whose Opt/Defer leftover paid a systematic reexec train.
    pub sys_reexec_n: usize,
    /// L1: long-spine locations whose chosen arm meets light `w_need`.
    pub covering_n: usize,
    /// L1: telemetry loc `w_need` (minimal cover; 0 = unset).
    pub chosen_w_need: u8,
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

/// Sparse per-arm ĉ / n (G3: not a fixed ARM_N=7 table).
#[derive(Debug, Clone, Copy)]
struct ArmStat {
    arm: LocStrategy,
    c: f64,
    n: f64,
}

/// Migratable morph bucket (G4): n_pairs band + long vs short + lazy object.
/// Bayes/morph are **features** that seed priors — not a second decide() mouth.
/// L1: lazy / near_independent heads share a bucket that never carries Win.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MorphKey {
    band: u8,
    long: bool,
    lazy: bool,
}

#[derive(Debug, Clone, Copy)]
struct MorphPrior {
    w_star: u8,
    seg_star: u8,
    c_opt: f64,
    n_opt: f64,
    c_win: f64,
    n_win: f64,
    c_seg: f64,
    n_seg: f64,
    c_defer: f64,
    n_defer: f64,
    c_full: f64,
    n_full: f64,
    seen: u32,
}

impl MorphPrior {
    fn new() -> Self {
        Self {
            w_star: 1,
            seg_star: 0,
            c_opt: PRIOR_C_WIDE_NS,
            n_opt: 1.0,
            c_win: PRIOR_C_WIDE_NS,
            n_win: 1.0,
            c_seg: PRIOR_C_WIDE_NS,
            n_seg: 1.0,
            c_defer: PRIOR_C_WIDE_NS,
            n_defer: 1.0,
            c_full: PRIOR_C_WIDE_NS,
            n_full: 1.0,
            seen: 0,
        }
    }
}

/// Cross-block short-edge promote (location, not a whole lazy spine).
#[derive(Debug, Clone)]
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
    /// F3: arm chosen at last begin / plant decision.
    decision: LocStrategy,
    /// Previous-block arm (L7 switch telemetry).
    prev_decision: LocStrategy,
    /// Per-arm sample counts (L6 α = 1/(n+c)).
    samples: u32,
    /// Generated-arm stats (Opt / Defer / Full / tried w / tried s).
    arms: Vec<ArmStat>,
    /// Posterior best window (G1). 0 = unset (cold starts at 1).
    w_star: u8,
    /// Posterior best seg_len (G2). 0 = unset.
    seg_star: u8,
    /// E1: leftover / unfenced crisis — allow high-prepaid again.
    last_crisis: bool,
    /// O1: ordered prefix + leftover OCC train (half-window). Not a climb to
    /// another half-window. Re-opens a **covering** ordered arm; ĉ may still
    /// trial Defer/Opt. Persists until covering succeeds or sys-reexec upgrades.
    last_double_pay: bool,
    /// R1: Opt/Defer (or leftover) paid a systematic reexec train. Next begin
    /// must re-open OrderedWindow/Seg — not nail Opt/Defer-only forever.
    last_sys_reexec: bool,
    /// L1/R3: last light covering arm absorbed the systematic train.
    last_cover_ok: bool,
    /// L1: minimal `w` that absorbs the last systematic reexec / leftover
    /// train. 0 = unset (cold uses `light_cover_w`, never `n_pairs−1`).
    w_need: u8,
    /// C1/C2: lazy vs real Basic/storage. Sticky once observed.
    object: LocObject,
    /// True after an EffectiveWAW / non-lazy Data write on this ℓ.
    saw_effective: bool,
    /// G4 morph bucket this loc last contributed to.
    morph: MorphKey,
    /// Telemetry only — leftover hops after the chosen arm (not a pick driver).
    leftover_reexec: u32,
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
            prev_decision: LocStrategy::OptimisticRead,
            samples: 0,
            arms: Vec::new(),
            w_star: 1,
            seg_star: 0,
            last_crisis: false,
            last_double_pay: false,
            last_sys_reexec: false,
            last_cover_ok: false,
            w_need: 0,
            object: LocObject::Unknown,
            saw_effective: false,
            morph: morph_key(0, false),
            leftover_reexec: 0,
            block_reexec_ns: 0,
            block_ordered_ns: 0,
            block_reexec_n: 0,
            block_ordered_n: 0,
        }
    }

    fn stat(&self, arm: LocStrategy) -> Option<(f64, f64)> {
        self.arms.iter().find(|s| s.arm == arm).map(|s| (s.c, s.n))
    }

    fn upsert_stat(&mut self, arm: LocStrategy, c: f64, n: f64) {
        if let Some(s) = self.arms.iter_mut().find(|s| s.arm == arm) {
            s.c = c.max(1.0);
            s.n = n.max(1.0);
        } else {
            self.arms.push(ArmStat {
                arm,
                c: c.max(1.0),
                n: n.max(1.0),
            });
        }
    }

    fn bump_c_floor(&mut self, arm: LocStrategy, floor: f64) {
        if let Some(s) = self.arms.iter_mut().find(|s| s.arm == arm) {
            s.c = s.c.max(floor).max(1.0);
        } else {
            self.arms.push(ArmStat {
                arm,
                c: floor.max(1.0),
                n: 1.0,
            });
        }
    }

    fn update_arm(&mut self, arm: LocStrategy, x: f64) {
        if let Some(s) = self.arms.iter_mut().find(|s| s.arm == arm) {
            let alpha = learn_alpha(s.n);
            s.c = (1.0 - alpha) * s.c + alpha * x.clamp(1.0, 10_000_000.0);
            s.n += 1.0;
        } else {
            self.arms.push(ArmStat {
                arm,
                c: x.clamp(1.0, 10_000_000.0),
                n: 2.0,
            });
        }
        self.refresh_stars();
    }

    fn refresh_stars(&mut self) {
        let mut best_w: Option<(u8, f64)> = None;
        let mut best_s: Option<(u8, f64)> = None;
        for s in &self.arms {
            if s.n <= 1.0 {
                continue;
            }
            match s.arm {
                LocStrategy::OrderedWindow { w } => {
                    if best_w.map(|(_, c)| s.c + 1e-9 < c).unwrap_or(true) {
                        best_w = Some((w, s.c));
                    }
                }
                LocStrategy::Segmented { seg_len } => {
                    if best_s.map(|(_, c)| s.c + 1e-9 < c).unwrap_or(true) {
                        best_s = Some((seg_len, s.c));
                    }
                }
                _ => {}
            }
        }
        if let Some((w, _)) = best_w {
            self.w_star = w;
        } else if let LocStrategy::OrderedWindow { w } = self.decision {
            self.w_star = w;
        }
        if let Some((s, _)) = best_s {
            self.seg_star = s;
        } else if let LocStrategy::Segmented { seg_len } = self.decision {
            self.seg_star = seg_len;
        }
    }

    fn seed_from_morph(&mut self, prior: &MorphPrior, n_pairs: usize) {
        let lazy = self.object == LocObject::Lazy;
        self.morph = morph_key(n_pairs, lazy);
        if prior.seen == 0 {
            return;
        }
        if prior.n_opt > 1.0 {
            self.upsert_stat(LocStrategy::OptimisticRead, prior.c_opt, prior.n_opt);
        }
        if prior.n_defer > 1.0 {
            self.upsert_stat(LocStrategy::DeferPlant, prior.c_defer, prior.n_defer);
        }
        // L1: lazy / near_independent never inherit Win/Full/Seg.
        if lazy {
            return;
        }
        if prior.w_star >= 1 {
            self.w_star = prior.w_star;
        }
        if prior.seg_star >= 2 {
            self.seg_star = prior.seg_star;
        }
        if prior.n_full > 1.0 {
            self.upsert_stat(LocStrategy::FullChain, prior.c_full, prior.n_full);
        }
        if prior.n_win > 1.0 && self.w_star >= 1 {
            self.upsert_stat(
                LocStrategy::win(self.w_star as usize),
                prior.c_win,
                prior.n_win,
            );
        }
        if prior.n_seg > 1.0 && self.seg_star >= 2 {
            self.upsert_stat(
                LocStrategy::seg(self.seg_star as usize),
                prior.c_seg,
                prior.n_seg,
            );
        }
    }
}

fn learn_alpha(n: f64) -> f64 {
    1.0 / (n.max(0.0) + LEARN_OFFSET)
}

fn morph_band(n_pairs: usize) -> u8 {
    match n_pairs {
        0 => 0,
        1 => 1,
        2 => 2,
        3..=4 => 3,
        5..=8 => 4,
        _ => 5,
    }
}

fn morph_key(n_pairs: usize, lazy: bool) -> MorphKey {
    MorphKey {
        band: morph_band(n_pairs),
        long: n_pairs > ORDER_WINDOW_K,
        lazy,
    }
}

fn arm_prior(arm: LocStrategy, n_pairs: usize) -> f64 {
    match arm {
        LocStrategy::OptimisticRead | LocStrategy::DeferPlant if n_pairs > ORDER_WINDOW_K => {
            PRIOR_C_OPT_COLD_NS
        }
        LocStrategy::FullChain if n_pairs > 0 && n_pairs <= ORDER_WINDOW_K => PRIOR_C_FULL_SHORT_NS,
        _ => PRIOR_C_WIDE_NS,
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
    /// Lock-free empty check so pick does not mutex when the queue is dry.
    pending_idle_n: AtomicUsize,
    /// L1: committed arm for this block (select once; hops/hint share it).
    block_arm: DashMap<MemoryLocationHash, LocStrategy, FxBuildHasher>,
    /// G4: morph-bucket priors shared across new ℓs (features only).
    morphs: DashMap<MorphKey, MorphPrior, FxBuildHasher>,
    /// C1: observed object class (lazy vs Basic/storage), even before promote.
    loc_object: DashMap<MemoryLocationHash, LocObject, FxBuildHasher>,
    /// P3: this process has seen a lazy writer chain (skip fat end_block walks).
    lazy_seen: AtomicBool,
    /// Process-persistent begin count (reuse iters). Not tx-count `block_n`.
    block_seq: AtomicUsize,
    arm_switch_n: AtomicUsize,
    explore_n: AtomicUsize,
    win2_deviate_n: AtomicUsize,
    /// Online core count (E2 / w_cap / seg_cap). Not a frozen strategy.
    cores: AtomicUsize,
    /// E2: remaining hot-ℓ explore slots this block.
    explore_budget_left: AtomicUsize,
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
            pending_idle_n: AtomicUsize::new(0),
            block_arm: DashMap::default(),
            morphs: DashMap::default(),
            loc_object: DashMap::default(),
            lazy_seen: AtomicBool::new(false),
            block_seq: AtomicUsize::new(0),
            arm_switch_n: AtomicUsize::new(0),
            explore_n: AtomicUsize::new(0),
            win2_deviate_n: AtomicUsize::new(0),
            cores: AtomicUsize::new(DEFAULT_CORES),
            explore_budget_left: AtomicUsize::new(0),
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
        self.pending_idle_n.store(0, Ordering::Relaxed);
        self.block_arm.clear();
        self.morphs.clear();
        self.loc_object.clear();
        self.lazy_seen.store(false, Ordering::Relaxed);
        self.prepaid_lose_streak.store(0, Ordering::Relaxed);
        self.last_block_n.store(0, Ordering::Relaxed);
        self.block_seq.store(0, Ordering::Relaxed);
        self.cores.store(DEFAULT_CORES, Ordering::Relaxed);
        self.explore_budget_left.store(0, Ordering::Relaxed);
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
        self.pending_idle_n.store(0, Ordering::Relaxed);
        self.block_arm.clear();
        self.arm_switch_n.store(0, Ordering::Relaxed);
        self.explore_n.store(0, Ordering::Relaxed);
        self.win2_deviate_n.store(0, Ordering::Relaxed);
        self.explore_budget_left.store(0, Ordering::Relaxed);
        // P3 is per-block: a prior lazy block must not lean-end a later real spine.
        self.lazy_seen.store(false, Ordering::Relaxed);
    }

    pub(crate) fn begin_block(&self, n: usize) {
        self.begin_block_with_cores(n, DEFAULT_CORES);
    }

    pub(crate) fn begin_block_with_cores(&self, n: usize, cores: usize) {
        self.cores.store(cores.max(1), Ordering::Relaxed);
        let prev = self.block_n.load(Ordering::Relaxed);
        if prev > 0 {
            self.last_block_n.store(prev, Ordering::Relaxed);
        }
        self.block_n.store(n, Ordering::Relaxed);
        self.block_seq.fetch_add(1, Ordering::Relaxed);
        self.reset_block_counters();
        // O8: thin is a structural n-cap, not META_FLOOR × SERIAL_NS EV.
        let thin = n > 0 && n <= THIN_N_MAX;
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
        self.explore_budget_left
            .store(self.explore_budget(), Ordering::Relaxed);
        for mut e in self.promoted.iter_mut() {
            e.prev_decision = e.decision;
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

    #[inline]
    pub(crate) fn cores(&self) -> usize {
        self.cores.load(Ordering::Relaxed).max(1)
    }

    /// C1: `basic_lazy` (and equivalent lazy writer chains) never OrderedAdmit.
    /// Near-independent fat heads with no EffectiveWAW are the same object.
    ///
    /// Reads `loc_object` first (safe under `promoted.entry`). Promoted is
    /// only peeked when the map is Unknown — never from `new_promoted_seeded`.
    pub(crate) fn loc_forbids_ordered(&self, location: MemoryLocationHash) -> bool {
        match self.loc_object_map(location) {
            LocObject::Lazy => true,
            LocObject::Basic | LocObject::Storage => false,
            LocObject::Unknown => {
                let n_pairs = self
                    .short_chain
                    .get(&location)
                    .map(|c| c.len())
                    .unwrap_or(0);
                self.unknown_looks_lazy(n_pairs, self.promoted_saw_effective(location))
            }
        }
    }

    fn loc_object_map(&self, location: MemoryLocationHash) -> LocObject {
        self.loc_object
            .get(&location)
            .map(|o| *o)
            .unwrap_or(LocObject::Unknown)
    }

    fn unknown_looks_lazy(&self, n_pairs: usize, saw_effective: bool) -> bool {
        self.block_n() >= FAT_N && n_pairs >= LAZY_CHAIN_PAIR_FLOOR && !saw_effective
    }

    fn promoted_saw_effective(&self, location: MemoryLocationHash) -> bool {
        self.promoted.get(&location).is_some_and(|s| {
            s.saw_effective || matches!(s.object, LocObject::Basic | LocObject::Storage)
        })
    }

    pub(crate) fn loc_object(&self, location: MemoryLocationHash) -> LocObject {
        let mapped = self.loc_object_map(location);
        if mapped != LocObject::Unknown {
            return mapped;
        }
        self.promoted
            .get(&location)
            .map(|e| e.object)
            .unwrap_or(LocObject::Unknown)
    }

    /// C1/C2: record write-set / conflict object. Lazy never upgrades a
    /// Basic/storage spine; effective never downgrades to lazy.
    pub(crate) fn note_loc_write(&self, location: MemoryLocationHash, lazy: bool) {
        let next = if lazy {
            LocObject::Lazy
        } else {
            LocObject::Basic
        };
        if lazy {
            self.lazy_seen.store(true, Ordering::Relaxed);
        }
        let cur = self
            .loc_object
            .get(&location)
            .map(|o| *o)
            .unwrap_or(LocObject::Unknown);
        let keep = match (cur, next) {
            (LocObject::Basic | LocObject::Storage, LocObject::Lazy) => cur,
            (LocObject::Lazy, LocObject::Basic | LocObject::Storage) => next,
            (LocObject::Unknown, _) => next,
            (a, _) => a,
        };
        self.loc_object.insert(location, keep);
        if let Some(mut e) = self.promoted.get_mut(&location) {
            e.object = keep;
            if !lazy {
                e.saw_effective = true;
            }
        }
    }

    #[inline]
    pub(crate) fn lazy_already_seen(&self) -> bool {
        self.lazy_seen.load(Ordering::Relaxed)
    }

    /// P2: fat begin hole cap — keep a light real-spine prefix, not 95-hole prepaid.
    pub(crate) fn fat_begin_hole_cap(&self) -> usize {
        if self.block_n() < FAT_N {
            return usize::MAX;
        }
        self.cores().max(8).min(16)
    }

    /// Seed a new `PromotedLoc`. Must not touch `self.promoted` — callers
    /// hold `promoted.entry()` (write lock) and a nested get deadlocks.
    fn new_promoted_seeded(&self, location: MemoryLocationHash, n_pairs: usize) -> PromotedLoc {
        let mut loc = PromotedLoc::new();
        loc.object = self.loc_object_map(location);
        loc.saw_effective = matches!(loc.object, LocObject::Basic | LocObject::Storage);
        let lazy =
            loc.object == LocObject::Lazy || self.unknown_looks_lazy(n_pairs, loc.saw_effective);
        if let Some(p) = self.morphs.get(&morph_key(n_pairs, lazy)) {
            loc.seed_from_morph(&p, n_pairs);
        }
        loc
    }

    /// G1: online window cap. Grows with `n_pairs` / cores; never Full-spine
    /// on long chains. Not a global `WINDOWED_W_MAX=3` nail.
    pub(crate) fn w_cap_of(&self, n_pairs: usize) -> usize {
        if n_pairs == 0 {
            return 0;
        }
        let cores = self.cores();
        let n_tx = self.block_n().max(1);
        let oversub = n_tx / cores;
        // Oversubscribed blocks: smaller useful windows (protect prepaid wall).
        // 176/8≈22 → hat 2 (PR27 Win_2 class), not a frozen WINDOWED_W_MAX.
        let core_hat = if oversub >= 16 {
            2
        } else if oversub >= 8 {
            (cores / 2).max(2)
        } else {
            cores.max(2)
        };
        let full_ban = if n_pairs > ORDER_WINDOW_K {
            n_pairs.saturating_sub(1)
        } else {
            n_pairs
        };
        full_ban
            .min(core_hat)
            .min(WINDOW_SAFETY_HAT)
            .max(1)
            .min(n_pairs)
    }

    /// Per-ℓ window cap. Oversub hat stays 2 until a **light** covering
    /// arm is the CC mouth (systematic reexec / leftover double-pay /
    /// proven cover). Raises to `w_need`, never nails `n_pairs−1`.
    pub(crate) fn w_cap_for(&self, location: MemoryLocationHash, n_pairs: usize) -> usize {
        let base = self.w_cap_of(n_pairs);
        if n_pairs <= ORDER_WINDOW_K {
            return base;
        }
        let Some(s) = self.promoted.get(&location) else {
            return base;
        };
        let need = self.loc_w_need(location, n_pairs);
        let proven_cover =
            (s.w_star as usize) >= need && is_covering(s.decision, n_pairs, self.seg_cap(), need);
        if s.last_sys_reexec || s.last_double_pay || proven_cover {
            need.max(base).min(full_cover_w(n_pairs))
        } else {
            base
        }
    }

    /// L1: prepaid-safe **first** cover hat. Same shape as `w_cap_of` so
    /// 176@8 starts Win_2-class — never a default `n_pairs−1` nail.
    fn light_hat(&self, n_pairs: usize) -> usize {
        if n_pairs <= ORDER_WINDOW_K {
            return n_pairs.max(1);
        }
        self.w_cap_of(n_pairs).min(full_cover_w(n_pairs)).max(1)
    }

    /// U: leftover-train grow ceiling. Cores-scaled (8@8), fat blocks stay
    /// light (S/O). Never `n_pairs−1` by default.
    fn train_hat(&self, n_pairs: usize) -> usize {
        let light = self.light_hat(n_pairs);
        let full = full_cover_w(n_pairs);
        if n_pairs <= ORDER_WINDOW_K {
            return light;
        }
        let n = self.block_n();
        let cores = self.cores();
        let cap = if n >= 512 {
            light.max(2).min(4)
        } else {
            cores.max(4).min(8)
        };
        cap.min(full).max(light)
    }

    /// L1/U: minimal cover width for `ℓ`. Unset + reopen → light first
    /// cover. Stored `w_need` may exceed the oversub hat after a leftover
    /// train (U); still capped by `train_hat`, never a full-spine nail.
    fn loc_w_need(&self, location: MemoryLocationHash, n_pairs: usize) -> usize {
        let full = full_cover_w(n_pairs);
        let light = self.light_hat(n_pairs);
        let stored = self
            .promoted
            .get(&location)
            .map(|s| s.w_need as usize)
            .unwrap_or(0);
        if stored >= 1 {
            return stored.min(full).min(self.train_hat(n_pairs)).max(1);
        }
        if self.reopen_ordered(location) {
            let observed = self
                .promoted
                .get(&location)
                .map(|s| (s.block_reexec_n as usize).max(2))
                .unwrap_or(2);
            return light_cover_w(n_pairs, light, observed);
        }
        1.min(full.max(1))
    }

    fn reopen_ordered(&self, location: MemoryLocationHash) -> bool {
        if self.loc_forbids_ordered(location) {
            return false;
        }
        self.promoted
            .get(&location)
            .is_some_and(|s| s.last_sys_reexec || s.last_double_pay)
    }

    /// T3 leftover slide is only for a still-leaking loc. Proven cover
    /// leaves leftover OCC (S1 / O). Fat reuse with cover_ok never slides (S).
    /// A planted covering prefix (Win_2+) must not slide on the same
    /// block — 3356896 i=1 otherwise prepaid-blows ĉ and retreats to Opt.
    pub(crate) fn leftover_slide_ok(&self, location: MemoryLocationHash) -> bool {
        let Some(s) = self.promoted.get(&location) else {
            return true;
        };
        if s.last_sys_reexec || s.last_double_pay || s.last_crisis {
            return true;
        }
        if s.last_cover_ok {
            return false;
        }
        let n_pairs = self
            .short_chain
            .get(&location)
            .map(|c| c.len())
            .unwrap_or(0);
        if n_pairs > ORDER_WINDOW_K
            && is_covering(
                s.decision,
                n_pairs,
                self.seg_cap(),
                self.loc_w_need(location, n_pairs),
            )
        {
            return false;
        }
        if self.block_n() >= 512 && s.decision.is_ordered() {
            return false;
        }
        true
    }

    /// L3: a proven **light** covering arm stays sticky. Cold may walk
    /// `w_need±1`; hot does not default-widen to full cover.
    fn covering_sticky(&self, location: MemoryLocationHash, n_pairs: usize) -> bool {
        n_pairs > ORDER_WINDOW_K
            && self.promoted.get(&location).is_some_and(|s| {
                s.last_cover_ok
                    && !s.last_crisis
                    && is_covering(
                        s.decision,
                        n_pairs,
                        self.seg_cap(),
                        self.loc_w_need(location, n_pairs),
                    )
            })
    }

    /// G2: planted-segment safety bound from block width / cores (not SEG_CAP=2).
    pub(crate) fn seg_cap(&self) -> usize {
        let cores = self.cores();
        let n = self.block_n().max(1);
        let from_cores = (cores / 2).max(1);
        let from_block = (n / 64).max(1);
        from_cores.min(from_block).min(SEG_CAP_SAFETY_HAT).max(1)
    }

    /// E2: hot-ℓ explore slots this block. Oversubscribed → 0 (exploit).
    fn explore_budget(&self) -> usize {
        let n = self.block_n().max(1);
        let cores = self.cores();
        let oversub = n / cores;
        if oversub >= 16 {
            0
        } else if oversub >= 8 {
            1
        } else {
            (cores / 4).max(1)
        }
    }

    fn try_consume_explore_budget(&self) -> bool {
        let mut left = self.explore_budget_left.load(Ordering::Relaxed);
        loop {
            if left == 0 {
                return false;
            }
            match self.explore_budget_left.compare_exchange_weak(
                left,
                left - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(v) => left = v,
            }
        }
    }

    /// T1/T2: hops to plant on `ℓ` at begin (Seg returns intra-segment count).
    ///
    /// Sole mouth: `select_arm`. Full only when n_pairs ≤ `ORDER_WINDOW_K`.
    /// Long spines never FullChain (PR22 1.40ms prepaid wall — safety).
    pub(crate) fn hops_to_plant(&self, location: MemoryLocationHash, n_pairs: usize) -> usize {
        if self.loc_forbids_ordered(location) {
            return 0;
        }
        let n_pairs = self.loc_n_pairs(location, n_pairs);
        self.hops_for_arm(self.loc_strategy(location, n_pairs), n_pairs)
    }

    #[inline]
    fn hops_for_arm(&self, strategy: LocStrategy, n_pairs: usize) -> usize {
        // Safety: a short-n Full cache must not FullChain a long spine.
        if n_pairs > ORDER_WINDOW_K && strategy == LocStrategy::FullChain {
            return 0;
        }
        hops_for_strategy(strategy, n_pairs, self.seg_cap())
    }

    /// Prefer the stored chain length so a first-write `n=1` cannot
    /// commit Full and then FullChain the 16-writer Basic spine.
    fn loc_n_pairs(&self, location: MemoryLocationHash, n_pairs: usize) -> usize {
        let stored = self
            .short_chain
            .get(&location)
            .map(|c| c.len())
            .unwrap_or(0);
        n_pairs.max(stored)
    }

    /// T1/T2: which stored pairs to plant for `strategy` (uses online seg_cap).
    pub(crate) fn plant_pairs(
        &self,
        strategy: LocStrategy,
        pairs: &[(TxIdx, TxIdx)],
    ) -> Vec<(TxIdx, TxIdx)> {
        select_pairs_capped(strategy, pairs, self.seg_cap())
    }

    /// Associated helper for tests / callers without a live policy.
    pub(crate) fn select_pairs_for_strategy(
        strategy: LocStrategy,
        pairs: &[(TxIdx, TxIdx)],
    ) -> Vec<(TxIdx, TxIdx)> {
        select_pairs_capped(strategy, pairs, 2)
    }

    /// T1 window width for next-quantum hops on `ℓ`.
    pub(crate) fn window_w_of(&self, location: MemoryLocationHash, n_pairs: usize) -> usize {
        self.loc_strategy(location, n_pairs)
            .window_w()
            .min(self.w_cap_for(location, n_pairs).max(1))
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
        self.remember_arm(location, strategy);
    }

    /// Persist the committed arm so DeferPlant survives the next begin.
    pub(crate) fn remember_arm(&self, location: MemoryLocationHash, arm: LocStrategy) {
        let n_pairs = self
            .short_chain
            .get(&location)
            .map(|c| c.len())
            .unwrap_or(0);
        let arm = if n_pairs > ORDER_WINDOW_K && arm == LocStrategy::FullChain {
            LocStrategy::OptimisticRead
        } else {
            arm
        };
        self.block_arm.insert(location, arm);
        let mut e = self
            .promoted
            .entry(location)
            .or_insert_with(|| self.new_promoted_seeded(location, n_pairs));
        e.decision = arm;
        e.hits = e.hits.max(1);
        if let LocStrategy::OrderedWindow { w } = arm {
            e.w_star = w;
        }
        if let LocStrategy::Segmented { seg_len } = arm {
            e.seg_star = seg_len;
        }
    }

    /// L1: per-ℓ arm. Cached for the block so hops / hint / flush share one mouth.
    pub(crate) fn loc_strategy(&self, location: MemoryLocationHash, n_pairs: usize) -> LocStrategy {
        if self.loc_forbids_ordered(location) {
            return LocStrategy::OptimisticRead;
        }
        let n_pairs = self.loc_n_pairs(location, n_pairs);
        if n_pairs == 0 {
            return LocStrategy::OptimisticRead;
        }
        if let Some(a) = self.block_arm.get(&location) {
            let cached = *a;
            drop(a);
            if n_pairs <= ORDER_WINDOW_K || cached != LocStrategy::FullChain {
                return cached;
            }
        }
        if !self.is_promoted(location) {
            return LocStrategy::OptimisticRead;
        }
        self.commit_arm(location, n_pairs)
    }

    /// L1/L5: hint path may select before `is_promoted` (each short CallWaw ℓ).
    pub(crate) fn select_hint_arm(
        &self,
        location: MemoryLocationHash,
        n_pairs: usize,
    ) -> LocStrategy {
        if self.loc_forbids_ordered(location) {
            return LocStrategy::OptimisticRead;
        }
        let n_pairs = self.loc_n_pairs(location, n_pairs);
        if n_pairs == 0 {
            return LocStrategy::OptimisticRead;
        }
        if let Some(a) = self.block_arm.get(&location) {
            if n_pairs > ORDER_WINDOW_K && *a == LocStrategy::FullChain {
                // fall through
            } else {
                return *a;
            }
        }
        self.commit_arm(location, n_pairs)
    }

    fn commit_arm(&self, location: MemoryLocationHash, n_pairs: usize) -> LocStrategy {
        let n_pairs = self.loc_n_pairs(location, n_pairs);
        let (mut arm, explore, greedy) = self.select_arm(location, n_pairs);
        // C1/L1: lazy / near_independent never persist an ordered arm.
        if self.loc_forbids_ordered(location) && arm.is_ordered() {
            arm = LocStrategy::OptimisticRead;
        }
        // Safety: never persist FullChain on a long spine (short-n cache).
        // C3: ultra-long storage is the same ban — never Full(n_pairs).
        if n_pairs > ORDER_WINDOW_K && arm == LocStrategy::FullChain {
            arm = if self.reopen_ordered(location) {
                LocStrategy::win(self.loc_w_need(location, n_pairs))
            } else {
                LocStrategy::OptimisticRead
            };
        }
        if let Some(e) = self.promoted.get(&location) {
            if e.prev_decision != arm && self.block_seq.load(Ordering::Relaxed) > 1 {
                self.arm_switch_n.fetch_add(1, Ordering::Relaxed);
            }
        }
        // Persist even when hops=0 (Defer/Opt). Otherwise census / learn
        // keep the prior Win_w and the next begin replants the half-window.
        self.remember_arm(location, arm);
        if explore || arm != greedy {
            self.explore_n.fetch_add(1, Ordering::Relaxed);
        }
        if n_pairs > ORDER_WINDOW_K && arm != LocStrategy::win(2) {
            self.win2_deviate_n.fetch_add(1, Ordering::Relaxed);
        }
        arm
    }

    /// Sole mouth (E1–E3 + G1–G5): generate candidates from posterior, then
    /// hot → greedy min-ĉ; cold / crisis → σ(ĉ)-scaled UCB.
    fn select_arm(
        &self,
        location: MemoryLocationHash,
        n_pairs: usize,
    ) -> (LocStrategy, bool, LocStrategy) {
        self.ensure_promoted_seeded(location, n_pairs);
        let demoted = self.loc_demoted(location);
        let (hot, crisis) = self.phase_of(location);
        let reopen = self.reopen_ordered(location);
        let upgrading = self
            .promoted
            .get(&location)
            .is_some_and(|s| s.last_sys_reexec && !s.decision.is_ordered());
        // Unused Win prior must not undercut measured Opt on a leftover-long
        // spine *until* systematic reexec / leftover double-pay re-opens CC.
        let leftover_pay_once = n_pairs.saturating_sub(self.w_cap_of(n_pairs)) >= 2
            && self.promoted.get(&location).is_some_and(|s| {
                s.measured && s.samples >= 2 && !s.decision.is_ordered() && !reopen
            });
        // L3: cold / crisis explores light `w_need±1` / Seg. Hot proven
        // light cover is sticky. Sys-reexec upgrade is greedy light cover.
        let sticky = self.covering_sticky(location, n_pairs);
        // L4: a proven cover must not UCB-explore unused Opt (3356896
        // false retreat after quiet Win_2). Greedy ĉ may still yield
        // when Opt is itself measured cheaper (prepaid blowout).
        let cover_ok = self
            .promoted
            .get(&location)
            .is_some_and(|s| s.last_cover_ok);
        // L3: cold generate still offers w_need±1 / Seg; reopen itself is
        // greedy min-ĉ (no UCB inventing an unmeasured cover over Defer).
        let explore_ok = if leftover_pay_once || upgrading || sticky || reopen || cover_ok {
            false
        } else if crisis || !hot {
            true
        } else {
            self.try_consume_explore_budget()
        };
        let eligible = self.generate_arms(location, n_pairs, demoted, !hot || crisis, crisis);
        let stats = self.arm_c_n(location, n_pairs, &eligible);
        let n_tot: f64 = stats.iter().map(|&(_, _, n)| n.max(1.0)).sum();
        let sigma = explore_sigma(&stats);
        let mut greedy = eligible[0];
        let mut greedy_c = stats[0].1;
        let mut best = greedy;
        let mut best_s = explore_score(stats[0].1, stats[0].2, n_tot, sigma);
        for (i, &a) in eligible.iter().enumerate().skip(1) {
            let ca = stats[i].1;
            if ca + 1e-9 < greedy_c
                || ((ca - greedy_c).abs() <= 1e-9 && a.tie_key() < greedy.tie_key())
            {
                greedy = a;
                greedy_c = ca;
            }
            let s = explore_score(ca, stats[i].2, n_tot, sigma);
            if s + 1e-9 < best_s || ((s - best_s).abs() <= 1e-9 && a.tie_key() < best.tie_key()) {
                best = a;
                best_s = s;
            }
        }
        if !explore_ok {
            return (greedy, false, greedy);
        }
        (best, best != greedy, greedy)
    }

    fn ensure_promoted_seeded(&self, location: MemoryLocationHash, n_pairs: usize) {
        if self.promoted.contains_key(&location) {
            return;
        }
        self.promoted
            .entry(location)
            .or_insert_with(|| self.new_promoted_seeded(location, n_pairs));
    }

    fn phase_of(&self, location: MemoryLocationHash) -> (bool, bool) {
        let Some(s) = self.promoted.get(&location) else {
            return (false, false);
        };
        let crisis = s.last_crisis;
        if crisis {
            return (false, true);
        }
        let n_pairs = self
            .short_chain
            .get(&location)
            .map(|c| c.len())
            .unwrap_or(0);
        if self.covering_sticky(location, n_pairs) {
            return (true, false);
        }
        let (c, n) = s.stat(s.decision).unwrap_or((PRIOR_C_WIDE_NS, 1.0));
        if s.samples < HOT_LOC_N || n < HOT_ARM_N {
            return (false, false);
        }
        let se = c.max(1.0) / n.sqrt();
        if se / c.max(1.0) > HOT_REL_SE {
            return (false, false);
        }
        (true, false)
    }

    fn generate_arms(
        &self,
        location: MemoryLocationHash,
        n_pairs: usize,
        demoted: bool,
        cold: bool,
        crisis: bool,
    ) -> Vec<LocStrategy> {
        // C1/L1/L3: lazy / near_independent — Opt/Defer only. Cold must
        // not explore Win/Full on that ℓ. Hot sticky is no-order.
        if self.loc_forbids_ordered(location) {
            return vec![LocStrategy::OptimisticRead, LocStrategy::DeferPlant];
        }
        let mut out = vec![LocStrategy::OptimisticRead, LocStrategy::DeferPlant];
        let w_cap = self.w_cap_for(location, n_pairs);
        let reopen = self.reopen_ordered(location);
        let (w_star, s_star) = self
            .promoted
            .get(&location)
            .map(|s| {
                let w = if s.w_star >= 1 { s.w_star as usize } else { 1 };
                let sl = if s.seg_star >= 2 {
                    s.seg_star as usize
                } else {
                    default_seg_len(n_pairs, self.cores())
                };
                (w.min(w_cap).max(1), sl)
            })
            .unwrap_or((1.min(w_cap.max(1)), default_seg_len(n_pairs, self.cores())));
        let leftover_pay_once = n_pairs.saturating_sub(self.w_cap_of(n_pairs)) >= 2
            && self.promoted.get(&location).is_some_and(|s| {
                s.measured && s.samples >= 2 && !s.decision.is_ordered() && !reopen
            });
        // L1: leftover / sys-reexec re-opens **light** OrderedWindow/Seg.
        // Pin-Opt is only unused-Win-prior protection (no signal yet).
        let pin_opt = leftover_pay_once && !reopen;
        let w_need = self.loc_w_need(location, n_pairs);
        if n_pairs >= 1 && w_cap >= 1 && !pin_opt {
            if reopen {
                let w_cover = w_need.min(w_cap).max(1);
                out.push(LocStrategy::win(w_cover));
                // L3: cold explores light w±1 / Seg. Never FullChain.
                if cold || crisis {
                    if w_cover > 1 {
                        out.push(LocStrategy::win((w_cover - 1).min(n_pairs)));
                    }
                    if w_cover + 1 < n_pairs && w_cover + 1 <= w_cap {
                        out.push(LocStrategy::win(w_cover + 1));
                    }
                }
            } else {
                out.push(LocStrategy::win(w_star.min(n_pairs).min(w_cap)));
                if cold || crisis {
                    if w_star > 1 {
                        out.push(LocStrategy::win((w_star - 1).min(n_pairs)));
                    }
                    if w_star < w_cap && w_star + 1 <= n_pairs {
                        out.push(LocStrategy::win(w_star + 1));
                    }
                }
            }
        }
        // E3: Seg is high-prepaid. Covering Seg is in the CC mouth when
        // leftover re-opens; otherwise only after the window neighborhood.
        let want_seg = n_pairs >= 3
            && !pin_opt
            && (reopen || ((cold || crisis) && w_star >= w_cap && w_cap >= 3));
        if want_seg {
            let sl = if reopen {
                covering_seg_len(n_pairs, self.cores(), w_need)
            } else {
                s_star.clamp(2, (n_pairs + 1).min(SEG_LEN_SAFETY_HAT))
            };
            out.push(LocStrategy::seg(sl));
            if (cold || crisis) && sl > 2 {
                out.push(LocStrategy::seg(sl - 1));
            }
        }
        // C3: Full only on short chains. Ultra-long storage never Full(n).
        if n_pairs > 0
            && n_pairs <= ORDER_WINDOW_K
            && !demoted
            && !pin_opt
            && (cold || crisis || n_pairs <= 2)
        {
            out.push(LocStrategy::FullChain);
        }
        if !cold && !crisis {
            // E1: hot exploit is last arm / w* / measured ĉ only.
            // Unmeasured Defer/Opt priors must not undercut a working window.
            // Reopen keeps covering + Opt/Defer so ĉ can trial Defer.
            let loc = self.promoted.get(&location);
            let keep = loc.as_ref().map(|s| s.decision);
            let w_keep = if reopen {
                w_need.min(w_cap).max(1)
            } else {
                w_star.min(n_pairs).min(w_cap.max(1))
            };
            out.retain(|a| {
                keep == Some(*a)
                    || *a == LocStrategy::win(w_keep)
                    || (reopen
                        && matches!(a, LocStrategy::OptimisticRead | LocStrategy::DeferPlant))
                    || loc
                        .as_ref()
                        .and_then(|s| s.stat(*a).map(|(_, n)| n > 1.5))
                        .unwrap_or(false)
            });
        }
        // L3: proven light cover does not walk below w_need (OCC tail).
        // O/S: fat cover_ok may keep Win_1 — leftover T3 is off.
        if self.covering_sticky(location, n_pairs) {
            let keep = self.promoted.get(&location).map(|s| s.decision);
            let w_cover = w_need.min(w_cap).max(1);
            let fat = self.block_n() >= 512;
            out.retain(|a| {
                keep == Some(*a)
                    || *a == LocStrategy::win(w_cover)
                    || (fat && *a == LocStrategy::win(1))
                    || matches!(a, LocStrategy::OptimisticRead | LocStrategy::DeferPlant)
            });
        }
        // L1: never schedule a prefix shorter than w_need + OCC tail.
        if reopen {
            let seg_cap = self.seg_cap();
            out.retain(|a| {
                matches!(a, LocStrategy::OptimisticRead | LocStrategy::DeferPlant)
                    || is_covering(*a, n_pairs, seg_cap, w_need)
            });
        }
        out.sort_by_key(|a| a.tie_key());
        out.dedup();
        if out.is_empty() {
            out.push(LocStrategy::OptimisticRead);
        }
        out
    }

    fn arm_c_n(
        &self,
        location: MemoryLocationHash,
        n_pairs: usize,
        eligible: &[LocStrategy],
    ) -> Vec<(LocStrategy, f64, f64)> {
        let loc = self.promoted.get(&location);
        let measured = loc.as_ref().is_some_and(|s| s.measured);
        let abort = loc
            .as_ref()
            .map(|s| s.reexec_ns_ema.max(1.0))
            .unwrap_or(1.0);
        // Floor unused wider/high-prepaid arms to the last paid ĉ unless a
        // confidence crisis says the window is too narrow. Leftover OCC on a
        // proven window (w≥2) is expected and must not invent cheap Win_3.
        let leftover0 = loc
            .as_ref()
            .is_some_and(|s| s.decision.is_ordered() && !s.last_crisis && s.block_reexec_n < 2);
        let sys_reexec = loc.as_ref().is_some_and(|s| s.last_sys_reexec);
        let double_pay = loc.as_ref().is_some_and(|s| s.last_double_pay);
        let reopen = sys_reexec || double_pay;
        let paid = loc
            .as_ref()
            .and_then(|s| s.stat(s.decision).filter(|(_, n)| *n > 1.0).map(|(c, _)| c));
        let planted = loc
            .as_ref()
            .map(|s| hops_for_strategy(s.decision, n_pairs, self.seg_cap()))
            .unwrap_or(0);
        let stall = self
            .loc_c_ord
            .get(&location)
            .map(|s| s.mean)
            .unwrap_or(PRIOR_C_ORDERED_NS)
            .max(1.0);
        let seg_cap = self.seg_cap();
        eligible
            .iter()
            .map(|&a| {
                let (mut c, n) = loc
                    .as_ref()
                    .and_then(|s| s.stat(a))
                    .unwrap_or((arm_prior(a, n_pairs), 1.0));
                if n <= 1.0 {
                    c = arm_prior(a, n_pairs);
                }
                if reopen {
                    // P3: ĉ is **overlap-aware prepaid** vs OCC whole-spine.
                    // Prefix shorter than w_need = prepaid + leftover OCC.
                    // U: leftover train past a hat-width window is still tail.
                    let hops = hops_for_strategy(a, n_pairs, seg_cap);
                    let light = self.light_hat(n_pairs);
                    let train = self.train_hat(n_pairs);
                    let leftover = loc
                        .as_ref()
                        .map(|s| s.leftover_reexec as usize)
                        .unwrap_or(0);
                    let last_ordered = loc.as_ref().is_some_and(|s| s.decision.is_ordered());
                    let need = loc
                        .as_ref()
                        .map(|s| {
                            if s.w_need >= 1 {
                                s.w_need as usize
                            } else {
                                light_cover_w(n_pairs, light, (s.block_reexec_n as usize).max(2))
                            }
                        })
                        .unwrap_or_else(|| light_cover_w(n_pairs, light, 2))
                        .min(train)
                        .max(1);
                    if a.is_ordered() {
                        let short = hops < need;
                        // U: leftover tail only on a hat-width *ordered* arm that
                        // still leaks. First Opt→cover upgrade uses need, not
                        // n_pairs leftover (that would make Win look like Full).
                        let tail = if short {
                            (need - hops) as f64 * abort
                        } else if last_ordered && leftover >= 2 && hops < leftover {
                            (leftover - hops) as f64 * abort
                        } else {
                            0.0
                        };
                        // Detect wait is stall makespan (S1 independents overlap),
                        // not hops×stall serial. Fat / wide hops stay serial (S).
                        let n_tx = self.block_n();
                        let wall = if n_tx >= 512 || hops > 4 {
                            hops as f64 * stall + tail
                        } else {
                            stall + tail
                        };
                        if n <= 1.0 || short || (last_ordered && leftover >= 2) {
                            c = wall.max(c);
                        }
                    } else if sys_reexec && loc.as_ref().is_some_and(|s| !s.decision.is_ordered()) {
                        // L4: systematic reexec must not nail Opt, even if a
                        // prior cover was measured expensive (blowout → Opt
                        // → unf explode). Floor unused cheap Defer/Opt above
                        // abort *and* the last measured cover.
                        let cover_hat = need as f64 * stall;
                        let cover_c = loc
                            .as_ref()
                            .and_then(|s| s.stat(LocStrategy::win(need)).filter(|(_, n)| *n > 1.5))
                            .map(|(c, _)| c)
                            .unwrap_or(cover_hat);
                        c = c
                            .max(abort)
                            .max(cover_hat + NS_DELTA)
                            .max(cover_c + NS_DELTA);
                    }
                } else {
                    if measured
                        && matches!(a, LocStrategy::OptimisticRead | LocStrategy::DeferPlant)
                    {
                        c = c.max(abort);
                    }
                    if leftover0
                        && let Some(paid) = paid
                        && n <= 1.0
                        && hops_for_strategy(a, n_pairs, seg_cap) > planted
                    {
                        c = c.max(paid);
                    }
                    // Unused Win prior (12k) must not undercut measured
                    // Opt/Defer when that window would leave leftover ≥2.
                    let hop_a = hops_for_strategy(a, n_pairs, seg_cap);
                    if a.is_ordered()
                        && n <= 1.0
                        && n_pairs.saturating_sub(hop_a) >= 2
                        && loc
                            .as_ref()
                            .is_some_and(|s| s.measured && !s.decision.is_ordered())
                    {
                        c = c.max(abort);
                        if let Some(paid) = paid {
                            c = c.max(paid);
                        }
                    }
                }
                if loc.as_ref().is_some_and(|s| s.last_cover_ok)
                    && !a.is_ordered()
                    && let Some(paid) = paid
                {
                    // L4: unused Opt/Defer prior must not undercut a proven
                    // cover. After quiet Win_2, abort EMA drops and the 25k
                    // Opt prior looks cheaper → false retreat, unf explodes
                    // (3356896 i=3/i=5). Yield to OCC only when Opt/Defer
                    // is itself measured cheaper (prepaid blowout).
                    let opt_measured_cheaper = loc
                        .as_ref()
                        .and_then(|s| {
                            s.stat(a)
                                .filter(|(_, n)| *n > 1.5)
                                .map(|(oc, _)| oc + NS_DELTA < paid)
                        })
                        .unwrap_or(false);
                    if !opt_measured_cheaper {
                        c = c.max(paid + NS_DELTA);
                    }
                }
                (a, c.max(1.0), n.max(1.0))
            })
            .collect()
    }

    fn arm_c_of(&self, location: MemoryLocationHash, n_pairs: usize, arm: LocStrategy) -> f64 {
        let eligible = [arm];
        self.arm_c_n(location, n_pairs, &eligible)
            .first()
            .map(|(_, c, _)| *c)
            .unwrap_or_else(|| arm_prior(arm, n_pairs))
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
        // O8: stall is the measured / snapped ĉ_ord, not a SERIAL/META floor.
        let stall = c_ord;
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
        self.pending_idle_n.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn has_pending_idle(&self) -> bool {
        self.pending_idle_n.load(Ordering::Relaxed) > 0
    }

    pub(crate) fn take_pending_idle(&self) -> Vec<(MemoryLocationHash, TxIdx, TxIdx)> {
        self.pending_idle_n.store(0, Ordering::Relaxed);
        std::mem::take(&mut *self.pending_idle.lock().unwrap())
    }

    /// O4: D1 consecutive pairs already stored — skip end_block persist.
    pub(crate) fn d1_pairs_already_stored(
        &self,
        orders: &[(MemoryLocationHash, Vec<TxIdx>)],
    ) -> bool {
        let mut saw = false;
        for (loc, writers) in orders {
            if writers.len() < 2 || !self.is_promoted(*loc) {
                continue;
            }
            saw = true;
            let have = self.pairs_of(*loc);
            for w in writers.windows(2) {
                if !have.iter().any(|&(a, b)| a == w[0] && b == w[1]) {
                    return false;
                }
            }
        }
        saw
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
        // Wide RAW fans (ERC-20 independent / same-`to` calldata) stay
        // OptimisticRead. A probe star of thousands serializes the block and
        // livelocks dual-path skip — do not "fill cores" by widening OrderedAdmit.
        if kind == CohortKind::RawFan {
            self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::OptimisticRead;
        }
        // PC-2: envelope empty-to on an EOA is LazyRecipient — never A1.
        if kind == CohortKind::EmptyTo && !is_contract {
            self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::OptimisticRead;
        }
        // C1/P2: fat EmptyTo is a lazy-payee star (95-hole prepaid). Never A1.
        if kind == CohortKind::EmptyTo && self.block_n() >= FAT_N {
            self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::OptimisticRead;
        }
        // 2-tx same-from pairs stay A0 (3356896 has ~60; refuse is pure meta).
        if kind == CohortKind::SameFrom && cohort_len < 3 {
            self.optimistic_read_cohorts.fetch_add(1, Ordering::Relaxed);
            return AdmitAction::OptimisticRead;
        }
        // Wide nonce chains (ERC-20 clusters: 15 transfers/person) stay
        // OptimisticRead. Full-shell priors would otherwise plant tens of
        // thousands of predecessor edges and livelock dual-path skip.
        if kind == CohortKind::SameFrom && cohort_len >= 8 {
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
        let cores = self.cores() as f64;
        idle * ((cohort_len.saturating_sub(1) as f64) / cores.max(1.0)).min(1.0)
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
            {
                let mut e = self
                    .loc_reexec_ns
                    .entry(loc)
                    .or_insert(OnlineStat::new(PRIOR_C_OPT_NS));
                let a = learn_alpha(e.n);
                e.ema_ns(ns as f64, a);
            }
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
        {
            let mut e = self
                .loc_c_ord
                .entry(location)
                .or_insert(OnlineStat::new(PRIOR_C_ORDERED_NS));
            let a = learn_alpha(e.n);
            e.ema_ns(ns as f64, a);
        }
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
        let lazy_obj = lazy
            || matches!(
                class,
                ConflictClass::LazyNoise | ConflictClass::CommuteCandidate
            );
        self.note_loc_write(location, lazy_obj);
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
        let n_pairs = self
            .short_chain
            .get(&location)
            .map(|c| c.len())
            .unwrap_or(0);
        let mut e = self
            .promoted
            .entry(location)
            .or_insert_with(|| self.new_promoted_seeded(location, n_pairs));
        e.hits = e.hits.saturating_add(1);
        let a = learn_alpha(e.samples as f64);
        e.reexec_ns_ema = (1.0 - a) * e.reexec_ns_ema + a * measured_ns;
        e.bump_c_floor(LocStrategy::OptimisticRead, measured_ns);
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
        let n_pairs = self
            .short_chain
            .get(&location)
            .map(|c| c.len() + 1)
            .unwrap_or(1);
        let mut e = self
            .promoted
            .entry(location)
            .or_insert_with(|| self.new_promoted_seeded(location, n_pairs));
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
            // C1/P3: never persist a lazy writer chain as plantable pairs.
            if self.loc_forbids_ordered(*loc) {
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
            let s = e.value();
            if s.object == LocObject::Lazy {
                continue;
            }
            if s.hits >= 1 && s.measured && s.pred < s.succ && s.succ < n {
                out.push((*e.key(), s.pred, s.succ));
            }
        }
        for e in self.short_chain.iter() {
            let loc = *e.key();
            if !self.is_promoted(loc) || self.loc_forbids_ordered(loc) {
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
        if self.loc_forbids_ordered(location) {
            return false;
        }
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
            if !self.loc_strategy(location, n_pairs.max(1)).is_ordered() {
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
        // Bandit identity: measured or remembered Defer stays in the mouth.
        // Arm pick (not this boolean) decides Opt / Win_w / Defer / Full.
        s.hits >= 1 && (s.measured || s.decision == LocStrategy::DeferPlant)
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
        self.end_block_learn_inner(false);
    }

    /// C3: D1 already has 4→31 on a thin block — skip morph flush / re-widen.
    /// Loc ĉ still updates so Opt/Defer leftover stays the product path.
    pub(crate) fn end_block_learn_stable_d1(&self) {
        self.end_block_learn_inner(true);
    }

    fn end_block_learn_inner(&self, stable_d1: bool) {
        let refuse = self.refuse_ns.load(Ordering::Relaxed);
        let reexec = self.reexec_ns.load(Ordering::Relaxed);
        // S2: prepaid is wall-clock gate stall only. Instant idle / width
        // must not enter ĉ (PROFILE Win_3→Win_1).
        let prepaid = refuse;
        let abort_cf = reexec;
        self.prepaid_ns.store(prepaid, Ordering::Relaxed);
        self.abort_cf_ns.store(abort_cf, Ordering::Relaxed);
        if refuse > 0 {
            let mut s = self.c_ord_ns.lock().unwrap();
            let a = learn_alpha(s.n);
            s.ema_ns(refuse as f64, a);
        }
        if reexec > 0 {
            let mut s = self.c_opt_ns.lock().unwrap();
            let a = learn_alpha(s.n);
            s.ema_ns(reexec as f64, a);
        }
        // Instant idle never enters prepaid. Decay only with a confident
        // unfenced train — not a PREPAID_LOSE_N arm demote ladder.
        let unfenced = self.unfenced_reexec.load(Ordering::Relaxed);
        let prepaid_lost = prepaid > abort_cf && prepaid > 0 && unfenced >= 4;
        if prepaid_lost {
            let n = self.prepaid_lose_streak.fetch_add(1, Ordering::Relaxed) + 1;
            if n >= PREPAID_LOSE_CONF as usize {
                self.decay_ordered_priors();
            }
            if !stable_d1 {
                self.demote_long_spines();
            }
        } else {
            self.prepaid_lose_streak.store(0, Ordering::Relaxed);
        }
        // L-E: abort_cf without prepaid → raise *short* edges. Long spines
        // that O2 demoted stay A0 — those aborts are the cheaper path.
        // C3: stable D1 must not re-widen (O1 leftover is pay-once OCC).
        if abort_cf > prepaid && !stable_d1 {
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
        // L3: per-ℓ reward = −(reexec_ns + ordered_ns + refuse_share).
        // Instant idle / occ_aborts / ready_width never enter ĉ.
        self.update_loc_counterfactuals(prepaid, abort_cf);
        if !stable_d1 {
            self.flush_morph_priors();
        }
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
        let n_hot = self
            .promoted
            .iter()
            .filter(|e| e.measured || e.decision == LocStrategy::DeferPlant)
            .count()
            .max(1) as u64;
        let refuse_share = prepaid / n_hot;
        for mut e in self.promoted.iter_mut() {
            let n_pairs = self.short_chain.get(e.key()).map(|c| c.len()).unwrap_or(0);
            let planted = hops_for_strategy(e.decision, n_pairs, self.seg_cap());
            let leftover_hops = n_pairs.saturating_sub(planted);
            e.leftover_reexec = leftover_hops as u32;
            if e.block_reexec_n > 0 {
                e.leftover_reexec = e.leftover_reexec.max(e.block_reexec_n);
            } else if e.decision.is_ordered() {
                e.leftover_reexec = 0;
            }
            let unfenced = self.unfenced_reexec.load(Ordering::Relaxed);
            let full = full_cover_w(n_pairs);
            let light = self.light_hat(n_pairs);
            let train = self.train_hat(n_pairs);
            // C1/L2/L3: lazy / near_independent never raise Win from
            // unfenced or sys-reexec. Reward stays OCC-comparable wall.
            let lazy_obj = e.object == LocObject::Lazy
                || (self.block_n() >= FAT_N
                    && n_pairs >= LAZY_CHAIN_PAIR_FLOOR
                    && !e.saw_effective);
            if lazy_obj {
                e.last_sys_reexec = false;
                e.last_double_pay = false;
                e.last_crisis = false;
                e.last_cover_ok = false;
                e.object = LocObject::Lazy;
                if e.decision.is_ordered() {
                    e.decision = LocStrategy::OptimisticRead;
                }
                if !e.measured
                    && e.decision != LocStrategy::DeferPlant
                    && e.block_reexec_ns == 0
                    && e.block_ordered_ns == 0
                {
                    continue;
                }
                let actual = e
                    .block_reexec_ns
                    .saturating_add(e.block_ordered_ns.saturating_add(refuse_share));
                let arm = e.decision;
                e.update_arm(arm, actual.max(1) as f64);
                e.samples = e.samples.saturating_add(1);
                e.morph = morph_key(n_pairs, true);
                continue;
            }
            // O1: prefix shorter than the light hat + leftover OCC train.
            // U: grow only after Win_2+ still leaks (unf≥8 or loc reexec≥4
            // past planted). First Win_1 leftover (3356896 cold) is T3 /
            // next-iter light-open, not train_hat climb — that prepaid
            // Win_8 and lost PRIMARY. unf=0–2 leftover hops stay T3.
            let under_cover = e.decision.is_ordered()
                && planted >= 2
                && leftover_hops >= 2
                && (unfenced >= 8 || (e.block_reexec_n >= 4 && leftover_hops > planted));
            let double_pay_now = under_cover;
            let had_sys = e.last_sys_reexec;
            let had_dp = e.last_double_pay;
            if double_pay_now {
                e.last_double_pay = true;
            } else if e.decision.is_ordered() && unfenced < 4 && e.block_reexec_n < 2 {
                e.last_double_pay = false;
            }
            // R1: systematic reexec on Opt/Defer leftover — CC mouth owns it.
            let long_leftover = n_pairs > ORDER_WINDOW_K && leftover_hops >= 2;
            let sys_now = !e.decision.is_ordered()
                && long_leftover
                && (e.block_reexec_n >= 2
                    || e.block_reexec_ns >= (PRIOR_C_OPT_NS as u64).saturating_mul(2)
                    || (unfenced >= 4 && (e.measured || e.samples >= 2)));
            if sys_now {
                e.last_sys_reexec = true;
                e.last_double_pay = false;
            } else if e.decision.is_ordered() && e.block_reexec_n < 2 && unfenced < 4 {
                e.last_sys_reexec = false;
            }
            // L1/U: learn minimal w_need. First sys-reexec opens a light
            // cover (not n_pairs−1). Leftover OCC train grows it up to
            // train_hat. Absorb shrinks it to the proven planted width.
            if sys_now {
                let obs = (e.block_reexec_n as usize).max(2);
                let first = light_cover_w(n_pairs, light, obs);
                e.w_need = if e.w_need == 0 {
                    first as u8
                } else {
                    (e.w_need as usize).max(first).min(train).min(full) as u8
                };
            } else if double_pay_now || under_cover {
                let cur = if e.w_need >= 1 {
                    e.w_need as usize
                } else {
                    planted.max(1)
                };
                let grow = leftover_hops.min(train).max(cur.max(planted) + 1);
                e.w_need = grow.min(train).min(full) as u8;
            } else if e.decision.is_ordered() && e.block_reexec_n < 2 && unfenced < 4 {
                let proven = planted.max(1).min(train).min(full);
                if e.w_need == 0 || (e.w_need as usize) > proven {
                    e.w_need = proven as u8;
                }
            }
            e.last_crisis = match e.decision {
                LocStrategy::OptimisticRead | LocStrategy::DeferPlant => {
                    sys_now || (e.block_reexec_n >= 2 && e.samples >= 2)
                }
                LocStrategy::OrderedWindow { w } => {
                    w <= 1 && (unfenced >= 4 || (e.block_reexec_n >= 2 && leftover_hops >= 2))
                }
                LocStrategy::Segmented { .. } | LocStrategy::FullChain => {
                    unfenced >= 4 && leftover_hops >= 2
                }
            };
            if e.decision.is_ordered() && e.block_reexec_n < 2 && unfenced < 4 {
                e.last_crisis = false;
            }
            let need_now = if e.w_need >= 1 {
                e.w_need as usize
            } else {
                planted.max(1)
            };
            // Light leftover_hops≥2 is cover_ok after a real train (sys /
            // double-pay) *or* a quiet light Win_2+ (3356896: leftover is
            // expected S1 OCC, unf=0–1). Vacuous fat prepaid (planted >
            // light) must not sticky a wide Win.
            let light_quiet =
                planted >= 2 && planted <= light.max(2) && unfenced < 4 && e.block_reexec_n < 2;
            let absorbed_train =
                leftover_hops < 2 || had_sys || had_dp || sys_now || double_pay_now || light_quiet;
            if n_pairs > ORDER_WINDOW_K
                && is_covering(e.decision, n_pairs, self.seg_cap(), need_now)
                && e.block_reexec_n < 2
                && unfenced < 4
                && absorbed_train
            {
                e.last_cover_ok = true;
            } else if e.block_reexec_n >= 2 || unfenced >= 4 {
                e.last_cover_ok = false;
            }
            if !e.measured
                && e.decision != LocStrategy::DeferPlant
                && e.block_reexec_ns == 0
                && e.block_ordered_ns == 0
            {
                continue;
            }
            let actual = e
                .block_reexec_ns
                .saturating_add(e.block_ordered_ns.saturating_add(refuse_share));
            let x = actual.max(1) as f64;
            // F3: credit the begin-selected arm only. No invented ns on others.
            let arm = e.decision;
            e.update_arm(arm, x);
            // O1: do not paint Opt with leftover abort after double-pay —
            // Opt/Defer is the pay-once counterfactual.
            if e.decision != LocStrategy::OptimisticRead && abort_cf > 0 && !e.last_double_pay {
                let cf = (abort_cf as f64).max(e.reexec_ns_ema).max(1.0);
                e.bump_c_floor(LocStrategy::OptimisticRead, cf);
            }
            if e.last_double_pay && e.decision.is_ordered() {
                let tax = (abort_cf as f64)
                    .max(e.reexec_ns_ema)
                    .max(e.block_reexec_ns as f64)
                    .max(1.0);
                let ordered = e.decision;
                e.bump_c_floor(ordered, tax);
            }
            e.samples = e.samples.saturating_add(1);
            e.morph = morph_key(n_pairs, e.object == LocObject::Lazy);
            // Safety: Full on a long spine (should be ineligible) stays demoted.
            if e.decision == LocStrategy::FullChain && n_pairs > ORDER_WINDOW_K {
                e.demoted = true;
            }
        }
    }

    /// G4: share ĉ / w* / s* with the morph bucket (features only).
    fn flush_morph_priors(&self) {
        for e in self.promoted.iter() {
            if e.samples == 0 && e.arms.is_empty() {
                continue;
            }
            let n_pairs = self.short_chain.get(e.key()).map(|c| c.len()).unwrap_or(0);
            // Do not call loc_forbids_ordered while holding promoted.iter().
            let lazy =
                e.object == LocObject::Lazy || self.unknown_looks_lazy(n_pairs, e.saw_effective);
            let key = morph_key(n_pairs, lazy);
            let mut prior = self
                .morphs
                .get(&key)
                .map(|p| *p)
                .unwrap_or_else(MorphPrior::new);
            let alpha = 0.25;
            let lazy = key.lazy;
            if let Some((c, n)) = e.stat(LocStrategy::OptimisticRead) {
                prior.c_opt = (1.0 - alpha) * prior.c_opt + alpha * c;
                prior.n_opt = (prior.n_opt + n) * 0.5;
            }
            if let Some((c, n)) = e.stat(LocStrategy::DeferPlant) {
                prior.c_defer = (1.0 - alpha) * prior.c_defer + alpha * c;
                prior.n_defer = (prior.n_defer + n) * 0.5;
            }
            // L1: lazy morph never carries Win/Full/Seg into the next ℓ.
            if !lazy {
                if e.w_star >= 1 {
                    prior.w_star = e.w_star;
                }
                if e.seg_star >= 2 {
                    prior.seg_star = e.seg_star;
                }
                if let Some((c, n)) = e.stat(LocStrategy::FullChain) {
                    prior.c_full = (1.0 - alpha) * prior.c_full + alpha * c;
                    prior.n_full = (prior.n_full + n) * 0.5;
                }
                if e.w_star >= 1
                    && let Some((c, n)) = e.stat(LocStrategy::win(e.w_star as usize))
                {
                    prior.c_win = (1.0 - alpha) * prior.c_win + alpha * c;
                    prior.n_win = (prior.n_win + n) * 0.5;
                }
                if e.seg_star >= 2
                    && let Some((c, n)) = e.stat(LocStrategy::seg(e.seg_star as usize))
                {
                    prior.c_seg = (1.0 - alpha) * prior.c_seg + alpha * c;
                    prior.n_seg = (prior.n_seg + n) * 0.5;
                }
            }
            prior.seen = prior.seen.saturating_add(1);
            self.morphs.insert(key, prior);
        }
    }

    fn decay_ordered_priors(&self) {
        self.prior_decay.fetch_add(1, Ordering::Relaxed);
        for mut e in self.cohorts.iter_mut() {
            e.p_eff.ema(0.0, 0.35);
            let raised = e.c_ord.mean * 1.25 + PRIOR_C_ORDERED_THIN_NS * 0.15;
            let a = learn_alpha(e.c_ord.n);
            e.c_ord.ema_ns(raised, a);
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
        let mut census = [0usize; CENSUS_N];
        let mut arms: Vec<(MemoryLocationHash, LocStrategy, usize)> = Vec::new();
        let mut win_ws = hashbrown::HashSet::new();
        let mut seg_ls = hashbrown::HashSet::new();
        let mut double_pay_n = 0usize;
        let mut sys_reexec_n = 0usize;
        let mut covering_n = 0usize;
        for e in self.promoted.iter() {
            if e.last_double_pay {
                double_pay_n += 1;
            }
            if e.last_sys_reexec {
                sys_reexec_n += 1;
            }
            if e.hits < 1 {
                continue;
            }
            let i = e.decision.census_bin();
            if i < census.len() {
                census[i] += 1;
            }
            if let LocStrategy::OrderedWindow { w } = e.decision {
                win_ws.insert(w);
            }
            if let LocStrategy::Segmented { seg_len } = e.decision {
                seg_ls.insert(seg_len);
            }
            let n_pairs = self.short_chain.get(e.key()).map(|c| c.len()).unwrap_or(0);
            let need = if e.w_need >= 1 { e.w_need as usize } else { 1 };
            if n_pairs > ORDER_WINDOW_K && is_covering(e.decision, n_pairs, self.seg_cap(), need) {
                covering_n += 1;
            }
            arms.push((*e.key(), e.decision, n_pairs));
        }
        arms.sort_unstable_by_key(|(loc, _, n)| (std::cmp::Reverse(*n), *loc));
        // Long-spine action is the story; storage Full/Win_2 must not hide
        // the leftover-long arm (R5).
        let dominant = arms
            .iter()
            .find(|(_, _, n)| *n > ORDER_WINDOW_K)
            .or_else(|| arms.first())
            .map(|(_, a, _)| *a)
            .unwrap_or(LocStrategy::OptimisticRead);
        let tel = arms
            .iter()
            .find(|(loc, _, n)| *n > ORDER_WINDOW_K || self.is_promoted(*loc))
            .map(|(loc, _, n)| (*loc, *n));
        let (tel_loc, tel_n) = tel.unwrap_or((0, 0));
        let tel_w_cap = self.w_cap_for(tel_loc, tel_n.max(1)) as u8;
        let tel_seg = self.promoted.get(&tel_loc).map(|s| s.seg_star).unwrap_or(0);
        let selected_arms = arms
            .iter()
            .map(|(loc, arm, n)| format!("{loc:x}:{}/{n}", arm.label()))
            .collect::<Vec<_>>()
            .join(",");
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
            defer_locs: census[6],
            chosen_strategy: dominant.label(),
            chosen_win_w: dominant.window_w().min(255) as u8,
            chosen_w_cap: tel_w_cap,
            chosen_seg_len: tel_seg,
            unique_win_w: win_ws.len().min(255) as u8,
            unique_seg_len: seg_ls.len().min(255) as u8,
            explore_budget: self.explore_budget_left.load(Ordering::Relaxed).min(255) as u8,
            pick_occ_n: 0,
            pick_gate_n: 0,
            skip_gate_n: 0,
            occ_pick_while_gated: 0,
            yield_ns: 0,
            gate_stall_ns: 0,
            worker_busy_ns: 0,
            arm_switch_n: self.arm_switch_n.load(Ordering::Relaxed),
            explore_n: self.explore_n.load(Ordering::Relaxed),
            win2_deviate_n: self.win2_deviate_n.load(Ordering::Relaxed),
            bandit_c_opt: self.arm_c_of(tel_loc, tel_n, LocStrategy::OptimisticRead),
            bandit_c_win1: self.arm_c_of(tel_loc, tel_n, LocStrategy::win(1)),
            bandit_c_win2: self.arm_c_of(tel_loc, tel_n, LocStrategy::win(2)),
            bandit_c_win3: self.arm_c_of(tel_loc, tel_n, LocStrategy::win(3)),
            bandit_c_seg: self.arm_c_of(
                tel_loc,
                tel_n,
                if tel_seg >= 2 {
                    LocStrategy::seg(tel_seg as usize)
                } else {
                    LocStrategy::seg(default_seg_len(tel_n, self.cores()))
                },
            ),
            bandit_c_full: self.arm_c_of(tel_loc, tel_n, LocStrategy::FullChain),
            bandit_c_defer: self.arm_c_of(tel_loc, tel_n, LocStrategy::DeferPlant),
            selected_arms,
            double_pay_n,
            sys_reexec_n,
            covering_n,
            chosen_w_need: self.promoted.get(&tel_loc).map(|s| s.w_need).unwrap_or(0),
        }
    }

    pub(crate) fn set_end_block_ns(&self, report: &mut LearnReport, ns: u64) {
        report.end_block_ns = ns;
    }

    /// Test helper: set leftover-slide flags on a promoted loc.
    pub(crate) fn test_set_cover_flags(
        &self,
        location: MemoryLocationHash,
        cover_ok: bool,
        sys_reexec: bool,
        double_pay: bool,
        crisis: bool,
    ) {
        if let Some(mut e) = self.promoted.get_mut(&location) {
            e.last_cover_ok = cover_ok;
            e.last_sys_reexec = sys_reexec;
            e.last_double_pay = double_pay;
            e.last_crisis = crisis;
        }
    }
}

/// G5: shared σ(ĉ) scale so equal-n arms keep ĉ order. Not `UCB_SCALE_NS`.
fn explore_sigma(stats: &[(LocStrategy, f64, f64)]) -> f64 {
    let mut min_c = f64::MAX;
    let mut max_c = 0.0f64;
    let mut measured = false;
    for &(_, c, n) in stats {
        if n > 1.0 {
            measured = true;
            min_c = min_c.min(c);
            max_c = max_c.max(c);
        }
    }
    if measured && max_c > min_c {
        (max_c - min_c).clamp(1.0, PRIOR_C_WIDE_NS * 2.0)
    } else {
        PRIOR_C_WIDE_NS
    }
}

fn explore_score(c: f64, n_a: f64, n_tot: f64, sigma: f64) -> f64 {
    let n_a = n_a.max(1.0);
    let bonus = sigma.max(1.0) * ((n_tot.max(1.0).ln().max(0.0) / n_a).sqrt());
    c.max(1.0) - bonus
}

/// Full-spine cover cap: leftover &lt; 2, never FullChain on a long spine.
/// Safety / grow ceiling only — not the default ĉ pick (L1).
fn full_cover_w(n_pairs: usize) -> usize {
    if n_pairs == 0 {
        return 0;
    }
    if n_pairs > ORDER_WINDOW_K {
        n_pairs.saturating_sub(1).min(WINDOW_SAFETY_HAT).max(1)
    } else {
        n_pairs
    }
}

/// L1: first light cover. Absorbs observed systematic reexec, capped by
/// the prepaid-safe hat — not `n_pairs−1`.
fn light_cover_w(n_pairs: usize, hat: usize, observed: usize) -> usize {
    if n_pairs == 0 {
        return 0;
    }
    if n_pairs <= ORDER_WINDOW_K {
        return n_pairs;
    }
    let full = full_cover_w(n_pairs);
    observed.max(2).min(hat.max(2)).min(full)
}

fn leaves_occ_tail(arm: LocStrategy, n_pairs: usize, seg_cap: usize, w_need: usize) -> bool {
    arm.is_ordered() && hops_for_strategy(arm, n_pairs, seg_cap) < w_need.max(1)
}

/// Long-spine hops == n_pairs is FullChain by another name (R4).
fn is_full_equiv(arm: LocStrategy, n_pairs: usize, seg_cap: usize) -> bool {
    n_pairs > ORDER_WINDOW_K && hops_for_strategy(arm, n_pairs, seg_cap) >= n_pairs
}

/// Light covering: hops meet `w_need`, not a full-spine nail.
fn is_covering(arm: LocStrategy, n_pairs: usize, seg_cap: usize, w_need: usize) -> bool {
    arm.is_ordered()
        && hops_for_strategy(arm, n_pairs, seg_cap) >= w_need.max(1)
        && !is_full_equiv(arm, n_pairs, seg_cap)
}

fn covering_seg_len(n_pairs: usize, cores: usize, w_need: usize) -> usize {
    (w_need + 1).clamp(
        2,
        default_seg_len(n_pairs, cores)
            .max(2)
            .min(SEG_LEN_SAFETY_HAT),
    )
}

fn hops_for_strategy(strategy: LocStrategy, n_pairs: usize, seg_cap: usize) -> usize {
    match strategy {
        LocStrategy::OptimisticRead | LocStrategy::DeferPlant => 0,
        LocStrategy::OrderedWindow { w } => (w as usize).min(n_pairs),
        LocStrategy::Segmented { seg_len } => {
            segmented_hop_count(n_pairs, seg_len as usize, seg_cap)
        }
        LocStrategy::FullChain => n_pairs,
    }
}

fn segmented_hop_count(n_pairs: usize, seg_len: usize, seg_cap: usize) -> usize {
    if n_pairs == 0 || seg_len < 2 {
        return 0;
    }
    let n_tx = (n_pairs + 1).min(seg_cap.saturating_mul(seg_len));
    let full_segs = n_tx / seg_len;
    let rem = n_tx % seg_len;
    full_segs * seg_len.saturating_sub(1) + rem.saturating_sub(1)
}

fn default_seg_len(n_pairs: usize, cores: usize) -> usize {
    let n_tx = n_pairs.saturating_add(1);
    (n_tx / 2).clamp(2, cores.max(2).min(SEG_LEN_SAFETY_HAT))
}

fn select_pairs_capped(
    strategy: LocStrategy,
    pairs: &[(TxIdx, TxIdx)],
    seg_cap: usize,
) -> Vec<(TxIdx, TxIdx)> {
    let mut pairs = pairs.to_vec();
    pairs.sort_unstable_by_key(|(pred, _)| *pred);
    pairs.dedup();
    match strategy {
        LocStrategy::OptimisticRead | LocStrategy::DeferPlant => Vec::new(),
        LocStrategy::OrderedWindow { w } => pairs.into_iter().take(w as usize).collect(),
        LocStrategy::FullChain => pairs,
        LocStrategy::Segmented { seg_len } => {
            intra_segment_pairs(&pairs, seg_len as usize, seg_cap)
        }
    }
}

/// T2: keep hops whose endpoints share a `seg_len` writer-bucket.
fn intra_segment_pairs(
    pairs: &[(TxIdx, TxIdx)],
    seg_len: usize,
    seg_cap: usize,
) -> Vec<(TxIdx, TxIdx)> {
    if seg_len < 2 {
        return Vec::new();
    }
    let mut writers: Vec<TxIdx> = pairs.iter().flat_map(|(a, b)| [*a, *b]).collect();
    writers.sort_unstable();
    writers.dedup();
    let idx = |t: TxIdx| writers.binary_search(&t).ok();
    pairs
        .iter()
        .copied()
        .filter(|&(pred, succ)| match (idx(pred), idx(succ)) {
            (Some(a), Some(b)) => {
                a / seg_len == b / seg_len && a / seg_len < seg_cap && b / seg_len < seg_cap
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
        // Full shell but not fat (THIN_N_MAX < n < FAT_N). Fat EmptyTo is A0.
        p.begin_block(400);
        let addr = Address::repeat_byte(0x20);
        assert!(
            p.choose_ordered(CohortKind::EmptyTo, addr, 16, true, 0.30),
            "full-shell contract empty-to n=16 stays A1"
        );
    }

    #[test]
    fn wide_raw_fan_stays_optimistic() {
        let p = CostPolicy::new();
        p.begin_block(4096);
        let addr = Address::repeat_byte(0x32);
        assert!(
            !p.choose_ordered(CohortKind::RawFan, addr, 16, true, 0.50),
            "wide RAW fan (ERC-20 independent class) must stay OptimisticRead"
        );
        assert!(
            !p.choose_ordered(CohortKind::RawFan, addr, 37123, true, 0.90),
            "37k same-to calldata must not plant a probe star"
        );
        assert!(
            !p.choose_ordered(CohortKind::SameFrom, addr, 15, false, 0.50),
            "wide same-from (ERC-20 clusters) must stay OptimisticRead"
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
        p.begin_block(400);
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
    fn thin_is_n_cap_not_meta_floor() {
        let p = CostPolicy::new();
        p.begin_block(220);
        assert!(
            p.is_optimistic_majority_block(),
            "O8: n=220 ≤ THIN_N_MAX is thin even if 220·2µs > the retired 400µs META_FLOOR"
        );
        p.begin_block(256);
        assert!(
            p.is_optimistic_majority_block(),
            "O8: n=THIN_N_MAX stays thin"
        );
        p.begin_block(257);
        assert!(
            !p.is_optimistic_majority_block(),
            "O8: n>THIN_N_MAX is full shell"
        );
    }

    #[test]
    fn optimistic_majority_block_on_small_block() {
        let p = CostPolicy::new();
        p.begin_block(176);
        assert!(
            p.is_optimistic_majority_block(),
            "n=176 ≤ THIN_N_MAX is the thin-shell safety class (O8: not META_FLOOR)"
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
        p.begin_block(400);
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
        p.end_block_learn();
        p.begin_block(176);
        p.note_refuse_ns(50_000);
        for _ in 0..4 {
            p.bump_unfenced_reexec();
        }
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
            "L6: confident prepaid+unfenced streak decays prior (not a 2-step arm ladder): {r:?}"
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
        assert_ne!(
            first,
            LocStrategy::FullChain,
            "CC-L3/L4: long thin spine never FullChain: {first:?}"
        );
        assert!(
            first.is_ordered()
                || matches!(first, LocStrategy::OptimisticRead | LocStrategy::DeferPlant),
            "O1: leftover-long spine is Win_w/Seg or pay-once Opt/Defer, got {first:?}"
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
        let first = p.loc_strategy(0x32be, 3);
        assert!(
            first.is_ordered() && first != LocStrategy::FullChain,
            "F2: measured abort makes an OrderedAdmit window cheaper than Opt/Defer: {first:?}"
        );
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
    fn bandit_picks_by_c_hat_not_leftover_ladder() {
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
        let cold = p.loc_strategy(0x32be, 8);
        assert_ne!(
            cold,
            LocStrategy::FullChain,
            "L4: long spine never Full, got {cold:?}"
        );
        p.note_hops_decision(0x32be, 8);
        p.note_reexec_ns_at(Some(0x32be), 80_000);
        p.note_reexec_ns_at(Some(0x32be), 80_000);
        p.end_block_learn();
        p.begin_block(176);
        let reuse = p.loc_strategy(0x32be, 8);
        assert_ne!(
            reuse,
            LocStrategy::FullChain,
            "L4: Full stays banned on long spine"
        );
        assert!(
            reuse != cold || reuse.is_ordered(),
            "L4: reexec raises ĉ of the chosen arm so UCB can leave it: cold={cold:?} reuse={reuse:?}"
        );
        let hops = p.hops_to_plant(0x32be, 8);
        assert!(
            hops < 8,
            "L4: bandit must not FullChain the long spine ({hops})"
        );
    }

    #[test]
    fn bandit_prepaid_wall_can_leave_wider_window() {
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
        // Half-window Win_3 on an 8-pair spine, then a fat prepaid wall.
        p.remember_arm(0x32be, LocStrategy::win(3));
        p.note_refuse_ns(200_000);
        p.note_loc_ordered_ns(0x32be, 200_000);
        p.end_block_learn();
        p.begin_block(176);
        let next = p.loc_strategy(0x32be, 8);
        assert_ne!(next, LocStrategy::FullChain);
        assert_ne!(
            next,
            LocStrategy::win(3),
            "L3/L4: high prepaid wall must raise ĉ_Win3 so another arm can win: {next:?}"
        );
        let r = p.take_report(0.0, 0);
        assert!(
            r.bandit_c_win3 > r.bandit_c_win1
                || r.win2_deviate_n > 0
                || next != LocStrategy::win(3),
            "L7: ĉ or arm must move off hard Win_3: {r:?} next={next:?}"
        );
    }

    #[test]
    fn bandit_short_defer_can_win() {
        let p = CostPolicy::new();
        p.begin_block(176);
        p.promote_short_edge(0xedba, 8_000);
        p.note_short_pair(0xedba, 14, 16);
        p.note_short_pair(0xedba, 16, 17);
        let cold = p.select_hint_arm(0xedba, 2);
        assert_eq!(
            cold,
            LocStrategy::FullChain,
            "L5: short cold prior prefers Full, got {cold:?}"
        );
        p.remember_arm(0xedba, LocStrategy::FullChain);
        p.note_refuse_ns(180_000);
        p.note_loc_ordered_ns(0xedba, 180_000);
        p.end_block_learn();
        p.begin_block(176);
        let next = p.select_hint_arm(0xedba, 2);
        assert_ne!(
            next,
            LocStrategy::FullChain,
            "L5: DeferPlant / Opt / Win_w can beat Full after prepaid: {next:?}"
        );
        assert_ne!(next, LocStrategy::win(3));
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
        let got = CostPolicy::select_pairs_for_strategy(LocStrategy::seg(4), &pairs);
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

    #[test]
    fn w_cap_varies_with_n_pairs_and_cores() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        let thin_long = p.w_cap_of(16);
        let thin_one = p.w_cap_of(1);
        assert_eq!(
            thin_long, 2,
            "G1: 176/8 oversub hat is 2 (not frozen WINDOWED_W_MAX=3), got {thin_long}"
        );
        assert_ne!(
            thin_one, thin_long,
            "G1: 1-pair w_cap != long-spine w_cap ({thin_one} vs {thin_long})"
        );
        p.begin_block_with_cores(32, 16);
        let wide = p.w_cap_of(16);
        assert!(
            wide > thin_long && wide > 3,
            "G1: roomier block/cores must raise w_cap above oversub hat and the retired WINDOWED_W_MAX=3 (wide={wide} thin_long={thin_long})"
        );
        p.begin_block_with_cores(4096, 8);
        let fat = p.w_cap_of(16);
        assert!(
            fat >= 2 && fat != wide,
            "G1: fat/oversub vs roomy w_cap differ (fat={fat} wide={wide})"
        );
    }

    #[test]
    fn generate_arms_not_fixed_seven_slot_table() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        let cold = p.generate_arms(0x32be, 4, false, true, false);
        assert!(
            cold.iter()
                .any(|a| matches!(a, LocStrategy::OrderedWindow { .. })),
            "G3: generator must propose OrderedWindow(w*), got {cold:?}"
        );
        assert!(
            !cold.iter().any(|a| matches!(a, LocStrategy::FullChain)),
            "G3: long spine never generates Full: {cold:?}"
        );
        assert!(
            cold.len() != 7
                || cold.iter().any(|a| matches!(a, LocStrategy::OrderedWindow { w } if *w != 1 && *w != 2 && *w != 3)),
            "G3: candidate set is generated, not ARM_N=7 {{Win1,2,3}}: {cold:?}"
        );
        // Make the loc hot: enough samples + successful last arm.
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.decision = LocStrategy::win(2);
            e.w_star = 2;
            e.last_crisis = false;
            e.leftover_reexec = 0;
            e.upsert_stat(LocStrategy::win(2), 12_000.0, 4.0);
        }
        let hot = p.generate_arms(0x32be, 4, false, false, false);
        assert!(
            !hot.iter()
                .any(|a| a.is_high_prepaid(4) || matches!(a, LocStrategy::Segmented { .. })),
            "E3: hot path must not generate Seg/Full/wide windows: {hot:?}"
        );
        assert!(
            hot.iter().any(|a| *a == LocStrategy::win(2)),
            "E1: hot generator keeps posterior w*: {hot:?}"
        );
        assert!(
            !hot.iter().any(|a| *a == LocStrategy::win(3)),
            "E3: hot must not grow w+1 for fun: {hot:?}"
        );
        assert!(
            !hot.iter()
                .any(|a| matches!(a, LocStrategy::OptimisticRead | LocStrategy::DeferPlant)),
            "E1: unmeasured Opt/Defer must not stay on a hot working window: {hot:?}"
        );
    }

    #[test]
    fn hot_phase_is_greedy_zero_explore() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 5;
            e.decision = LocStrategy::win(2);
            e.prev_decision = LocStrategy::win(2);
            e.w_star = 2;
            e.last_crisis = false;
            e.leftover_reexec = 0;
            e.upsert_stat(LocStrategy::win(1), 40_000.0, 3.0);
            e.upsert_stat(LocStrategy::win(2), 10_000.0, 5.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 80_000.0, 3.0);
            e.upsert_stat(LocStrategy::DeferPlant, 80_000.0, 3.0);
        }
        let (hot, crisis) = p.phase_of(0x32be);
        assert!(hot && !crisis, "E1: enough samples + last arm OK is hot");
        let (arm, explore, greedy) = p.select_arm(0x32be, 4);
        assert!(!explore, "E1: hot explore → 0, got explore arm={arm:?}");
        assert_eq!(arm, greedy);
        assert_eq!(
            arm,
            LocStrategy::win(2),
            "E1: hot greedy min-ĉ sticks at proven Win_2, got {arm:?}"
        );
        p.remember_arm(0x32be, arm);
        let r = p.take_report(0.0, 0);
        assert_eq!(
            r.explore_budget, 0,
            "E2: 176 txs / 8 cores is oversubscribed → hot budget 0"
        );
        // Cheap unmeasured Defer prior must not steal a working Win_2 (run3
        // plant-miss: hops=0, refuse=0, unfenced OCC + SF shell).
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.arms.retain(|s| {
                !matches!(s.arm, LocStrategy::OptimisticRead | LocStrategy::DeferPlant)
            });
            e.decision = LocStrategy::win(2);
            e.last_crisis = false;
        }
        let (stuck, explore2, _) = p.select_arm(0x32be, 4);
        assert!(
            !explore2 && stuck == LocStrategy::win(2),
            "E1: hot exploit stays on measured Win_2, not cheap Defer prior: {stuck:?}"
        );
    }

    #[test]
    fn seg_len_and_candidates_change_with_spine() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(32, 16);
        let short_s = default_seg_len(3, 16);
        let long_s = default_seg_len(16, 16);
        assert_ne!(
            short_s, long_s,
            "G2: seg_len prior is not SEG_TX=4 for every spine (short={short_s} long={long_s})"
        );
        p.promote_short_edge(0xaaaa, 20_000);
        for pair in [(1, 2), (2, 3), (3, 4)] {
            p.note_short_pair(0xaaaa, pair.0, pair.1);
        }
        p.promote_short_edge(0xbbbb, 20_000);
        for i in 0..12 {
            p.note_short_pair(0xbbbb, 10 + i, 11 + i);
        }
        // Exhaust the window neighborhood so Seg enters the generator (E3).
        {
            let cap_s = p.w_cap_of(3).max(1) as u8;
            let mut e = p.promoted.get_mut(&0xaaaa).unwrap();
            e.w_star = cap_s;
        }
        {
            let cap_l = p.w_cap_of(12).max(1) as u8;
            let mut e = p.promoted.get_mut(&0xbbbb).unwrap();
            e.w_star = cap_l;
        }
        let short_cands = p.generate_arms(0xaaaa, 3, false, true, false);
        let long_cands = p.generate_arms(0xbbbb, 12, false, true, false);
        let short_ws: Vec<u8> = short_cands
            .iter()
            .filter_map(|a| match a {
                LocStrategy::OrderedWindow { w } => Some(*w),
                _ => None,
            })
            .collect();
        let long_ws: Vec<u8> = long_cands
            .iter()
            .filter_map(|a| match a {
                LocStrategy::OrderedWindow { w } => Some(*w),
                _ => None,
            })
            .collect();
        assert!(
            short_ws != long_ws || p.w_cap_of(3) != p.w_cap_of(12),
            "G1/G3: shorter vs longer spine changes w_cap or window candidates (short={short_ws:?} long={long_ws:?})"
        );
        let short_segs: Vec<u8> = short_cands
            .iter()
            .filter_map(|a| match a {
                LocStrategy::Segmented { seg_len } => Some(*seg_len),
                _ => None,
            })
            .collect();
        let long_segs: Vec<u8> = long_cands
            .iter()
            .filter_map(|a| match a {
                LocStrategy::Segmented { seg_len } => Some(*seg_len),
                _ => None,
            })
            .collect();
        assert_ne!(
            short_segs, long_segs,
            "G2: Seg(seg_len) candidates move with spine length (short={short_segs:?} long={long_segs:?})"
        );
    }

    #[test]
    fn morph_prior_seeds_new_location() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.remember_arm(0x32be, LocStrategy::win(2));
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.w_star = 2;
            e.upsert_stat(LocStrategy::win(2), 11_000.0, 4.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 90_000.0, 3.0);
        }
        p.end_block_learn();
        // New long-spine ℓ inherits the morph prior — not a from-zero table.
        p.ensure_promoted_seeded(0xfeed, 4);
        let seeded = p.promoted.get(&0xfeed).unwrap();
        assert_eq!(
            seeded.w_star, 2,
            "G4: new ℓ inherits morph w*, got {}",
            seeded.w_star
        );
        let (c, n) = seeded.stat(LocStrategy::win(2)).expect("inherited Win_2");
        assert!(
            n > 1.0 && c < 20_000.0,
            "G4: inherited Win_2 ĉ/n from morph (c={c} n={n})"
        );
    }

    #[test]
    fn leftover_occ_on_proven_window_is_not_crisis() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.remember_arm(0x32be, LocStrategy::win(2));
        p.note_reexec_ns_at(Some(0x32be), 8_000);
        p.note_reexec_ns_at(Some(0x32be), 8_000);
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(
                !e.last_crisis,
                "E1: leftover OCC on Win_2 + unfenced=0 is not a climb crisis"
            );
        }
        p.begin_block_with_cores(176, 8);
        p.remember_arm(0x32be, LocStrategy::win(1));
        p.note_reexec_ns_at(Some(0x32be), 8_000);
        p.note_reexec_ns_at(Some(0x32be), 8_000);
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(
                e.last_crisis,
                "E1: narrow Win_1 still leaking leftover OCC may grow once"
            );
        }
        p.begin_block_with_cores(176, 8);
        p.remember_arm(0x32be, LocStrategy::win(2));
        for _ in 0..4 {
            p.bump_unfenced_reexec();
        }
        p.note_reexec_ns_at(Some(0x32be), 8_000);
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(
                !e.last_crisis,
                "O7: leftover OCC train on proven Win_2 is not a widen-window crisis"
            );
            assert!(
                !e.last_double_pay,
                "L1: Win_2 meeting the oversub hat is T3-slide, not a climb"
            );
        }
    }

    #[test]
    fn double_pay_reopens_covering_not_half_window() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(32, 16);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 5;
            e.decision = LocStrategy::win(2);
            e.w_star = 2;
            e.last_crisis = false;
            e.last_double_pay = true;
            e.last_sys_reexec = false;
            e.w_need = 3;
            e.upsert_stat(LocStrategy::win(2), 80_000.0, 4.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 20_000.0, 3.0);
            e.upsert_stat(LocStrategy::DeferPlant, 18_000.0, 3.0);
        }
        let hot = p.generate_arms(0x32be, 4, false, false, false);
        assert!(
            hot.iter()
                .any(|a| matches!(a, LocStrategy::OptimisticRead | LocStrategy::DeferPlant)),
            "R1: leftover double-pay still keeps Opt/Defer as a trial: {hot:?}"
        );
        assert!(
            hot.iter().any(|a| is_covering(*a, 4, p.seg_cap(), 3)),
            "L1: leftover re-opens a light covering arm (w_need=3), got {hot:?}"
        );
        assert!(
            !hot.iter().any(|a| leaves_occ_tail(*a, 4, p.seg_cap(), 3)),
            "L1: prefix shorter than w_need + OCC tail is not eligible, got {hot:?}"
        );
        let cold_dp = p.generate_arms(0x32be, 4, false, true, true);
        assert!(
            cold_dp.iter().any(|a| is_covering(*a, 4, p.seg_cap(), 3)),
            "L1: cold leftover set includes light Win/Seg, got {cold_dp:?}"
        );
        // Half-window Win_2 prior must not win ĉ after leftover OCC.
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.arms
                .retain(|s| !matches!(s.arm, LocStrategy::OrderedWindow { w: 2 }));
            e.reexec_ns_ema = 90_000.0;
            e.last_double_pay = true;
            e.decision = LocStrategy::win(2);
            e.w_star = 2;
            e.w_need = 3;
        }
        p.block_arm.clear();
        let (arm2, _, _) = p.select_arm(0x32be, 4);
        assert!(
            !leaves_occ_tail(arm2, 4, p.seg_cap(), 3),
            "L1: select_arm must not schedule prefix < w_need + OCC tail, got {arm2:?}"
        );
    }

    #[test]
    fn unused_win_prior_does_not_undercut_measured_opt_on_leftover_spine() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 180_000);
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
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 2;
            e.measured = true;
            e.decision = LocStrategy::OptimisticRead;
            e.last_crisis = false;
            e.last_double_pay = false;
            e.reexec_ns_ema = 180_000.0;
            e.upsert_stat(LocStrategy::OptimisticRead, 62_716.0, 2.0);
        }
        p.block_arm.clear();
        let (arm, _, _) = p.select_arm(0x32be, 8);
        assert!(
            matches!(arm, LocStrategy::OptimisticRead | LocStrategy::DeferPlant),
            "O1: unused Win_1/2 prior 12k must not replace measured Opt on a leftover spine, got {arm:?}"
        );
    }

    #[test]
    fn commit_arm_persists_defer_decision() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.decision = LocStrategy::win(1);
            e.w_star = 1;
            e.last_crisis = true;
            e.last_double_pay = true;
            e.last_sys_reexec = false;
            e.reexec_ns_ema = 180_000.0;
            e.upsert_stat(LocStrategy::win(1), 270_000.0, 3.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 62_000.0, 2.0);
            e.upsert_stat(LocStrategy::DeferPlant, 9_000.0, 2.0);
        }
        p.block_arm.clear();
        let arm = p.loc_strategy(0x32be, 4);
        assert!(
            matches!(arm, LocStrategy::OptimisticRead | LocStrategy::DeferPlant),
            "O1: after double-pay loc_strategy is Defer/Opt, got {arm:?}"
        );
        assert_eq!(
            p.hops_to_plant(0x32be, 4),
            0,
            "O1: Defer/Opt plants no begin prefix"
        );
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert_eq!(
                e.decision, arm,
                "O1: hops=0 Defer/Opt must still write e.decision (census/learn)"
            );
        }
    }

    #[test]
    fn long_spine_never_keeps_short_n_full_cache() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
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
        p.block_arm.insert(0x32be, LocStrategy::FullChain);
        let long = p.loc_strategy(0x32be, 8);
        assert_ne!(
            long,
            LocStrategy::FullChain,
            "O1: short-n Full cache must not FullChain n=8, got {long:?}"
        );
        assert!(
            p.hops_to_plant(0x32be, 8) < 8,
            "O1: long spine hops < n_pairs, got {}",
            p.hops_to_plant(0x32be, 8)
        );
    }

    #[test]
    fn leftover_long_measured_opt_does_not_explore_window() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
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
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 3;
            e.measured = true;
            e.decision = LocStrategy::DeferPlant;
            e.last_crisis = false;
            e.last_double_pay = false;
            e.last_sys_reexec = false;
            e.upsert_stat(LocStrategy::DeferPlant, 8_096.0, 2.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 47_962.0, 2.0);
        }
        p.block_arm.clear();
        let arms = p.generate_arms(0x32be, 8, false, true, false);
        assert!(
            !arms.iter().any(|a| a.is_ordered()),
            "no-signal leftover Defer must not re-offer unused Win prior: {arms:?}"
        );
        let (arm, explore, _) = p.select_arm(0x32be, 8);
        assert!(
            !explore && matches!(arm, LocStrategy::OptimisticRead | LocStrategy::DeferPlant),
            "no-signal: unused Win prior must not UCB over measured Defer, got explore={explore} {arm:?}"
        );
    }

    #[test]
    fn explore_budget_zero_when_oversubscribed() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        assert_eq!(p.explore_budget(), 0, "E2: 176/8 oversub → B=0");
        p.begin_block_with_cores(32, 16);
        assert!(
            p.explore_budget() >= 1,
            "E2: roomier cores/block may explore, got {}",
            p.explore_budget()
        );
    }

    #[test]
    fn stable_d1_sys_reexec_reopens_covering_ordered() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
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
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.measured = true;
            e.decision = LocStrategy::OptimisticRead;
            e.last_double_pay = true;
            e.last_sys_reexec = false;
            e.last_crisis = false;
            e.reexec_ns_ema = 180_000.0;
            e.upsert_stat(LocStrategy::OptimisticRead, 90_000.0, 3.0);
            e.upsert_stat(LocStrategy::DeferPlant, 8_096.0, 2.0);
        }
        for _ in 0..8 {
            p.bump_unfenced_reexec();
        }
        p.note_reexec_ns_at(Some(0x32be), 80_000);
        p.note_reexec_ns_at(Some(0x32be), 80_000);
        p.end_block_learn_stable_d1();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(
                e.last_sys_reexec,
                "R1: unfenced/reexec train on leftover Opt is systematic reexec"
            );
        }
        p.begin_block_with_cores(176, 8);
        p.block_arm.clear();
        let need = p.loc_w_need(0x32be, 8);
        assert!(
            need >= 2 && need < 8,
            "L1: first sys-reexec opens light w_need, not full spine, need={need}"
        );
        let (arm, _, _) = p.select_arm(0x32be, 8);
        assert!(
            is_covering(arm, 8, p.seg_cap(), need),
            "L1: stable D1 sys-reexec upgrades to light covering, got {arm:?} need={need}"
        );
        assert_ne!(
            arm,
            LocStrategy::FullChain,
            "R4: long spine never FullChain"
        );
        let hops = p.hops_to_plant(0x32be, 8);
        assert!(
            hops == need && hops < 8,
            "L1: light cover plants w_need not n_pairs−1, hops={hops} need={need}"
        );
    }

    #[test]
    fn sys_reexec_reopens_covering_ordered_arm() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
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
        p.remember_arm(0x32be, LocStrategy::DeferPlant);
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.measured = true;
            e.decision = LocStrategy::DeferPlant;
            e.last_double_pay = true;
            e.last_sys_reexec = false;
            e.reexec_ns_ema = 160_000.0;
            e.upsert_stat(LocStrategy::DeferPlant, 8_096.0, 3.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 48_000.0, 3.0);
        }
        p.note_reexec_ns_at(Some(0x32be), 70_000);
        p.note_reexec_ns_at(Some(0x32be), 70_000);
        for _ in 0..8 {
            p.bump_unfenced_reexec();
        }
        p.end_block_learn();
        p.begin_block_with_cores(176, 8);
        p.block_arm.clear();
        let need = p.loc_w_need(0x32be, 8);
        assert!(
            need >= 2 && need < 8,
            "L1: sys-reexec w_need is light, not n_pairs−1, need={need}"
        );
        let arms = p.generate_arms(0x32be, 8, false, true, true);
        assert!(
            arms.iter().any(|a| is_covering(*a, 8, p.seg_cap(), need)),
            "L1: sys-reexec eligible set includes light Win/Seg, got {arms:?} need={need}"
        );
        assert!(
            !arms
                .iter()
                .any(|a| leaves_occ_tail(*a, 8, p.seg_cap(), need)),
            "L1: sys-reexec must not offer prefix < w_need, got {arms:?}"
        );
        let (arm, _, _) = p.select_arm(0x32be, 8);
        assert!(
            is_covering(arm, 8, p.seg_cap(), need),
            "L1: sys-reexec upgrades leftover Defer to light covering, got {arm:?}"
        );
        p.remember_arm(0x32be, arm);
        let r = p.take_report(0.0, 0);
        assert!(
            r.chosen_strategy.starts_with("Win_") || r.chosen_strategy.starts_with("Seg_"),
            "R5: long ℓ telemetry moves Opt/Defer → Win/Seg, got {}",
            r.chosen_strategy
        );
        assert_eq!(r.covering_n, 1, "R5: covering_n counts the upgraded ℓ");
    }

    #[test]
    fn sys_reexec_full_spine_wall_rejects_half_window() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(32, 16);
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
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.measured = true;
            e.decision = LocStrategy::OptimisticRead;
            e.last_sys_reexec = true;
            e.last_double_pay = false;
            e.w_need = 4;
            e.reexec_ns_ema = 150_000.0;
            e.upsert_stat(LocStrategy::OptimisticRead, 80_000.0, 3.0);
            e.upsert_stat(LocStrategy::win(2), 12_000.0, 1.0);
        }
        p.block_arm.clear();
        let (arm, _, _) = p.select_arm(0x32be, 8);
        assert_ne!(
            arm,
            LocStrategy::win(2),
            "L1: unused cheap Win_2 prior is shorter than w_need=4, got {arm:?}"
        );
        assert!(
            !leaves_occ_tail(arm, 8, p.seg_cap(), 4),
            "L1: ĉ must not pick prefix < w_need, got {arm:?}"
        );
    }

    #[test]
    fn hot_covering_ordered_is_sticky() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(32, 16);
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
        let cover = LocStrategy::win(4);
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 5;
            e.measured = true;
            e.decision = cover;
            e.prev_decision = cover;
            e.w_star = 4;
            e.w_need = 4;
            e.last_crisis = false;
            e.last_double_pay = false;
            e.last_sys_reexec = false;
            e.last_cover_ok = true;
            e.leftover_reexec = 0;
            e.upsert_stat(cover, 40_000.0, 5.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 180_000.0, 3.0);
            e.upsert_stat(LocStrategy::DeferPlant, 180_000.0, 3.0);
        }
        let (hot, crisis) = p.phase_of(0x32be);
        assert!(hot && !crisis, "L3: proven light covering arm is hot");
        let (arm, explore, greedy) = p.select_arm(0x32be, 8);
        assert!(
            !explore,
            "L3: hot light cover is sticky, got explore {arm:?}"
        );
        assert_eq!(arm, greedy);
        assert_eq!(arm, cover, "L3: hot exploit stays on proven light cover");
    }

    #[test]
    fn sys_reexec_picks_minimal_w_not_full_cover() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
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
        p.remember_arm(0x32be, LocStrategy::OptimisticRead);
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 3;
            e.measured = true;
            e.decision = LocStrategy::OptimisticRead;
            e.upsert_stat(LocStrategy::OptimisticRead, 90_000.0, 3.0);
            e.upsert_stat(LocStrategy::DeferPlant, 8_000.0, 2.0);
        }
        p.note_reexec_ns_at(Some(0x32be), 70_000);
        p.note_reexec_ns_at(Some(0x32be), 70_000);
        for _ in 0..8 {
            p.bump_unfenced_reexec();
        }
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(e.last_sys_reexec, "R1: Opt leftover train is sys-reexec");
            assert!(
                e.w_need >= 2 && (e.w_need as usize) < 7,
                "L1: first need is light, not n_pairs−1, w_need={}",
                e.w_need
            );
        }
        p.begin_block_with_cores(176, 8);
        p.block_arm.clear();
        let (arm, _, _) = p.select_arm(0x32be, 8);
        let hops = hops_for_strategy(arm, 8, p.seg_cap());
        assert!(
            arm.is_ordered() && hops < 7 && hops >= 2,
            "L1: first ordered arm is light cover, got {arm:?} hops={hops}"
        );
    }

    #[test]
    fn double_pay_grows_w_need_not_occ_tail() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(32, 16);
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
        p.remember_arm(0x32be, LocStrategy::win(2));
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.measured = true;
            e.decision = LocStrategy::win(2);
            e.w_star = 2;
            e.w_need = 2;
            e.last_sys_reexec = false;
            e.upsert_stat(LocStrategy::win(2), 40_000.0, 3.0);
        }
        for _ in 0..8 {
            p.bump_unfenced_reexec();
        }
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(e.last_double_pay, "L1: leftover OCC on Win_2 is double-pay");
            assert!(
                (e.w_need as usize) > 2,
                "L1: leftover train grows w_need above 2, got {}",
                e.w_need
            );
            assert!(
                (e.w_need as usize) <= 7,
                "L1: grow is capped at n_pairs−1, got {}",
                e.w_need
            );
        }
        p.begin_block_with_cores(32, 16);
        p.block_arm.clear();
        let need = p.loc_w_need(0x32be, 8);
        let (arm, _, _) = p.select_arm(0x32be, 8);
        assert!(
            !leaves_occ_tail(arm, 8, p.seg_cap(), need),
            "L1: grown need must not schedule OCC tail, got {arm:?} need={need}"
        );
    }

    #[test]
    fn light_cover_ok_allows_leftover_hops() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
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
        p.remember_arm(0x32be, LocStrategy::win(4));
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.measured = true;
            e.decision = LocStrategy::win(4);
            e.w_star = 4;
            e.w_need = 4;
            e.last_sys_reexec = true;
            e.block_reexec_n = 0;
            e.block_reexec_ns = 0;
            e.block_ordered_ns = 8_000;
        }
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(
                e.last_cover_ok,
                "L1: light Win_4 with unfenced=0 is cover_ok even if leftover_hops≥2"
            );
            assert!(
                !e.last_double_pay,
                "L1: no leftover OCC train → no double-pay"
            );
            assert!(
                e.w_need >= 2 && e.w_need <= 4,
                "L1: absorbed light w stays at/under the hat, w_need={}",
                e.w_need
            );
        }
    }

    #[test]
    fn prepaid_blowout_allows_occ_after_light_cover() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(32, 16);
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
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 5;
            e.measured = true;
            e.decision = LocStrategy::win(4);
            e.w_star = 4;
            e.w_need = 4;
            e.last_cover_ok = true;
            e.last_sys_reexec = false;
            e.last_double_pay = false;
            e.last_crisis = false;
            e.reexec_ns_ema = 40_000.0;
            e.upsert_stat(LocStrategy::win(4), 200_000.0, 4.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 40_000.0, 3.0);
            e.upsert_stat(LocStrategy::DeferPlant, 40_000.0, 3.0);
        }
        p.block_arm.clear();
        let (arm, _, _) = p.select_arm(0x32be, 8);
        assert!(
            matches!(arm, LocStrategy::OptimisticRead | LocStrategy::DeferPlant),
            "L4: prepaid blowout vs OCC may yield to Opt/Defer, got {arm:?}"
        );
    }

    #[test]
    fn under_cover_train_grows_w_need_past_oversub_hat() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
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
        p.remember_arm(0x32be, LocStrategy::win(2));
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.measured = true;
            e.decision = LocStrategy::win(2);
            e.w_star = 2;
            e.w_need = 2;
            e.last_sys_reexec = false;
            e.upsert_stat(LocStrategy::win(2), 40_000.0, 3.0);
        }
        for _ in 0..8 {
            p.bump_unfenced_reexec();
        }
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(
                e.last_double_pay,
                "U: leftover train on hat-width Win_2 is under-cover, not T3-only"
            );
            assert!(
                (e.w_need as usize) > 2,
                "U: leftover train grows w_need past oversub hat 2, got {}",
                e.w_need
            );
            assert!(
                (e.w_need as usize) <= 8,
                "U: grow is train_hat (cores-scaled), not n_pairs−1, got {}",
                e.w_need
            );
        }
        p.begin_block_with_cores(176, 8);
        p.block_arm.clear();
        let need = p.loc_w_need(0x32be, 8);
        assert!(
            need > 2 && need <= 8,
            "U: loc_w_need must not re-clamp stored need to light_hat=2, need={need}"
        );
        let (arm, _, _) = p.select_arm(0x32be, 8);
        assert!(
            !leaves_occ_tail(arm, 8, p.seg_cap(), need),
            "U/D: grown need must not schedule OCC tail, got {arm:?} need={need}"
        );
        assert!(
            hops_for_strategy(arm, 8, p.seg_cap()) < 8,
            "U: still not a full-spine nail, got {arm:?}"
        );
    }

    #[test]
    fn sys_reexec_after_blowout_does_not_nail_opt() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(32, 16);
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
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 5;
            e.measured = true;
            e.decision = LocStrategy::OptimisticRead;
            e.w_star = 4;
            e.w_need = 4;
            e.last_cover_ok = false;
            e.last_sys_reexec = true;
            e.last_double_pay = false;
            e.last_crisis = false;
            e.reexec_ns_ema = 180_000.0;
            e.leftover_reexec = 8;
            e.upsert_stat(LocStrategy::win(4), 200_000.0, 4.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 40_000.0, 3.0);
            e.upsert_stat(LocStrategy::DeferPlant, 40_000.0, 3.0);
        }
        p.block_arm.clear();
        let (arm, _, _) = p.select_arm(0x32be, 8);
        assert!(
            is_covering(arm, 8, p.seg_cap(), 4),
            "L4: sys-reexec after prepaid blowout must re-open covering, not nail Opt, got {arm:?}"
        );
        assert!(
            !matches!(arm, LocStrategy::OptimisticRead | LocStrategy::DeferPlant),
            "L4: Opt/Defer is floored under systematic reexec, got {arm:?}"
        );
    }

    #[test]
    fn cover_ok_unused_opt_prior_does_not_retreat() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 12_000);
        for pair in [
            (4, 31),
            (31, 66),
            (66, 67),
            (67, 69),
            (69, 70),
            (70, 93),
            (93, 96),
            (96, 103),
            (103, 115),
            (115, 131),
            (131, 132),
            (132, 135),
            (135, 138),
            (138, 141),
            (141, 166),
            (166, 171),
        ] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.measured = true;
            e.decision = LocStrategy::win(2);
            e.w_star = 2;
            e.w_need = 2;
            e.last_cover_ok = true;
            e.last_sys_reexec = false;
            e.last_double_pay = false;
            e.last_crisis = false;
            // Quiet cover: abort EMA has dropped below measured Win_2.
            e.reexec_ns_ema = 8_000.0;
            e.upsert_stat(LocStrategy::win(2), 36_828.0, 3.0);
            // Opt stays on the unused 25k prior (n=1).
        }
        p.block_arm.clear();
        let (arm, explore, _) = p.select_arm(0x32be, 16);
        assert!(!explore, "L4: proven cover must not UCB-explore unused Opt");
        assert!(
            is_covering(arm, 16, p.seg_cap(), 2),
            "L4: unused Opt prior must not retreat off proven Win_2, got {arm:?}"
        );
        assert!(
            !matches!(arm, LocStrategy::OptimisticRead | LocStrategy::DeferPlant),
            "L4: cover_ok + unused Opt prior stays ordered, got {arm:?}"
        );
    }

    #[test]
    fn win1_leftover_does_not_grow_train_hat() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 12_000);
        for pair in [
            (4, 31),
            (31, 66),
            (66, 67),
            (67, 69),
            (69, 70),
            (70, 93),
            (93, 96),
            (96, 103),
            (103, 115),
            (115, 131),
            (131, 132),
            (132, 135),
            (135, 138),
            (138, 141),
            (141, 166),
            (166, 171),
        ] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.remember_arm(0x32be, LocStrategy::win(1));
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 2;
            e.measured = true;
            e.decision = LocStrategy::win(1);
            e.w_star = 1;
            e.w_need = 0;
            e.last_sys_reexec = false;
            e.upsert_stat(LocStrategy::win(1), 40_000.0, 2.0);
        }
        for _ in 0..14 {
            p.bump_unfenced_reexec();
        }
        p.note_reexec_ns_at(Some(0x32be), 20_000);
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x32be).unwrap();
            assert!(
                !e.last_double_pay,
                "O: first Win_1 leftover is not train_hat double-pay"
            );
            assert!(
                (e.w_need as usize) <= 2,
                "O: Win_1 leftover must not climb to train_hat, got {}",
                e.w_need
            );
        }
    }

    #[test]
    fn leftover_slide_off_when_cover_ok() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.decision = LocStrategy::win(2);
            e.w_need = 2;
            e.last_cover_ok = true;
            e.last_sys_reexec = false;
            e.last_double_pay = false;
            e.last_crisis = false;
        }
        assert!(
            !p.leftover_slide_ok(0x32be),
            "O: proven cover must not T3-slide leftover Detect hops"
        );
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.last_cover_ok = false;
            e.last_sys_reexec = false;
            e.last_crisis = false;
            e.decision = LocStrategy::win(2);
            e.w_need = 2;
        }
        assert!(
            !p.leftover_slide_ok(0x32be),
            "O: planted Win_2 must not T3-slide on the same block"
        );
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.last_cover_ok = false;
            e.last_sys_reexec = true;
        }
        assert!(
            p.leftover_slide_ok(0x32be),
            "T3: still-leaking loc may slide leftover hops"
        );
    }

    #[test]
    fn train_hat_exceeds_oversub_light_hat() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        assert_eq!(p.light_hat(8), 2, "176@8 first cover stays Win_2-class");
        assert_eq!(
            p.train_hat(8),
            7,
            "U: leftover-train ceiling is cores-scaled ∩ n_pairs−1, not hat=2"
        );
        assert_eq!(
            p.train_hat(16),
            8,
            "U: longer spine can grow to cores, still not full cover"
        );
        p.begin_block_with_cores(800, 8);
        assert!(
            p.train_hat(8) <= 4,
            "S/O: fat block train hat stays light, got {}",
            p.train_hat(8)
        );
    }

    #[test]
    fn lazy_loc_never_plants_ordered_admit() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(800, 8);
        p.note_loc_write(0x1a2e, true);
        p.promote_short_edge(0x1a2e, 40_000);
        for i in 0..40 {
            p.note_short_pair(0x1a2e, 100 + i, 101 + i);
        }
        assert!(
            p.loc_forbids_ordered(0x1a2e),
            "C1: basic_lazy is not an OrderedAdmit object"
        );
        assert_eq!(
            p.hops_to_plant(0x1a2e, 40),
            0,
            "C1: lazy hops_to_plant is 0"
        );
        let cands = p.generate_arms(0x1a2e, 40, false, true, true);
        assert!(
            cands.iter().all(|a| !a.is_ordered()),
            "L1/L3: lazy candidates are Opt/Defer only, got {cands:?}"
        );
        let (arm, _, _) = p.select_arm(0x1a2e, 40);
        assert!(
            !arm.is_ordered(),
            "C1: select_arm must not pick Win/Full on lazy, got {arm:?}"
        );
        assert_eq!(p.loc_strategy(0x1a2e, 40), LocStrategy::OptimisticRead);
    }

    #[test]
    fn sys_reexec_on_lazy_does_not_promote_win() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(800, 8);
        p.note_loc_write(0x1a2e, true);
        p.promote_short_edge(0x1a2e, 40_000);
        for i in 0..40 {
            p.note_short_pair(0x1a2e, 100 + i, 101 + i);
        }
        p.remember_arm(0x1a2e, LocStrategy::OptimisticRead);
        {
            let mut e = p.promoted.get_mut(&0x1a2e).unwrap();
            e.object = LocObject::Lazy;
            e.samples = 3;
            e.measured = true;
            e.decision = LocStrategy::OptimisticRead;
            e.last_sys_reexec = false;
        }
        for _ in 0..8 {
            p.bump_unfenced_reexec();
        }
        p.note_reexec_ns_at(Some(0x1a2e), 50_000);
        p.note_reexec_ns_at(Some(0x1a2e), 50_000);
        p.end_block_learn();
        {
            let e = p.promoted.get(&0x1a2e).unwrap();
            assert!(
                !e.last_sys_reexec,
                "C1/L2: sys-reexec on lazy must not reopen Win"
            );
        }
        let (arm, _, _) = p.select_arm(0x1a2e, 40);
        assert!(
            !arm.is_ordered(),
            "C1: after unfenced train, lazy stays Opt/Defer, got {arm:?}"
        );
    }

    #[test]
    fn real_basic_spine_still_covers() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.note_loc_write(0x32be, false);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.object = LocObject::Basic;
            e.saw_effective = true;
            e.samples = 5;
            e.decision = LocStrategy::win(2);
            e.w_star = 2;
            e.last_cover_ok = true;
            e.upsert_stat(LocStrategy::win(2), 10_000.0, 5.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 80_000.0, 3.0);
        }
        assert!(
            !p.loc_forbids_ordered(0x32be),
            "C2: real Basic is still an OrderedAdmit object"
        );
        let (arm, _, _) = p.select_arm(0x32be, 4);
        assert_eq!(
            arm,
            LocStrategy::win(2),
            "C2: 3356896-class Basic keeps light cover, got {arm:?}"
        );
        assert!(p.hops_to_plant(0x32be, 4) >= 2);
    }

    #[test]
    fn long_storage_never_full_chain() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(800, 8);
        p.note_loc_write(0x571, false);
        p.promote_short_edge(0x571, 40_000);
        for i in 0..80 {
            p.note_short_pair(0x571, 10 + i, 11 + i);
        }
        {
            let mut e = p.promoted.get_mut(&0x571).unwrap();
            e.object = LocObject::Storage;
            e.saw_effective = true;
            e.last_sys_reexec = true;
        }
        let cands = p.generate_arms(0x571, 80, false, true, true);
        assert!(
            !cands.iter().any(|a| *a == LocStrategy::FullChain),
            "C3: ultra-long storage must not offer Full, got {cands:?}"
        );
        let (arm, _, _) = p.select_arm(0x571, 80);
        assert_ne!(
            arm,
            LocStrategy::FullChain,
            "C3: select_arm must not Full(571)-class, got {arm:?}"
        );
    }

    #[test]
    fn morph_does_not_inherit_win_onto_lazy() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        p.note_loc_write(0x32be, false);
        p.promote_short_edge(0x32be, 40_000);
        for pair in [(4, 31), (31, 66), (66, 67), (67, 69)] {
            p.note_short_pair(0x32be, pair.0, pair.1);
        }
        p.remember_arm(0x32be, LocStrategy::win(2));
        {
            let mut e = p.promoted.get_mut(&0x32be).unwrap();
            e.samples = 4;
            e.w_star = 2;
            e.object = LocObject::Basic;
            e.upsert_stat(LocStrategy::win(2), 11_000.0, 4.0);
            e.upsert_stat(LocStrategy::OptimisticRead, 90_000.0, 3.0);
        }
        p.end_block_learn();
        p.begin_block_with_cores(800, 8);
        p.note_loc_write(0xfeed, true);
        p.ensure_promoted_seeded(0xfeed, 40);
        let seeded = p.promoted.get(&0xfeed).unwrap();
        assert!(
            seeded.stat(LocStrategy::win(2)).is_none(),
            "L1: lazy loc must not inherit Win_2 morph from Basic"
        );
        let (arm, _, _) = p.select_arm(0xfeed, 40);
        assert!(
            !arm.is_ordered(),
            "L1: lazy after morph seed stays unordered, got {arm:?}"
        );
    }

    #[test]
    fn fat_empty_to_is_a0() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(800, 8);
        let addr = Address::repeat_byte(0x20);
        assert!(
            !p.choose_ordered(CohortKind::EmptyTo, addr, 64, true, 0.5),
            "C1/P2: fat EmptyTo contract payee is A0 (lazy star)"
        );
        for _ in 0..4 {
            p.note_eff_waw(CohortKind::EmptyTo, addr, true);
        }
        assert!(
            !p.choose_ordered(CohortKind::EmptyTo, addr, 64, true, 0.5),
            "C1: fat EmptyTo stays A0 even after envelope eff-WAW (real spine is C2)"
        );
    }

    #[test]
    fn fat_begin_hole_cap_is_light() {
        let p = CostPolicy::new();
        p.begin_block_with_cores(176, 8);
        assert_eq!(p.fat_begin_hole_cap(), usize::MAX, "thin has no hole cap");
        p.begin_block_with_cores(800, 8);
        let cap = p.fat_begin_hole_cap();
        assert!(
            (8..=16).contains(&cap),
            "P2: fat begin hole cap is soft 8–16, got {cap}"
        );
    }
}
