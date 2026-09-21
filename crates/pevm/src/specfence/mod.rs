//! SpecFence Parallel Spine (SF-PS) — Detect → RunnableSet → Schedule.pick →
//! Execute(vis) → Validate.to_resolve → Resolve.apply → Learn.
//!
//! SoT: `lab/notes/specfence-first-class-architecture-redesign-v1.md`
//! and `lab/notes/specfence-sf-ps-full-land-v1.md`.
//! Live vocabulary: `lab/notes/specfence-cc-glossary.md`
//! (`OptimisticRead` / `OrderedAdmit` / `refuse_admit` / `wait_for_dependency` /
//! `partial_abort` / `full_abort_reexecute` plus `RunnableSet` /
//! `VisibilityPolicy` / `ResolvePlan`).
//! Triple PC⊗CC⊗Learn = analysis lens, **not** folder kingdoms.
//!
//! `ConcurrencyMode::Occ` is pristine Block-STM (**zero** SpecFence ticks) —
//! contrast engine only.
//! `ConcurrencyMode::SpecFence` is a **different protocol**: ready = Detect
//! antichain ∪ released dependents on real `Q_indep`/`Q_released`/`Q_ordered`;
//! read = `SfMvMemory.read(ℓ, vis)`; validate produces ResolvePlan and
//! `ResolvePlan.apply` mutates certificates/queues. Independent txs may use
//! Opt visibility (Avoid=noop). SpecFence **must not** call
//! `Scheduler::next_task` / wave-ready next-task / OCC-stage validate.
//!
//! Soft=0. seq≡par. Lazy-update / near-independent are never OrderedAdmit
//! objects. Thin (n≤176) must not learn Win_8. Under-covered spines must
//! not leak Full as success.
//!
//! Frozen π: \(a=(t,k,\mathrm{depth},ℓ,\mathrm{mode})\) + \(e_{\mathrm{vis}}\) +
//! gate `PredictedEssential(ℓ,k,morph) ∨ independence_certified`.
//! `inc` / ForcePrefix / canary / H-OR / morph actuator are **not** Avoid keys.
//!
//! Historical (superseded grain):
//! Authoritative v2: `lab/notes/specfence-complete-architecture-v2.md`.
//! Landed map v2: `lab/notes/specfence-architecture-v2-impl.md`.
//! Family: preset-order hybrid OCC + ahead sketch + ordered admission +
//! early-visible OrderedAdmit + first-wave Avoid + piece-restricted resolve.
//!
//! # Single algorithm (hot path)
//! ```text
//! Inter prior → H + chain templates (A6)   # warm-start; decay on flip
//! Block-STM scheduler + MvMemory           # L0 work-conserving
//!     ↑
//! choose_edge_action (A1–A4, D6)           # OrderedAdmit Data / WaitFor essential / OptimisticRead indep
//!                                          # Region is the control unit; OptimisticRead is the OCC-cost verb
//!     ↑
//! First-wave Avoid on publish (A2)         # not block-end EMA
//!     ↑
//! Resolve partial_abort → rewind → full_abort_reexecute (A5)
//! ```
//!
//! **Avoid (A):** unfinished writer on hot program ℓ → BlockingOther prefer-steal
//! Await until Executed/Validated, then OrderedAdmit. Fence intent at first-cross `a`
//! records `armed_at_k`. SoftWait Soft stays ~0 unless wake EV re-proven.
//!
//! **Resolve (B):** RebindOnly when value-stable (Iter6: Estimate→Data spin +
//! multi-origin Basic snap / prior match); else SuffixRepair + journal FF.
//! Escalate FullAbortReexecute after depth≥2 when RewindTo/FF armed (one extra
//! SuffixRepair vs classic fb escalate-at-1; depth≥3 measured wall↑). Iter7:
//! after first SuffixRepair fail, sticky BO Await on fail locs (+ force_ordered_admit
//! extend) until writers Validated before 2nd resume. Iter10: strip Iter9 hot-path
//! tax (stock SSTORE unless plant install wanted; no first_k-from-gas on finalize).
//! Iter11: multi-SSTORE last-tip + k<k_fail snap select; jump/capture OFF.
//! Iter12: Validated-strict spin on 2nd-repair prefer_await (no park tax);
//! force_prefix ESTIMATE→BO when writer Executing; doomed-2nd-repair escalate to
//! serial-barrier when ESTIMATE/Aborting∧Executing spine; longer RebindOnly spin
//! on force_ordered_admit/ff_head. Abort-path Validated evidence spins falsified (wall↑).
//! Iter13: Validated-gated Storage+Basic value-stable journal FF (origin bump,
//! same value; Iter10 bare value-stable falsified); serial-barrier multi-candidate
//! claim (no sibling park; Executed→Validated escalate spin falsified). Jump/
//! capture OFF (single-SSTORE env trial hung). SoftWait Soft ~0.
//! Iter14: schedule-side first-SuffixRepair Await behind Executing conflict writer
//! (prevent doomed first repair; Estimate park falsified Iter8) + RebindOnly
//! Validated collapse spin (Executed tip→Validated). Keep Iter12 2nd-repair +
//! Validated-gated FF. Jump/capture OFF. SoftWait Soft ~0.
//! Iter15: fan-out FR collapse — first-fail true_suffix + fan≥8 Executing spine
//! → escalate+serial-barrier (fr↓; SoftWait Soft=0). Widen storm serial-barrier;
//! longer true_suffix Validated RebindOnly spin. Pre-abort drain / BO-OR Await
//! falsified. Keep Iter12–14. Jump/capture OFF. SoftWait Soft ~0.
//! Iter16: cheap absorb — !true_suffix validate-defer behind Executing spine
//! (RebindOnly-after-spine, no invalidate); true_suffix SuffixRepair+fra absorb
//! instead of early FullAbortReexecute (FR collapse reserved for Estimate/Aborting
//! doomed spines). Keep Iter12–15 barrier widen / vs-spin / fra. Jump/capture
//! OFF. SoftWait Soft ~0. No 15b drain / 15c BO-OR.
//! Iter17: schedule Await before OptimisticRead via yield-spin on storm+program
//! live_fanout≥8 unfinished writers (no BO park). Falsified: 17a/f BO park,
//! 17b true_suffix defer, 17d long RebindOnly wait, 17g cut vs_spin. Keep
//! Iter16 absorb-no-sticky. Jump/capture OFF. SoftWait Soft ~0.
//! Iter18: diagnose hang-free opcode skip on successful SuffixRepair. Falsified:
//! Handler plant+capture (hsstore>0, aj=0 — SSTORE snaps at k≥k_fail on RAW-read
//! fails); synthetic mid RewindTo; late-k yield192; ForceOrderedAdmit/park ff_head seed
//! (599 wall↑). Production remains Iter17 tip. Jump/capture OFF. SoftWait Soft ~0.
//! Iter19: hang-free OrderedAdmit/SLOAD snap at certified-prefix end (not post-SSTORE).
//! Opt-in `SPECFENCE_BIND_SNAP=1` → bsnap>0 with k<k_fail on 597 RAW-read fails.
//! Absolute jump (`SPECFENCE_BIND_SNAP_JUMP=1`) hung Lean fixtures — production OFF.
//! Capture-without-jump wall↑/599↑ — default capture OFF. Keep Iter17 yield-spin +
//! Iter16 absorb; stock SSTORE; SoftWait Soft ~0.
//! Iter20: hang-free OrderedAdmit-snap *consume* diagnosis. Iter19 `!memory_lite_ok` left
//! aj=0 on mainnet (OrderedAdmit snaps clone memory). Fixing the gate + Validated-safe
//! origin seed still **hangs 597** once Storage-FF OrderedAdmit jump arms; Basic-only tips
//! refuse (`bytecode_no_storage_ff`). Abs jump stays hard-OFF; hang-free credit
//! consume (`bcredit`) when OrderedAdmit tip on resume. SNAP opt-in; production OFF.
//! SoftWait Soft ~0. Stock SSTORE. No mega-fan yield.
//! Iter21: minimal Storage-FF OrderedAdmit jump hang repro. Env-gated JUMP arm
//! (`SPECFENCE_BIND_SNAP_JUMP=1`); matching MvMemory origins must be Validated;
//! clear stale PENDING_RESUME in `with_ordered_admit_snap_tls`. Prove width=1 seq≡par
//! then width≥2 hang-free before concurrency enable. Production SNAP/JUMP OFF;
//! SoftWait Soft ~0. Keep Iter16–17 absorb/yield-spin.
//! Iter22: OrderedAdmit-jump restore — defer FF origin seed until after successful
//! `apply_to_interp`; warm FF Storage/Basic in journal (EIP-2929); matching-origin
//! value check; refuse truncated-memory tips. Dig for ERC-20 aj>0∧seq≡par; JUMP
//! stays OFF under concurrency until stable + 597 no-hang. SoftWait Soft ~0.
//! Iter23: diff-first OrderedAdmit-jump vs cold SuffixRepair — tip_sloads log; refuse
//! jump when OrderedAdmit SLOAD ≠ FF (stale consumed into require/SUB → ERC-20 revert
//! dgas=+661); journal warm prefer_tx=min. Non-jump: high-fan (≥32) first-repair
//! pre-yield skip-park (cut park_ms). Dig aj>0∧fail=0 + 597 SNAP+JUMP no-hang;
//! production stayed OFF (mass SNAP tax). SoftWait Soft ~0. Keep Iter16–17.
//! Iter24: cautious OrderedAdmit-jump enable — `SPECFENCE_BIND_SNAP=resume` ResumePath
//! SNAP (capture only on SuffixRepair resume / force_ordered_admit / needs_live_capture;
//! not every Handler run) + JUMP follows with refuse-if-stale. Mass=`=1`. SoftWait Soft ~0.
//! Iter25: silent-default ResumePath (Mass JUMP was Lean hang — ResumePath+refuse
//! hang-free + Lean seq≡par). tip_sloads skip of all-prefix Validated spin
//! falsified (Lean p2 seq≠par); broad inc>0 SNAP falsified (tax, aj=0). SoftWait Soft ~0.
//! Iter26: Validated-fresh tip→jump — arm OrderedAdmit-snap on FF-served SLOAD (tip≡FF);
//! Validated-only OrderedAdmit-on-Data; prefer tip_sloads≡FF at jump_snap select; keep
//! all-prefix Validated spin (no tip_sloads skip). SoftWait Soft ~0.
//! Iter27: tip≡FF overlap (extras OK) + steps_cap select + deeper all-prefix
//! Validated spin (no tip_sloads skip / nested apply hang falsified). SoftWait Soft ~0.
//! Iter28: diagnose 597 first-frame tip identity (router→token nested OrderedAdmit tips;
//! nested apply / defer-until-match hung) + LAST_SNAP TLS clear. SoftWait Soft ~0.
//! Iter29: hang-free nested OrderedAdmit consume ≠ frame_init defer — stash+natural CALL
//! apply dig hang-free; Iter29 default-on was Lean seq≠par — stayed opt-in then.
//! Iter30: Lean-safe nested apply **default-on** — tip_sloads addr≡target ∧
//! depth≤2 ∧ tip≡FF only (opt-out `SPECFENCE_NESTED_BIND=0`); multi-addr / deep
//! tips still credit. SoftWait Soft ~0. Keep tip≡FF + steps_cap + LAST_SNAP.
//! Keep Iter16–17/23/24/25/27/28/29.
//! Iter8 memory snap retained. Head-FF (Iter5). SoftWait Soft ~0.
//!
//! **Three-pillar (default-on):**
//! 1. Await@a — storm+program+live_fanout≥8 unfinished writer → BO until done +
//!    Validated yield-spin, then OrderedAdmit. SoftWait Soft stays ~0. Escape:
//!    `SPECFENCE_DISABLE_AWAIT_AT_A=1`.
//! 2. Resolve ≠ FullAbortReexecute — ResumePath OrderedAdmit tips; tip≡FF max_steps 8192;
//!    best deferred tip; Lean-safe nested apply; refuse unsafe jumps.
//! 3. Morph mode — Quiet OCC-lite vs Storm Await-ready from inter morph + live
//!    fanout flip; Await/choose_action only on hot candidates.
//!
//! **Learn (C):** Inter morph selects Quiet (598 OCC-lite) vs Storm (597 Await-ready).
//! Intra `choose_action` / learner updates only on hot candidates.
//!
//! # Shoveled off SpecFence control (V5-P0)
//! - Heat / `seed_wait_regions` SoftWait arming (PCC may still seed account Wait)
//! - Account Wait / `promote_account` (diagnostic stub; always false)
//! - `RegionTable` Wait as decision authority (mirrors only; FenceGraph SoT)
//! - Bayes `should_wait_hard` Boolean second π (Beta posteriors = EV features)
//! - AdaptiveEngagement abort_rate mode ladders (always Lean execute)
//! - HotSet as Wait gate (`H_w`/`H_a` = optional dense-stat / fanout features)
//!
//! # Research-only (not default behavior) — V5-P3
//! Inspect / absolute jump / CallOutcome SC stay behind `SPECFENCE_ENABLE_INSPECT=1`.
//! M1a–M1l + [`research_apply_abort_repair`] remain research; **not** graduated
//! (A/B: inspect hangs on 597 path). Lean SoftWait wake may still arm hang-free
//! journal FF via `try_arm_park_resume_at_k` (no absolute jump).
//! Finegrain collectors are lab opt-in.
//!
//! Correctness shield: cascade fence + Block-STM validate / ESTIMATE unchanged.
//! Learning never commits. SpecFence ≡ sequential on Ethereum fixtures.

use crate::{
    BuildSuffixHasher, MemoryLocation, TxIdx, chain::PevmChain, hash_deterministic,
    scheduler::Scheduler,
};
use alloy_primitives::Address;
use hashbrown::HashMap;

mod access_log;
mod arm_table;
mod access_policy;
mod access_vis;
pub(crate) mod admit;
mod bayes;
mod boundary;
mod certificate;
mod collateral;
mod computer;
mod dag;
mod decision_field;
mod edge;
mod engagement;
mod executor;
pub(crate) mod feeder;
#[allow(missing_docs)]
mod finegrain;
mod heat;
mod hotset;
#[cfg(test)]
mod kernel;
mod lane;
mod learner;
mod metrics;
pub(crate) mod ordered_admit_act;
mod policy;
mod prior;
mod process;
mod producer_stage;
mod ready_edge;
mod region;
mod rem;
mod repair;
mod resolve;
mod resolve_plan;
mod runnable_set;
mod schedule;
mod sf_mv;
mod sketch;
mod visibility;
mod wave;
mod worker;

pub(crate) use access_log::AccessOrdinalLog;
pub(crate) use access_policy::{
    AccessDecision, AccessVis, decide as decide_access, decide_queried as decide_access_queried,
};
pub(crate) use access_vis::compose_unfinished;
pub(crate) use bayes::BayesAccessQuery;
pub(crate) use bayes::{BayesMap, DEFAULT_TAU};
pub use boundary::SpecFenceInspector;
#[allow(unused_imports)]
pub(crate) use boundary::{
    BoundarySnapshot, CachedCallOutcome, JournalBlob, OrderedAdmitSnapMode, absolute_jump_eligible,
    absolute_jump_env_enabled, arm_call_outcome_cache, arm_ff_origin_seeds, arm_pc_resume,
    arm_pending_effect_cp_only, attach_current_live_snap, clear_pc_resume,
    handler_ordered_admit_snap_install_wanted, handler_sstore_protocol_install_wanted,
    in_inspect_run, install_handler_ordered_admit_snap_capture,
    install_handler_sstore_protocol_capture, jump_is_safe, jump_refuse_reason, last_boundary_snap,
    nested_ordered_admit_consume_enabled, nested_ordered_admit_stash_armed,
    note_pending_effect_boundary, note_pending_ordered_admit_snap,
    ordered_admit_snap_capture_wanted, ordered_admit_snap_env_enabled,
    ordered_admit_snap_jump_enabled, ordered_admit_snap_mode, pending_resume_armed,
    protocol_tls_active, resume_was_applied, steps_this_run, suffix_repair_jump_env_ok,
    take_ff_origin_seeds, try_apply_pending_pc_resume, try_arm_safe_absolute_jump,
    try_arm_safe_absolute_jump_gated, try_consume_nested_ordered_admit_resume,
    with_ordered_admit_snap_tls, with_protocol_tls, with_protocol_tls_journal,
};
pub(crate) use certificate::CertificateTable;
#[allow(unused_imports)]
pub(crate) use collateral::{
    ConflictClass, FirstConflict, classify_first_conflict, commute_ok, envelopes_disjoint,
    is_value_transfer, location_is_lazy, optimistic_majority_hinted_lazy,
};
pub(crate) use arm_table::ArmTable;
pub(crate) use worker::{SfExec, run_sf_block};
pub(crate) use dag::{FenceGraph, SpecDag};
pub(crate) use decision_field::{DecisionFeat, DecisionVerb};
pub use decision_field::{DecisionFieldSnap, QualityProxies, VerbHist};
pub(crate) use edge::{
    EdgeAction, EdgeKey, EdgeKind, EdgeState, EdgeTable, EdgeView, EdgeVisibility, access_k_class,
    classify_edge,
};
pub(crate) use engagement::{AdaptiveEngagement, profile_timing_enabled, research_inspect_enabled};
pub(crate) use executor::{
    fence_for_mode, hinted_wait_enabled, next_occ_task, occ_pick_calls, occ_read_set_valid,
    reset_occ_pick_calls, specfence_access_is_occ, specfence_cost_class_spec,
    specfence_partial_abort_validate, specfence_plant_is_occ, uses_specfence_resolve,
    validate_occ_kernel, validate_occ_stage, validate_optimistic_fast, validate_specfence,
    validate_to_plan, wave_for_mode,
};
pub use finegrain::{
    AbortEvent, AccountGrainObserve, ConsumerFirstCross, DagStats, EffectClass, EffectLogEntry,
    EffectStreamDiag, FineGrainCollector, FineGrainSnapshot, HotLocation, L1DagSummary,
    LocationKind, MaMdProxy, MeasurementMethod, RawEdge, RawEffectEdge, TxRw, TxWorkTotal,
    analyze_dag, classify_raw_edges, dependency_edges, effect_raw_longest_chain,
    effect_raw_max_fanout, estimate_ma_md, filter_effect_edges, hot_locations, kind_histogram,
    l1_dag_summary, percentile_f64, producer_status_canonical, program_raw_longest_chain,
};
pub(crate) use heat::HeatMap;
pub(crate) use hotset::HotSet;
#[allow(unused_imports)]
pub(crate) use hotset::{H_A, H_W};
pub use resolve_plan::ResolvePlan;
pub(crate) use runnable_set::RunnableSet;
pub(crate) use sf_mv::SfMvMemory;
pub use visibility::VisibilityPolicy;
// kernel.rs museum — tests only; rem-legal SoT is CertificateTable.
pub(crate) use lane::LaneTable;
pub(crate) use learner::{AdaptiveParams, InterBlockPrior, LiveLearner};
pub(crate) use metrics::MetricsInner;
pub use metrics::SpecFenceMetrics;
pub use policy::LearnReport;
#[allow(unused_imports)]
pub(crate) use policy::{AdmitAction, CohortKind, CostPolicy};
pub(crate) use prior::RwPriorMap;
pub(crate) use process::ProcessTrace;
pub use process::{ExecProcessSnapshot, LocProcessSnap, PerTxProcessSnap, ProcessReason};
pub(crate) use producer_stage::ProducerStageTable;
pub(crate) use ready_edge::ReadyEdgeTable;
pub use region::RegionMode;
pub(crate) use region::RegionTable;
pub(crate) use rem::PartialRetryTable;
pub(crate) use rem::RemCounters;
#[allow(unused_imports)]
pub(crate) use rem::{
    AccessMode, Checkpoint, CheckpointId, CheckpointKind, EffectOrdinal, FfValue, LeanAbortRepair,
    PartialRetryPlan, PartialRetryState, RegionAccess, RemTask, RepairPlan, ResearchAbortRepair,
    ResumeContinuation, StorageWriteReplay,
};
#[allow(unused_imports)]
pub(crate) use repair::{RepairGrain, repair_grain};
#[cfg(test)]
use resolve::choose_action;
#[allow(unused_imports)]
pub(crate) use resolve::{
    C_RETRY, COST_MARGIN, D_EARLY, D_WAIT, EvScores, OrderedAdmitTarget, SelectiveOutcome,
    TAU_REVOKE, TAU_S, TAU_VERY_HIGH, TAU_W, compute_ev, cost_prefers_wait, early_val_probability,
};
#[cfg(test)]
pub(crate) use resolve::{PolicyCtx, ResolveAction};
pub(crate) use sketch::{HotSketch, ResidualOrderedAdmit};
pub(crate) use wave::{
    ParkKind, ParkResumeIntent, ParkResumeKind, ParkedWait, PendingPark, WaveParkTable,
};

/// Selectable concurrency control for parallel block execution.
///
/// Default is current PEVM Block-STM (OCC). `SpecFence` mixes Wait and Speculate
/// in the same block; PCC waits on hinted prior writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConcurrencyMode {
    /// Block-STM optimistic concurrency. Unchanged default path.
    #[default]
    Occ,
    /// Conservative PCC: hinted `from`/`to` accounts start in Wait.
    Pcc,
    /// SpecFence complete CC: sketch + edge π + early-visible OrderedAdmit + partial_abort resolve.
    SpecFence,
}

impl ConcurrencyMode {
    /// Whether this mode uses per-region Wait/Speculate state.
    pub const fn uses_regions(self) -> bool {
        matches!(self, Self::Pcc | Self::SpecFence)
    }
}

/// Cheap `from`/`to` index: which transactions hint they touch an account.
#[derive(Debug, Default)]
pub(crate) struct AccountHints {
    by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher>,
    /// Senders — always write Basic(from) (nonce / balance).
    from_by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher>,
    /// Recipients / callees (envelope `to`).
    to_by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher>,
    /// `to` with nonempty calldata (contract calls).
    call_to_by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher>,
    /// Per-tx: true when envelope calldata is empty (lazy-transfer class).
    empty_calldata: Vec<bool>,
    /// Per-tx gas limit (CC-X1: 21000 pure transfer).
    gas_limit: Vec<u64>,
    /// Per-tx sender.
    from_of: Vec<Address>,
    /// Per-tx envelope `to`.
    to_of: Vec<Option<Address>>,
}

impl AccountHints {
    pub(crate) fn build<C: PevmChain>(chain: &C, txs: &[C::EvmTx]) -> Self {
        let mut by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher> =
            HashMap::with_hasher(BuildSuffixHasher::default());
        let mut from_by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher> =
            HashMap::with_hasher(BuildSuffixHasher::default());
        let mut to_by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher> =
            HashMap::with_hasher(BuildSuffixHasher::default());
        let mut call_to_by_account: HashMap<Address, Vec<TxIdx>, BuildSuffixHasher> =
            HashMap::with_hasher(BuildSuffixHasher::default());
        let mut empty_calldata = vec![true; txs.len()];
        let mut gas_limit = vec![0u64; txs.len()];
        let mut from_of = vec![Address::ZERO; txs.len()];
        let mut to_of = vec![None; txs.len()];
        for (idx, tx) in txs.iter().enumerate() {
            let env = chain.tx_env(tx);
            by_account.entry(env.caller).or_default().push(idx);
            from_by_account.entry(env.caller).or_default().push(idx);
            let empty = env.data.is_empty();
            empty_calldata[idx] = empty;
            gas_limit[idx] = env.gas_limit;
            from_of[idx] = env.caller;
            if let Some(to) = env.kind.to() {
                to_of[idx] = Some(*to);
                by_account.entry(*to).or_default().push(idx);
                to_by_account.entry(*to).or_default().push(idx);
                if !empty {
                    call_to_by_account.entry(*to).or_default().push(idx);
                }
            }
        }
        for map in [
            &mut by_account,
            &mut from_by_account,
            &mut to_by_account,
            &mut call_to_by_account,
        ] {
            for list in map.values_mut() {
                list.sort_unstable();
                list.dedup();
            }
        }
        Self {
            by_account,
            from_by_account,
            to_by_account,
            call_to_by_account,
            empty_calldata,
            gas_limit,
            from_of,
            to_of,
        }
    }

    pub(crate) fn accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.by_account.keys().copied()
    }

    pub(crate) fn from_accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.from_by_account.keys().copied()
    }

    pub(crate) fn to_accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.to_by_account.keys().copied()
    }

    pub(crate) fn call_to_accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.call_to_by_account.keys().copied()
    }

    pub(crate) fn writer_count(&self, address: &Address) -> usize {
        self.by_account.get(address).map(Vec::len).unwrap_or(0)
    }

    /// Consensus-order hinted txs for an account (union of from+to).
    pub(crate) fn txs(&self, address: &Address) -> &[TxIdx] {
        self.by_account
            .get(address)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Consensus-order txs with this sender.
    pub(crate) fn from_txs(&self, address: &Address) -> &[TxIdx] {
        self.from_by_account
            .get(address)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Consensus-order txs with this envelope `to`.
    pub(crate) fn to_txs(&self, address: &Address) -> &[TxIdx] {
        self.to_by_account
            .get(address)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Consensus-order contract calls to this `to`.
    pub(crate) fn call_to_txs(&self, address: &Address) -> &[TxIdx] {
        self.call_to_by_account
            .get(address)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// True when tx `idx` has empty envelope calldata (lazy-transfer class).
    #[inline]
    pub(crate) fn is_empty_calldata(&self, idx: TxIdx) -> bool {
        self.empty_calldata.get(idx).copied().unwrap_or(true)
    }

    /// True when every tx in `txs` is empty-calldata (basic_lazy sender spine).
    #[inline]
    pub(crate) fn cohort_all_empty(&self, txs: &[TxIdx]) -> bool {
        !txs.is_empty() && txs.iter().all(|&t| self.is_empty_calldata(t))
    }

    /// Empty-input value transfer (21k class; gas may be 21k or 90k).
    #[inline]
    pub(crate) fn is_value_transfer(&self, idx: TxIdx) -> bool {
        self.is_empty_calldata(idx)
    }

    /// CC-X1: gas=21000 empty-calldata transfer.
    #[inline]
    pub(crate) fn is_pure_transfer(&self, idx: TxIdx) -> bool {
        self.is_value_transfer(idx) && self.gas_limit.get(idx).copied() == Some(21_000)
    }

    #[inline]
    pub(crate) fn from_of(&self, idx: TxIdx) -> Address {
        self.from_of.get(idx).copied().unwrap_or(Address::ZERO)
    }

    #[inline]
    pub(crate) fn to_of(&self, idx: TxIdx) -> Option<Address> {
        self.to_of.get(idx).copied().flatten()
    }

    pub(crate) fn n_txs(&self) -> usize {
        self.empty_calldata.len()
    }

    #[cfg(test)]
    pub(crate) fn from_account_txs(addr: Address, txs: Vec<TxIdx>) -> Self {
        Self::from_many(vec![(addr, txs)])
    }

    #[cfg(test)]
    pub(crate) fn from_many(pairs: Vec<(Address, Vec<TxIdx>)>) -> Self {
        let mut by_account = HashMap::with_hasher(BuildSuffixHasher::default());
        let mut from_by_account = HashMap::with_hasher(BuildSuffixHasher::default());
        for (addr, txs) in pairs {
            by_account.insert(addr, txs.clone());
            from_by_account.insert(addr, txs);
        }
        let max = from_by_account
            .values()
            .flatten()
            .copied()
            .max()
            .unwrap_or(0);
        let n = max + 1;
        let mut from_of = vec![Address::ZERO; n];
        for (addr, txs) in from_by_account.iter() {
            for &t in txs {
                if t < n {
                    from_of[t] = *addr;
                }
            }
        }
        Self {
            by_account,
            from_by_account,
            to_by_account: HashMap::with_hasher(BuildSuffixHasher::default()),
            call_to_by_account: HashMap::with_hasher(BuildSuffixHasher::default()),
            empty_calldata: vec![true; n],
            gas_limit: vec![21_000; n],
            from_of,
            to_of: vec![None; n],
        }
    }

    #[cfg(test)]
    pub(crate) fn from_to_txs(addr: Address, txs: Vec<TxIdx>) -> Self {
        let mut by_account = HashMap::with_hasher(BuildSuffixHasher::default());
        let mut to_by_account = HashMap::with_hasher(BuildSuffixHasher::default());
        let max = txs.iter().copied().max().unwrap_or(0);
        by_account.insert(addr, txs.clone());
        to_by_account.insert(addr, txs.clone());
        let n = max + 1;
        let mut to_opt = vec![None; n];
        for &t in &txs {
            if t < n {
                to_opt[t] = Some(addr);
            }
        }
        Self {
            by_account,
            from_by_account: HashMap::with_hasher(BuildSuffixHasher::default()),
            to_by_account,
            call_to_by_account: HashMap::with_hasher(BuildSuffixHasher::default()),
            empty_calldata: vec![true; n],
            gas_limit: vec![21_000; n],
            from_of: vec![Address::ZERO; n],
            to_of: to_opt,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_from_and_to(from: Address, to: Address, txs: Vec<TxIdx>) -> Self {
        let mut h = Self::from_account_txs(from, txs.clone());
        let max = txs.iter().copied().max().unwrap_or(0);
        if h.empty_calldata.len() <= max {
            h.empty_calldata.resize(max + 1, true);
        }
        h.to_by_account.insert(to, txs.clone());
        h.by_account.entry(to).or_insert(txs.clone());
        let n = h.empty_calldata.len();
        if h.to_of.len() < n {
            h.to_of.resize(n, None);
        }
        if h.from_of.len() < n {
            h.from_of.resize(n, Address::ZERO);
        }
        if h.gas_limit.len() < n {
            h.gas_limit.resize(n, 21_000);
        }
        for &t in &txs {
            if t < n {
                h.to_of[t] = Some(to);
                h.from_of[t] = from;
            }
        }
        h
    }

    #[cfg(test)]
    pub(crate) fn from_two_transfers(
        from_a: Address,
        to_a: Address,
        from_b: Address,
        to_b: Address,
    ) -> Self {
        let mut h = Self::from_from_and_to(from_a, to_a, vec![0]);
        h.empty_calldata.resize(2, true);
        h.gas_limit.resize(2, 21_000);
        h.from_of.resize(2, Address::ZERO);
        h.to_of.resize(2, None);
        h.from_of[1] = from_b;
        h.to_of[1] = Some(to_b);
        h.from_by_account.entry(from_b).or_default().push(1);
        h.to_by_account.entry(to_b).or_default().push(1);
        h.by_account.entry(from_b).or_default().push(1);
        h.by_account.entry(to_b).or_default().push(1);
        h
    }

    #[cfg(test)]
    pub(crate) fn from_call_to_txs(addr: Address, txs: Vec<TxIdx>) -> Self {
        let mut by_account = HashMap::with_hasher(BuildSuffixHasher::default());
        let mut to_by_account = HashMap::with_hasher(BuildSuffixHasher::default());
        let mut call_to_by_account = HashMap::with_hasher(BuildSuffixHasher::default());
        let max = txs.iter().copied().max().unwrap_or(0);
        by_account.insert(addr, txs.clone());
        to_by_account.insert(addr, txs.clone());
        call_to_by_account.insert(addr, txs.clone());
        let n = max + 1;
        let mut to_opt = vec![None; n];
        for &t in &txs {
            if t < n {
                to_opt[t] = Some(addr);
            }
        }
        Self {
            by_account,
            from_by_account: HashMap::with_hasher(BuildSuffixHasher::default()),
            to_by_account,
            call_to_by_account,
            empty_calldata: vec![false; n],
            gas_limit: vec![100_000; n],
            from_of: vec![Address::ZERO; n],
            to_of: to_opt,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_two_call_to(
        a: Address,
        a_txs: Vec<TxIdx>,
        b: Address,
        b_txs: Vec<TxIdx>,
    ) -> Self {
        let mut h = Self::from_call_to_txs(a, a_txs);
        let max = b_txs.iter().copied().max().unwrap_or(0);
        if h.empty_calldata.len() <= max {
            let n = max + 1;
            h.empty_calldata.resize(n, false);
            h.gas_limit.resize(n, 100_000);
            h.from_of.resize(n, Address::ZERO);
            h.to_of.resize(n, None);
        }
        h.call_to_by_account.insert(b, b_txs.clone());
        h.to_by_account.insert(b, b_txs.clone());
        h.by_account.insert(b, b_txs.clone());
        for &t in &b_txs {
            if t < h.to_of.len() {
                h.to_of[t] = Some(b);
            }
        }
        h
    }

    /// Last transaction before `tx_idx` that hinted this account.
    pub(crate) fn prev(&self, address: &Address, tx_idx: TxIdx) -> Option<TxIdx> {
        let list = self.by_account.get(address)?;
        match list.binary_search(&tx_idx) {
            Ok(i) | Err(i) if i > 0 => Some(list[i - 1]),
            _ => None,
        }
    }
}

/// Shared `SpecFence` context for one block (copied into workers).
#[derive(Clone, Copy, Debug)]
pub(crate) struct SpecFenceCtx<'a> {
    pub mode: ConcurrencyMode,
    pub hints: &'a AccountHints,
    pub metrics: &'a MetricsInner,
    pub scheduler: &'a Scheduler,
    pub beneficiary: Address,
    pub bayes: &'a BayesMap,
    pub tau: f64,
    pub dag: &'a SpecDag,
    pub rem: &'a RemCounters,
    pub partial_retry: &'a PartialRetryTable,
    /// M2 wave park / ready deque (SpecFence only; unused by OCC/PCC).
    pub wave: &'a WaveParkTable,
    /// M3 process-local online WŜ/RŜ prior (OrderedAdmit-before-touch).
    pub rw_prior: &'a RwPriorMap,
    /// M4/R1 adaptive lean engagement (SpecFence only).
    pub engagement: &'a AdaptiveEngagement,
    /// R1 location-local HotSet (fanout/tracking hint only).
    pub hotset: &'a HotSet,
    /// P1 dual-horizon live learner (morph / fanout).
    pub learner: &'a LiveLearner,
    /// P1 tunable π constants.
    pub params: &'a AdaptiveParams,
    /// D5: multi-touch EdgeTable `(ℓ, reader, k, depth)`.
    pub edges: &'a EdgeTable,
    /// A1/A2/A6: hot set H + chain templates + Avoid broadcast.
    pub sketch: &'a HotSketch,
    /// Process-level Fence/OptimisticRead reason + per-ℓ timeline (lab / G7).
    pub process: &'a ProcessTrace,
    /// Spec-safe AccessOrdinalLog — true \(k\) without rem DashMap.
    pub access_log: &'a crate::specfence::AccessOrdinalLog,
    /// Per-prefix Fence certificates (not a tx-global bit).
    pub certificates: &'a crate::specfence::CertificateTable,
    /// PE unpublished-RAW ready-edges (schedule Avoid).
    pub ready_edges: &'a crate::specfence::ReadyEdgeTable,
    /// PC ProducerStage reservations (refuse-safe progress path).
    pub producer_stages: &'a crate::specfence::ProducerStageTable,
    /// SerialLane / OrderedAdmit progress tokens.
    pub lanes: &'a crate::specfence::LaneTable,
    /// Opt-in lab fine-grain OCC/RW tracer (None = disabled, zero cost).
    pub finegrain: Option<&'a crate::specfence::FineGrainCollector>,
    /// B3/B2 cost-aware A0 vs A1 policy (Soft=0).
    pub policy: Option<&'a CostPolicy>,
}

impl<'a> SpecFenceCtx<'a> {
    pub(crate) fn should_wait_account(&self, regions: &RegionTable, address: &Address) -> bool {
        if !self.mode.uses_regions() || *address == self.beneficiary {
            return false;
        }
        if self.mode == ConcurrencyMode::Pcc {
            return true;
        }
        // V5-P0: SpecFence conflict key = MemoryLocation only. Account Wait is
        // a diagnostic stub — never schedules SoftWait / never promotes.
        let _ = (regions, address);
        false
    }

    /// PCC sticky Wait probe. SpecFence v5: **always false**.
    ///
    /// SpecFence Wait is decided only by `choose_action` → FenceGraph SoftWait
    /// inside `Vm::maybe_wait`. Bayes Bool / RegionTable sticky / account Wait
    /// must not OR into SpecFence π (V5-P0 shovel).
    pub(crate) fn should_wait_location(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: &Address,
    ) -> bool {
        if !self.mode.uses_regions() || *address == self.beneficiary {
            return false;
        }
        if self.mode == ConcurrencyMode::Pcc {
            return true;
        }
        // SpecFence: diagnostic no-op. SoftWait arms only via choose_action.
        let _ = (regions, location, address);
        false
    }

    /// Retired AEC EV π — **not live**. Museum for resolve tests only.
    #[cfg(test)]
    pub(crate) fn choose_resolve(
        &self,
        location: crate::MemoryLocationHash,
        address: &Address,
        writer: Option<TxIdx>,
        writer_done: bool,
        ordered_admit_version: Option<crate::TxVersion>,
        residual_predicts: bool,
        prior_ws_predicts: bool,
        is_program: bool,
        fanout_hint: bool,
        live_fanout: f64,
        gross_work_depth: Option<f64>,
        waw_spine_hint: bool,
        tx_heavy_hint: bool,
    ) -> ResolveAction {
        let posterior_conflict = self.bayes.conflict_probability(location, Some(address));
        let posterior_ordered_admit = self
            .bayes
            .ordered_admit_useful_probability(location)
            .max(self.rw_prior.write_confidence(location));
        // M3: residual / process prior makes a published version a OrderedAdmit placeholder.
        let prior = residual_predicts || prior_ws_predicts;
        let morph_weights = self.learner.morph_weights();
        let e_wait_time = self.learner.e_wait_time(location);
        let e_cascade = self.learner.e_cascade(location);
        let e_reexec = self.learner.e_reexec();
        let e_idle_steal = self.learner.e_idle_steal();
        let meta_tax = self.learner.meta_tax_ratio(self.params);
        let meta_budget_exceeded = self.learner.meta_budget_exceeded(self.params);
        // G3: pass published Data version into π even when writer not yet is_done —
        // choose_action decides OrderedAdmit via prior_ws / high P / placeholder_ready.
        let ctx = PolicyCtx {
            location,
            writer_known: writer.is_some() || ordered_admit_version.is_some(),
            writer,
            writer_done,
            posterior_conflict,
            posterior_ordered_admit_success: posterior_ordered_admit,
            placeholder_ready: prior && (writer_done || ordered_admit_version.is_some()),
            ordered_admit_version,
            prior_ws_predicts: prior,
            is_program,
            fanout_hint,
            live_fanout,
            e_wait_time,
            e_cascade,
            e_reexec,
            e_idle_steal,
            meta_tax,
            meta_budget_exceeded,
            gross_work_depth,
            morph_weights,
            waw_spine_hint,
            tx_heavy_hint,
            sticky_resolve: self.learner.is_sticky_resolve(location),
            params: *self.params,
        };
        let action = choose_action(ctx);
        match &action {
            ResolveAction::WaitHard => {
                self.metrics.record_cost_chose_wait();
                self.learner.note_meta_op();
                if is_program {
                    self.metrics.record_cost_chose_wait_program();
                } else {
                    self.metrics.record_cost_chose_wait_handler();
                }
                self.bayes
                    .note_cost_decision_posterior(posterior_conflict, true);
            }
            ResolveAction::EarlyAbort => {
                // EarlyAbort niche — count as wait-side cost choice + early_abort.
                self.metrics.record_cost_chose_wait();
                if is_program {
                    self.metrics.record_cost_chose_wait_program();
                } else {
                    self.metrics.record_cost_chose_wait_handler();
                }
                self.metrics.record_early_abort();
                self.bayes
                    .note_cost_decision_posterior(posterior_conflict, true);
            }
            ResolveAction::OptimisticRead => {
                self.metrics.record_cost_chose_optimistic_read();
                if is_program {
                    self.metrics.record_cost_chose_optimistic_read_program();
                } else {
                    self.metrics.record_cost_chose_optimistic_read_handler();
                }
                self.bayes
                    .note_cost_decision_posterior(posterior_conflict, false);
            }
            ResolveAction::OrderedAdmit(_) => {
                self.metrics.record_cost_chose_ordered_admit();
            }
        }
        action
    }

    /// Proactive PCC / cold-start: previous hinted writer that has not finished.
    pub(crate) fn wait_blocker(
        &self,
        regions: &RegionTable,
        address: &Address,
        tx_idx: TxIdx,
    ) -> Option<TxIdx> {
        if !self.should_wait_account(regions, address) {
            return None;
        }
        let prev = self.hints.prev(address, tx_idx)?;
        if self.scheduler.is_done(prev) {
            None
        } else {
            Some(prev)
        }
    }

    /// Promote location Wait **mirror** (FenceGraph SoftWait remains authority for arms).
    /// Abort/conflict densifies tracking; does not arm SoftWait by itself.
    pub(crate) fn promote_from_bayes(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<Address>,
    ) -> bool {
        let promoted = regions.promote_location(location);
        // Mirror only — SoftWait arms come from choose_action→WaitHard→arm_soft.
        let _ = self.dag.set_wait(location);
        if promoted {
            self.metrics.record_promotion(address);
            self.metrics.record_wave_promotion();
            self.bayes.bump_wave();
            true
        } else {
            false
        }
    }

    /// Unified SoftWait revoke: τ_revoke and/or morph quiet|waw.
    pub(crate) fn try_revoke_unified(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<&Address>,
    ) -> bool {
        let morph = self.learner.morph_weights();
        let morph_revoke = morph.dominant_quiet() || morph.dominant_waw();
        let bayes_revoke = self.bayes.should_revoke(location, address);
        if !bayes_revoke && !morph_revoke {
            return false;
        }
        // Only morph-revoke SoftWaits when posterior is also not insisting on Wait,
        // or when quiet/waw dominates (schedule/steal ≫ sticky Wait).
        if morph_revoke || bayes_revoke {
            let cleared_region = regions.clear_location_wait(location);
            let cleared_fence = self.dag.clear(location) > 0 || self.dag.clear_wait(location);
            if morph.dominant_quiet() {
                let n = self.sketch.revoke_prior_fences_if_quiet(true);
                self.metrics.record_quiet_pessimistic_revoke(n);
            }
            if cleared_region || cleared_fence {
                self.metrics.record_soft_edge_revoke();
                return true;
            }
        }
        false
    }

    /// Attempt revoke of sticky Wait when posterior dropped (delegates to unified).
    pub(crate) fn try_revoke(
        &self,
        regions: &RegionTable,
        location: crate::MemoryLocationHash,
        address: Option<&Address>,
    ) -> bool {
        self.try_revoke_unified(regions, location, address)
    }
}

/// Seed account Wait for **PCC only**.
///
/// SpecFence v5: this is a **no-op** for SoftWait / Region Wait. Heat and Bayes
/// priors must never arm SoftWait at block start (V5-P0 shovel). Callers should
/// gate on `ConcurrencyMode::Pcc` (see `pevm.rs`).
pub(crate) fn seed_wait_regions(
    regions: &RegionTable,
    hints: &AccountHints,
    bayes: &BayesMap,
    mode: ConcurrencyMode,
    beneficiary: Address,
    tau: f64,
    initial_wait: &mut std::collections::HashSet<Address>,
) {
    if mode != ConcurrencyMode::Pcc {
        let _ = (regions, hints, bayes, beneficiary, tau, initial_wait);
        return;
    }
    for address in hints.accounts() {
        if address == beneficiary {
            continue;
        }
        let _ = (bayes, tau);
        regions.seed_account_wait(address);
        regions.promote_location(hash_deterministic(MemoryLocation::Basic(address)));
        initial_wait.insert(address);
    }
}

/// Apply bounded EWMA updates (PCC / legacy heat path). SpecFence uses Bayes.
pub(crate) fn update_heat(
    heat: &HeatMap,
    hints: &AccountHints,
    metrics: &MetricsInner,
    beneficiary: Address,
) {
    for address in hints.accounts() {
        if address != beneficiary && hints.writer_count(&address) >= 2 {
            heat.observe(address);
        }
    }
    for address in metrics.hot_accounts() {
        if address != beneficiary {
            heat.observe(address);
        }
    }
}

/// End-of-block Bayesian maintenance for SpecFence.
pub(crate) fn update_bayes(bayes: &BayesMap) {
    bayes.decay_block();
}

/// End-of-block RW prior decay (M3).
pub(crate) fn update_rw_prior(rw_prior: &RwPriorMap) {
    rw_prior.decay_block();
}
