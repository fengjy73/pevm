//! Test-visible `SpecFence` counters. Updated atomically during a block.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use alloy_primitives::Address;
use dashmap::DashMap;

use crate::BuildSuffixHasher;

/// Snapshot of `SpecFence` counters after a parallel block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpecFenceMetrics {
    /// Times a transaction was blocked because a Wait-mode region was not yet
    /// written by the prior consensus-order writer (proactive PCC admission).
    pub wait_admissions: usize,
    /// Successful executions that ran against Speculate (OCC) hinted accounts.
    pub speculate_executions: usize,
    /// Intra-block Speculate → Wait promotions (wave updates).
    pub region_promotions: usize,
    /// Validation aborts (OCC / SpecFence / PCC).
    pub occ_aborts: usize,
    /// Higher txs forced into the validation cascade by an abort rewind
    /// (`block_size - rewind_to` when a dependent reader exists).
    pub cascade_validations_scheduled: usize,
    /// Higher txs between `aborted_idx+1` and the first dependent reader that
    /// were **not** forced into the abort cascade (SpecFence fence).
    pub independent_txs_skipped_by_fence: usize,
    /// Bayesian decide() chose Wait for a location/account access.
    pub bayes_wait_decisions: usize,
    /// Bayesian decide() chose Speculate.
    pub bayes_speculate_decisions: usize,
    /// `observe_conflict` updates applied.
    pub bayes_conflict_updates: usize,
    /// `observe_speculate_ok` updates applied.
    pub bayes_success_updates: usize,
    /// Wave counter bumps when regions flip Speculate→Wait from bayes.
    pub wave_promotions: usize,
    /// Final wave id after the block.
    pub wave_id: usize,
    /// Mean conflict posterior among Wait decisions this block.
    pub mean_wait_posterior: f64,
    /// Accounts that triggered a Wait admission in this block.
    pub wait_addresses: Vec<Address>,
    /// `from`/`to` accounts of speculative executions in this block.
    pub speculate_addresses: Vec<Address>,
    /// Per-location validation failures.
    pub region_validate_fail: usize,
    /// FullAbortReexecute (whole-tx re-exec) counts.
    pub tx_full_abort_reexecute: usize,
    /// OrderedAdmit hits (read matched predicted writer).
    pub ordered_admit_hits: usize,
    /// WaitHard decisions / admissions at location grain.
    pub wait_hard_count: usize,
    /// Legacy AEC OptimisticRead path counts (OCC-cost verb; Region is the control unit).
    pub optimistic_read_count: usize,
    /// Selective invalidate applications.
    pub selective_invalidate_count: usize,
    /// Cascade revalidations scheduled (alias tracking for Spec v1 metrics).
    pub cascade_revalidate_count: usize,
    /// Soft Wait flags cleared because posterior < τ_revoke.
    pub soft_edge_revokes: usize,
    /// Times selective invalidate fell back to full write-set ESTIMATE.
    pub selective_fallback_full: usize,
    /// Checkpoint opportunities recorded (Phase-2 prep).
    pub checkpoint_opportunities: usize,
    /// Semantic PartialRetry applications (certified-prefix OrderedAdmit on reexec).
    pub partial_retry_count: usize,
    /// PartialRetry attempted but fell back to FullAbortReexecute (unsafe split).
    pub partial_retry_fallback_full: usize,
    /// Cost-aware π chose WaitHard.
    pub cost_chose_wait: usize,
    /// Cost-aware π chose OptimisticRead.
    pub cost_chose_optimistic_read: usize,
    /// Cost-aware π chose OrderedAdmit.
    pub cost_chose_ordered_admit: usize,
    /// Mean P_conflict among cost-aware WaitHard decisions.
    pub mean_p_at_wait: f64,
    /// Mean P_conflict among cost-aware OptimisticRead decisions.
    pub mean_p_at_optimistic_read: f64,
    /// M0: fresh EVM/transact/interpreter starts (new incarnation from tx head).
    /// Incremented at `Vm::execute` immediately before the handler `run` (OCC + SpecFence).
    /// Baseline today: ≈ n_tx + head-reexecs (PartialRetry and FullAbortReexecute both restart from head).
    pub evm_entries: usize,
    /// M1+: resume from checkpoint without fresh interpreter start (stays 0 until RewindTo).
    pub resume_count: usize,
    /// M1+: rebind-only repair without rewind/restart (stays 0 until Rebind).
    pub rebind_only: usize,
    /// Cold OptimisticRead fast path: skipped Bayes/HotSet/π (OCC-like discovery).
    pub cold_optimistic_fast: usize,
    /// First-incarnation OCC-identical OptimisticRead (no FenceGraph/π/writer lookups).
    pub occ_fast_first: usize,
    /// Profile: ns spent in Handler::run / inspect_run (includes DB/maybe_wait).
    pub profile_handler_ns: u64,
    /// Profile: ns spent inside maybe_wait (π / OrderedAdmit / SoftWait decide).
    pub profile_maybe_wait_ns: u64,
    /// Profile: ns spent in try_validate + SuffixRepair/RebindOnly.
    pub profile_validate_ns: u64,
    /// Profile: ns spent in worker next_task / steal scheduling.
    pub profile_scheduler_ns: u64,
    /// M1+: rewind journal/PC to checkpoint then resume (stays 0 until RewindTo).
    pub rewind_to_cp: usize,
    /// FullAbortReexecute decisions: OCC abort reexec, or SpecFence FullAbortReexecute (no certified prefix).
    /// Each corresponding reexec also increments `evm_entries` at the next `Vm::execute`.
    pub full_abort_reexecute: usize,
    /// Semantic PartialRetry (and EarlyVal force-ordered_admit) that still restarts the interpreter
    /// from tx head — not an L1 resume. Documented alias for "head reexec under PartialRetry".
    /// M1 RewindTo must NOT increment this; use `resume_count` / `rewind_to_cp` instead.
    pub tx_head_reexec: usize,
    /// M2: WaitHard parks (tx-level Blocking → worker returns to steal).
    pub wait_park_count: usize,
    /// M2: best-effort nanoseconds spent parked waiting on a writer.
    pub wait_park_ns: u64,
    /// M2: worker stole other ready work immediately after a WaitHard park.
    pub ready_steal_on_wait: usize,
    /// Park idle subtype: SoftWait Soft arm park count.
    pub park_count_softwait: usize,
    /// Park idle subtype: SoftWait Soft arm park ns (worker sum).
    pub park_ns_softwait: u64,
    /// Park idle subtype: EarlyAbort Blocking park count.
    pub park_count_early_abort: usize,
    /// Park idle subtype: EarlyAbort Blocking park ns (worker sum).
    pub park_ns_early_abort: u64,
    /// Park idle subtype: other Blocking (cold/hint/ESTIMATE) park count.
    pub park_count_blocking_other: usize,
    /// Park idle subtype: other Blocking park ns (worker sum).
    pub park_ns_blocking_other: u64,
    /// M2: mean ready-queue depth sampled at park/push (best-effort wave width).
    pub wave_width_mean: f64,
    /// M1b: SpecFence journal effects / bound values restored on RewindTo resume.
    pub journal_ff_entries: usize,
    /// M1b: certified-prefix DB reads served from FF cache (skipped MV lazy walk).
    pub journal_ff_hits: usize,
    /// Iter13: journal FF hits via Validated-gated value-stable (origin bump, same value).
    pub value_stable_ff_hits: usize,
    /// M1b: MV lazy-walk steps + cold storage fallbacks (heavy DB work).
    pub db_heavy_ops: usize,
    /// M1c: times RewindTo credited a boundary resume (PC jump or effect-boundary skip).
    pub pc_resume_count: usize,
    /// M1c: estimated prefix opcodes/work units skipped on resume (`BoundarySnapshot.opcode_steps`).
    /// M1d: live Inspector path records real skipped steps when `live_pc_resume_count` > 0.
    pub prefix_opcodes_skipped: usize,
    /// M1d: times SpecFenceInspector actually applied a live PC/stack/gas snap on resume.
    pub live_pc_resume_count: usize,
    /// M1d: cumulative SpecFenceInspector::step counts (real opcodes entered).
    pub inspector_steps: usize,
    /// M1d: inspector steps during RewindTo resume executes only.
    pub inspector_steps_resume: usize,
    /// M1e: production absolute PC jump applied (initialize_interp jumped).
    pub absolute_jump_applied: usize,
    /// M1e: RewindTo resume deferred absolute jump (safety gate / fallback).
    pub absolute_jump_fallback: usize,
    /// M1e: accounts restored from revm journal blob on jump resume.
    pub journal_blob_ff_accounts: usize,
    /// M1g: nested CALL short-circuits served from CallOutcome cache on resume.
    pub call_outcome_cache_hits: usize,
    /// M3: OrderedAdmit chosen because residual / process WŜ prior predicted the writer.
    pub prior_ordered_admit_hits: usize,
    /// M3: prior WŜ predicted a writer but OptimisticRead was taken and later failed,
    /// or OrderedAdmit placeholder missed (writer ESTIMATE / wrong version).
    pub prior_ordered_admit_miss: usize,
    /// M3: validation failures attributed to first-incarnation OptimisticRead waste
    /// (best-effort; counted when invalid reads overlap prior-predicted locations).
    pub first_pass_validate_fail: usize,
    /// M4: Tx incarnations that ran the lean OCC-fast path (meta off).
    pub lean_mode_txs: usize,
    /// M4: Tx incarnations that ran the full SpecFence plant.
    pub full_mode_txs: usize,
    /// M4: Times engagement flipped lean → full within a block.
    pub engagement_switches: usize,
    /// R1: location-hot resolve invocations (ℓ ∈ HotSet).
    pub location_hot_resolves: usize,
    /// R1: |HotSet| at block end.
    pub hotset_size: usize,
    /// P0/P2: SoftWait arms created (FenceGraph.arm_soft).
    pub soft_wait_arms: usize,
    /// Three-pillar Await@a: BO-until-Validated arms at first unresolved access a on hot ℓ.
    pub await_at_a_arms: usize,
    /// Await@a wake → next validation succeeded (productive OrderedAdmit-when-ready).
    pub await_at_a_wake_ok: usize,
    /// Await@a wake → next validation aborted again.
    pub await_at_a_wake_reabort: usize,
    /// P0: cost π WaitHard on program locations.
    pub cost_chose_wait_program: usize,
    /// P0: cost π WaitHard on handler locations (should stay ~0).
    pub cost_chose_wait_handler: usize,
    /// P0: cost π OptimisticRead on program locations.
    pub cost_chose_optimistic_read_program: usize,
    /// P0: cost π OptimisticRead on handler locations.
    pub cost_chose_optimistic_read_handler: usize,
    /// P3: EarlyAbort fence arms (cut incarnation at early heavy program cross).
    pub early_abort_count: usize,
    /// P4: SoftWait wakes that armed RewindTo/FF at checkpoint before `k`.
    pub park_resume_at_k: usize,
    /// P4: SoftWait wakes that fell back to tx-grain FullAbortReexecute.
    pub park_resume_full_abort_reexecute: usize,
    /// V5 dig: abort while force_ordered_admit / force_prefix was already armed (Lean PartialRetry reabort).
    pub force_ordered_admit_reabort: usize,
    /// V5 dig: SoftWait wake → next validation succeeded without abort.
    pub soft_wait_wake_ok: usize,
    /// V5 dig: SoftWait wake → next validation aborted again.
    pub soft_wait_wake_reabort: usize,
    /// Iter3: escalate FullAbortReexecute parked behind unfinished conflict writer.
    pub serial_barrier_resolve: usize,
    /// Iter3: escalate FullAbortReexecute steal-first defer (no unfinished writer).
    pub serial_barrier_defer: usize,
    /// Iter4: hang-free Handler::run post-SSTORE plant captures.
    pub handler_sstore_capture: usize,
    /// Iter19: Handler SLOAD OrderedAdmit/EffectBoundary live snaps.
    pub ordered_admit_snap_capture: usize,
    /// Iter20: hang-free OrderedAdmit-snap credit consume (opcode_steps credited, no PC jump).
    pub ordered_admit_snap_credit: usize,
    /// Iter4: sibling consumers parked in hot-ℓ clique barrier.
    pub serial_barrier_clique: usize,
    /// Iter5: fb escalate deferred once for jump_is_safe after capture window.
    pub jump_defer: usize,
    /// Iter7: 2nd SuffixRepair parked behind unfinished fail-loc writers (BO Await).
    pub second_repair_await: usize,
    /// Iter14: first SuffixRepair parked behind Executing conflict writer (schedule-side).
    pub first_repair_await: usize,
    /// Iter15: first-fail true_suffix + high-fan Executing spine → escalate+barrier
    /// (collapse doomed SuffixRepair→fb_reabort→FullAbortReexecute chains).
    pub fanout_fr_collapse: usize,
    /// Iter16: !true_suffix validate-defer behind Executing spine (RebindOnly-after-spine).
    pub fanout_validate_defer: usize,
    /// Iter16: true_suffix SuffixRepair+barrier absorb (no FullAbortReexecute) on fan≥8 spine.
    pub fanout_absorb: usize,
    /// A3: `choose_edge_action` OrderedAdmit (published Data, no writer_done gate).
    pub edge_ordered_admit: usize,
    /// D6: essential wait-for (unpublished anti-dep).
    pub edge_wait_for: usize,
    /// A4/A2: independence-certified or canary OptimisticRead (OCC-cost; Region is the control unit).
    pub edge_optimistic_read: usize,
    /// A2: first-wave Avoid broadcasts on publish.
    pub avoid_broadcasts: usize,
    /// A2: canary probes consumed.
    pub canary_probes: usize,
    /// A4: independence-certified OptimisticRead.
    pub independent_optimistic_read: usize,
    /// A1: clique/spine WaitFor (mass OptimisticRead gated).
    pub spine_waits: usize,
    /// A1: |H| at block end.
    pub sketch_hot_size: usize,
    /// A2: Data-publish progressive wakes (Blocking, not SoftWait Soft).
    pub data_publish_wakes: usize,
    /// U1/U5: must_wait/force_prefix fell through to OptimisticRead (should stay ~0).
    pub force_prefix_optimistic_read: usize,
    /// S1: Ready spine writer prefer-admitted before independence OptimisticRead.
    pub prefer_admit: usize,
    /// S4: admitted more than one unfinished writer on the same ℓ.
    pub multi_spine_admit: usize,
    /// U6: quiet morph revoked a prior-seeded Fence (not live Avoid).
    pub quiet_pessimistic_revoke: usize,
    /// U4: ℓ→writer identity stored across R2/R4.
    pub writer_identity_preserved: usize,
    /// Done→Data residual OrderedAdmit (Avoid/Fence, not OptimisticReadWriterDone).
    pub ordered_admit_residual: usize,
    /// First-wave canary reopened after probe Done without Avoid.
    pub canary_reopen: usize,
    /// writer_done / u_aa learned into H/Avoid priors.
    pub writer_done_learned: usize,
    /// Frozen-grain: Detect records at access boundaries (coverage).
    pub detect_accesses: usize,
    /// Frozen-grain: PredictedEssential gate true at this \(a\).
    pub predicted_essential_hits: usize,
    /// Frozen-grain: PCC OrderedAdmit/WaitFor fired at this access (not tx-sticky).
    pub pcc_fire_at_a: usize,
    /// PredictedEssential but ROI said Fence_tax ≥ OCC_reexec → stayed OptimisticRead.
    pub pcc_roi_skip: usize,
    /// OptimisticRead≡OCC fast path (no Edge SM / sketch / checkpoint).
    pub optimistic_read_occ_fast: usize,
    /// SpecFence incarnations that executed as OccKernel (no rem).
    pub occ_kernel_execs: usize,
    /// SpecFence incarnations that executed as PccKernel (rem / Resolve).
    pub pcc_kernel_execs: usize,
    /// SpecFence validates that used the OCC bool kernel (no collect_invalid_reads tax).
    pub occ_kernel_validates: usize,
    /// Certified prefix existed but PrefixSkip lost to full_abort_reexecute reincarnation.
    pub prefix_skip_roi_full_abort: usize,
    /// Falsifier: ForcePrefix used as live Avoid key (target 0).
    pub force_prefix_as_pi: usize,
    /// Falsifier: canary live verb (target 0).
    pub canary_live_verb: usize,
    /// Falsifier: `inc` used as Avoid key (target 0).
    pub inc_avoid_hits: usize,
    /// Falsifier: H as Wait OR-door (target 0).
    pub h_or_wait_door: usize,
    /// Falsifier: morph Storm/Quiet as Fence actuator (target 0).
    pub morph_pessimistic_actuator: usize,
    /// Falsifier: writer_validated OrderedAdmit gate (target 0).
    pub writer_validated_ordered_admit_gate: usize,
    /// Falsifier: flat EdgeKey(\(ℓ\),reader) control SoT (target 0).
    pub flat_edgekey_sot: usize,
    /// WaitForDependency parks (no Aborting).
    pub wait_for_dependency: usize,
    /// WaitFor that still took AbortingThrow (should stay rare).
    pub wait_for_full_abort: usize,
    /// Dependency-aware admission: known consumer not admitted.
    pub refuse_admit: usize,
    /// WaitFor/lane hit a Done producer (ordered admit after Done — should stay rare).
    pub ordered_admit_after_done: usize,
    /// PartialAbortRebind / PartialAbortRewind wins.
    pub partial_abort_win: usize,
    /// PartialAbortRewind attempts that fell through to full_abort_reexecute.
    pub partial_abort_attempt: usize,
    /// PC-4: mean ready-set width (may_execute ∧ Ready) sampled on steal/refuse.
    pub ready_width_mean: f64,
    /// PC-4: ns spent in scheduler yield / empty refuse (idle cores).
    pub idle_core_ns: u64,
    /// D4: txs whose final incarnation is >0 (honest reexec count).
    pub incarnation_gt0: usize,
    /// D4: sum of final incarnations (= extra EVM entries from reexec).
    pub reexec_entries: usize,
    /// Incarnation>0 txs that were never dependency-admitted.
    pub unfenced_reexec: usize,
    /// Cohorts that chose OrderedAdmit this block.
    pub ordered_admit_cohorts: usize,
    /// Cohorts that chose OptimisticRead this block.
    pub optimistic_read_cohorts: usize,
    /// L6: measured refuse-path nanoseconds.
    pub refuse_ns: u64,
    /// L6: measured incarnation>0 execute nanoseconds.
    pub reexec_ns: u64,
    /// OptimisticRead-majority / low-meta block (ReadyEdge only on ordered-admit).
    pub optimistic_majority_block: bool,
    /// L1: ns-EV kept OrderedAdmit.
    pub cost_ev_keep_ordered: usize,
    /// L1: ns-EV demoted to OptimisticRead.
    pub cost_ev_demote_optimistic: usize,
    /// PC-S1: OrderedAdmit candidates beyond K.
    pub k_cap_demote: usize,
    /// CC-X1: OptimisticRead commute accepts (empty-input transfer class).
    pub commute_skip: usize,
    /// CC-R3: batch-parked off-edge aborts.
    pub batch_repair: usize,
    /// CC-D1: effective conflict promote.
    pub conflict_promote: usize,
    /// CC-D1: lazy-noise ignore.
    pub conflict_ignore: usize,
    /// Begin-block admit_seed wall (one Instant, not per-tx).
    pub admit_seed_begin_ns: u64,
    /// SF-PS: mean RunnableSet width sampled at Schedule.pick.
    pub runnable_set_width_mean: f64,
    /// SF-PS: Execute under VisibilityPolicy::Opt (Avoid=noop independent set).
    pub visibility_opt: usize,
    /// SF-PS: Execute under VisibilityPolicy::WaitReleased.
    pub visibility_wait_released: usize,
    /// SF-PS: Execute under VisibilityPolicy::OrderedTip.
    pub visibility_ordered_tip: usize,
    /// SF-PS: ResolvePlan::Commit.
    pub resolve_commit: usize,
    /// SF-PS: ResolvePlan::PartialAbortRebind.
    pub resolve_partial_rebind: usize,
    /// SF-PS: ResolvePlan::PartialAbortRewind.
    pub resolve_partial_rewind: usize,
    /// SF-PS: ResolvePlan::OrderedReplay.
    pub resolve_ordered_replay: usize,
    /// SF-PS: ResolvePlan::FullReplay (Learn penalty; not “this is OCC”).
    pub resolve_full_replay: usize,
    /// SF-PS: Schedule.pick entries (SpecFence spine).
    pub sf_schedule_picks: usize,
    /// OCC `next_occ_task` picks observed this process (must be 0 on SF blocks
    /// after [`crate::specfence::executor::reset_occ_pick_calls`]).
    pub occ_schedule_picks: usize,
    /// SF-PS: work-steal count across the three queues.
    pub steal_n: usize,
    /// SF-PS: Detect refuse that immediately filled from Q_indep (PC-2).
    pub refuse_fill_n: usize,
    /// SF-PS: mid-block IntraPatch promotes (≤1 per ℓ per block).
    pub mid_promote_n: usize,
    /// SF-PS: IntraPatch promotes vetoed by PC (width would collapse).
    pub mid_promote_veto_n: usize,
    /// Learn E1: Commit on an edged location.
    pub learn_e1_n: usize,
    /// Learn E2: FullReplay / systematic reexec.
    pub learn_e2_n: usize,
    /// Learn E3: PartialAbort success.
    pub learn_e3_n: usize,
    /// Learn E4: refuse_fill that immediately ran an independent.
    pub learn_e4_n: usize,
    /// Learn E5: unfenced storm after prepaid → under-covered.
    pub learn_e5_n: usize,
    /// Learn E6: lazy / near-indep immediate Opt demote.
    pub learn_e6_n: usize,
    /// B4: ordered prior arms planted into CostPolicy before admit_seed.
    pub prior_plant_n: usize,
    /// SF-PS: idle_core_ns / (idle_core_ns + worker_busy_ns).
    pub idle_core_frac: f64,
    /// SF-PS: ResolvePlan.apply invocations (must change queues/certs).
    pub resolve_apply_n: usize,
    /// SF-PS: SfMvMemory WaitReleased reads.
    pub sf_mv_wait_released_reads: usize,
    /// SF-PS: SfMvMemory OrderedTip reads.
    pub sf_mv_ordered_tip_reads: usize,
    /// SF-PS: explore pulls this block (reuse sticky must be 0).
    pub explore_n: usize,
    /// SF-PS: begin restored ArmTable from inter-block prior.
    pub began_from_prior: bool,
    /// v3: first WaitOnce parks this block. Same `(tx, ℓ, w)` is not counted twice.
    pub access_wait_once: usize,
    /// v3: a second Blocking of the same `(tx, ℓ, w)` was refused.
    pub access_wait_suppressed: usize,
    /// v3: beneficiary / basic-lazy reads that skipped a live writer.
    pub access_never_wait: usize,
    /// v3: single-invalid validates that installed a prefix keep (`k < fail_k`).
    pub prefix_resume_n: usize,
    /// v3: FullReplay applies that did not install a prefix keep (restart from k=0).
    pub full_from_zero: usize,
    /// v3: prefix-keep events with a known `fail_k > 0`.
    pub fail_k_n: usize,
    /// v3: smallest `fail_k` among prefix keeps. 0 when `fail_k_n` is 0.
    pub fail_k_min: usize,
    /// v3: largest `fail_k` among prefix keeps.
    pub fail_k_max: usize,
    /// v3: `fail_k` histogram. Index `k` for `1..=31`; index 31 also holds `k >= 31`.
    pub fail_k_hist: [usize; 32],
    /// SfMvMemory: early version tips installed this block.
    pub sf_early_tip_n: usize,
    /// SfMvMemory: exact waiters woken on Data publish.
    pub sf_publish_wake_n: usize,
    /// SfMvMemory: WaitOnce consume hits (spin / defer).
    pub sf_wait_once_consume_n: usize,
    /// Concurrent Detect (a): WaitOnce/crit + pred known before read.
    pub sf_detect_before_n: usize,
    /// Concurrent Avoid (b): true publish / done — collision never happens.
    pub sf_avoid_publish_n: usize,
    /// Concurrent Resolve (c): after-fail FullReplay / Rewind (path-c dominate = incomplete).
    pub sf_resolve_after_fail_n: usize,
    /// Must be 0 on Soft=0 SF: Estimate Block on SpecFence path.
    pub estimate_block_sf: usize,
}

/// Shared counters written by worker threads.
#[derive(Debug, Default)]
pub(crate) struct MetricsInner {
    wait_admissions: AtomicUsize,
    speculate_executions: AtomicUsize,
    region_promotions: AtomicUsize,
    occ_aborts: AtomicUsize,
    cascade_validations_scheduled: AtomicUsize,
    independent_txs_skipped_by_fence: AtomicUsize,
    bayes_wait_decisions: AtomicUsize,
    bayes_speculate_decisions: AtomicUsize,
    bayes_conflict_updates: AtomicUsize,
    bayes_success_updates: AtomicUsize,
    wave_promotions: AtomicUsize,
    region_validate_fail: AtomicUsize,
    tx_full_abort_reexecute: AtomicUsize,
    ordered_admit_hits: AtomicUsize,
    wait_hard_count: AtomicUsize,
    optimistic_read_count: AtomicUsize,
    selective_invalidate_count: AtomicUsize,
    cascade_revalidate_count: AtomicUsize,
    soft_edge_revokes: AtomicUsize,
    selective_fallback_full: AtomicUsize,
    checkpoint_opportunities: AtomicUsize,
    partial_retry_count: AtomicUsize,
    partial_retry_fallback_full: AtomicUsize,
    cost_chose_wait: AtomicUsize,
    cost_chose_optimistic_read: AtomicUsize,
    cost_chose_ordered_admit: AtomicUsize,
    evm_entries: AtomicUsize,
    resume_count: AtomicUsize,
    rebind_only: AtomicUsize,
    cold_optimistic_fast: AtomicUsize,
    occ_fast_first: AtomicUsize,
    profile_handler_ns: std::sync::atomic::AtomicU64,
    profile_maybe_wait_ns: std::sync::atomic::AtomicU64,
    profile_validate_ns: std::sync::atomic::AtomicU64,
    profile_scheduler_ns: std::sync::atomic::AtomicU64,
    rewind_to_cp: AtomicUsize,
    full_abort_reexecute: AtomicUsize,
    tx_head_reexec: AtomicUsize,
    wait_park_count: AtomicUsize,
    wait_park_ns: std::sync::atomic::AtomicU64,
    ready_steal_on_wait: AtomicUsize,
    park_count_softwait: AtomicUsize,
    park_ns_softwait: std::sync::atomic::AtomicU64,
    park_count_early_abort: AtomicUsize,
    park_ns_early_abort: std::sync::atomic::AtomicU64,
    park_count_blocking_other: AtomicUsize,
    park_ns_blocking_other: std::sync::atomic::AtomicU64,
    journal_ff_entries: AtomicUsize,
    journal_ff_hits: AtomicUsize,
    value_stable_ff_hits: AtomicUsize,
    db_heavy_ops: AtomicUsize,
    pc_resume_count: AtomicUsize,
    prefix_opcodes_skipped: AtomicUsize,
    live_pc_resume_count: AtomicUsize,
    inspector_steps: AtomicUsize,
    inspector_steps_resume: AtomicUsize,
    absolute_jump_applied: AtomicUsize,
    absolute_jump_fallback: AtomicUsize,
    journal_blob_ff_accounts: AtomicUsize,
    call_outcome_cache_hits: AtomicUsize,
    prior_ordered_admit_hits: AtomicUsize,
    prior_ordered_admit_miss: AtomicUsize,
    first_pass_validate_fail: AtomicUsize,
    lean_mode_txs: AtomicUsize,
    full_mode_txs: AtomicUsize,
    engagement_switches: AtomicUsize,
    location_hot_resolves: AtomicUsize,
    hotset_size: AtomicUsize,
    soft_wait_arms: AtomicUsize,
    await_at_a_arms: AtomicUsize,
    await_at_a_wake_ok: AtomicUsize,
    await_at_a_wake_reabort: AtomicUsize,
    cost_chose_wait_program: AtomicUsize,
    cost_chose_wait_handler: AtomicUsize,
    cost_chose_optimistic_read_program: AtomicUsize,
    cost_chose_optimistic_read_handler: AtomicUsize,
    early_abort_count: AtomicUsize,
    park_resume_at_k: AtomicUsize,
    park_resume_full_abort_reexecute: AtomicUsize,
    force_ordered_admit_reabort: AtomicUsize,
    soft_wait_wake_ok: AtomicUsize,
    soft_wait_wake_reabort: AtomicUsize,
    serial_barrier_resolve: AtomicUsize,
    serial_barrier_defer: AtomicUsize,
    handler_sstore_capture: AtomicUsize,
    ordered_admit_snap_capture: AtomicUsize,
    ordered_admit_snap_credit: AtomicUsize,
    serial_barrier_clique: AtomicUsize,
    jump_defer: AtomicUsize,
    second_repair_await: AtomicUsize,
    first_repair_await: AtomicUsize,
    fanout_fr_collapse: AtomicUsize,
    fanout_validate_defer: AtomicUsize,
    fanout_absorb: AtomicUsize,
    edge_ordered_admit: AtomicUsize,
    edge_wait_for: AtomicUsize,
    edge_optimistic_read: AtomicUsize,
    avoid_broadcasts: AtomicUsize,
    canary_probes: AtomicUsize,
    independent_optimistic_read: AtomicUsize,
    spine_waits: AtomicUsize,
    sketch_hot_size: AtomicUsize,
    data_publish_wakes: AtomicUsize,
    force_prefix_optimistic_read: AtomicUsize,
    prefer_admit: AtomicUsize,
    multi_spine_admit: AtomicUsize,
    quiet_pessimistic_revoke: AtomicUsize,
    writer_identity_preserved: AtomicUsize,
    ordered_admit_residual: AtomicUsize,
    canary_reopen: AtomicUsize,
    writer_done_learned: AtomicUsize,
    detect_accesses: AtomicUsize,
    predicted_essential_hits: AtomicUsize,
    pcc_fire_at_a: AtomicUsize,
    pcc_roi_skip: AtomicUsize,
    optimistic_read_occ_fast: AtomicUsize,
    occ_kernel_execs: AtomicUsize,
    pcc_kernel_execs: AtomicUsize,
    occ_kernel_validates: AtomicUsize,
    prefix_skip_roi_full_abort: AtomicUsize,
    force_prefix_as_pi: AtomicUsize,
    canary_live_verb: AtomicUsize,
    inc_avoid_hits: AtomicUsize,
    h_or_wait_door: AtomicUsize,
    morph_pessimistic_actuator: AtomicUsize,
    writer_validated_ordered_admit_gate: AtomicUsize,
    flat_edgekey_sot: AtomicUsize,
    wait_for_dependency: AtomicUsize,
    wait_for_full_abort: AtomicUsize,
    refuse_admit: AtomicUsize,
    ordered_admit_after_done: AtomicUsize,
    partial_abort_win: AtomicUsize,
    partial_abort_attempt: AtomicUsize,
    ready_width_sum_bits: AtomicU64,
    idle_core_ns: std::sync::atomic::AtomicU64,
    incarnation_gt0: AtomicUsize,
    reexec_entries: AtomicUsize,
    unfenced_reexec: AtomicUsize,
    ordered_admit_cohorts: AtomicUsize,
    optimistic_read_cohorts: AtomicUsize,
    refuse_ns: AtomicU64,
    reexec_ns: AtomicU64,
    optimistic_majority_block: AtomicUsize,
    cost_ev_keep_ordered: AtomicUsize,
    cost_ev_demote_optimistic: AtomicUsize,
    k_cap_demote: AtomicUsize,
    commute_skip: AtomicUsize,
    batch_repair: AtomicUsize,
    conflict_promote: AtomicUsize,
    conflict_ignore: AtomicUsize,
    admit_seed_begin_ns: AtomicU64,
    worker_busy_ns: AtomicU64,
    runnable_width_sum: AtomicU64,
    runnable_width_n: AtomicUsize,
    visibility_opt: AtomicUsize,
    visibility_wait_released: AtomicUsize,
    visibility_ordered_tip: AtomicUsize,
    resolve_commit: AtomicUsize,
    resolve_partial_rebind: AtomicUsize,
    resolve_partial_rewind: AtomicUsize,
    resolve_ordered_replay: AtomicUsize,
    resolve_full_replay: AtomicUsize,
    sf_schedule_picks: AtomicUsize,
    steal_n: AtomicUsize,
    refuse_fill_n: AtomicUsize,
    mid_promote_n: AtomicUsize,
    mid_promote_veto_n: AtomicUsize,
    learn_e1_n: AtomicUsize,
    learn_e2_n: AtomicUsize,
    learn_e3_n: AtomicUsize,
    learn_e4_n: AtomicUsize,
    learn_e5_n: AtomicUsize,
    learn_e6_n: AtomicUsize,
    prior_plant_n: AtomicUsize,
    resolve_apply_n: AtomicUsize,
    sf_mv_wait_released_reads: AtomicUsize,
    sf_mv_ordered_tip_reads: AtomicUsize,
    explore_n: AtomicUsize,
    began_from_prior: AtomicUsize,
    access_wait_once: AtomicUsize,
    access_wait_suppressed: AtomicUsize,
    access_never_wait: AtomicUsize,
    prefix_resume_n: AtomicUsize,
    full_from_zero: AtomicUsize,
    fail_k_sum: AtomicU64,
    fail_k_n: AtomicUsize,
    fail_k_min: AtomicUsize,
    fail_k_max: AtomicUsize,
    fail_k_hist: [AtomicUsize; 32],
    sf_early_tip_n: AtomicUsize,
    sf_publish_wake_n: AtomicUsize,
    sf_wait_once_consume_n: AtomicUsize,
    sf_detect_before_n: AtomicUsize,
    sf_avoid_publish_n: AtomicUsize,
    sf_resolve_after_fail_n: AtomicUsize,
    estimate_block_sf: AtomicUsize,
    /// Stored as bits of f64 mean at snapshot time from WaveParkTable.
    wait_addresses: DashMap<Address, (), BuildSuffixHasher>,
    speculate_addresses: DashMap<Address, (), BuildSuffixHasher>,
    hot_accounts: DashMap<Address, (), BuildSuffixHasher>,
}

impl MetricsInner {
    pub(crate) fn record_wait(&self, address: Address) {
        self.wait_admissions.fetch_add(1, Ordering::Relaxed);
        self.wait_addresses.insert(address, ());
        self.hot_accounts.insert(address, ());
    }

    pub(crate) fn record_speculate(&self, from: Address, to: Option<Address>) {
        self.speculate_executions.fetch_add(1, Ordering::Relaxed);
        self.speculate_addresses.insert(from, ());
        if let Some(to) = to {
            self.speculate_addresses.insert(to, ());
        }
    }

    pub(crate) fn record_promotion(&self, address: Option<Address>) {
        self.region_promotions.fetch_add(1, Ordering::Relaxed);
        if let Some(address) = address {
            self.hot_accounts.insert(address, ());
        }
    }

    pub(crate) fn record_occ_abort(&self) {
        self.occ_aborts.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_fence_cascade(
        &self,
        cascade_scheduled: usize,
        independent_skipped: usize,
    ) {
        if cascade_scheduled > 0 {
            self.cascade_validations_scheduled
                .fetch_add(cascade_scheduled, Ordering::Relaxed);
            self.cascade_revalidate_count
                .fetch_add(cascade_scheduled, Ordering::Relaxed);
        }
        if independent_skipped > 0 {
            self.independent_txs_skipped_by_fence
                .fetch_add(independent_skipped, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_bayes_wait(&self) {
        self.bayes_wait_decisions.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_speculate(&self) {
        self.bayes_speculate_decisions
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_conflict(&self) {
        self.bayes_conflict_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_bayes_success(&self) {
        self.bayes_success_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_wave_promotion(&self) {
        self.wave_promotions.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_region_validate_fail(&self, n: usize) {
        if n > 0 {
            self.region_validate_fail.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_tx_full_abort_reexecute(&self) {
        self.tx_full_abort_reexecute.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_ordered_admit_hit(&self) {
        self.ordered_admit_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_wait_hard(&self) {
        self.wait_hard_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_optimistic_read(&self) {
        self.optimistic_read_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_selective_invalidate(&self, n: usize) {
        if n > 0 {
            self.selective_invalidate_count
                .fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_soft_edge_revoke(&self) {
        self.soft_edge_revokes.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_selective_fallback_full(&self) {
        self.selective_fallback_full.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_checkpoint_opportunity(&self) {
        self.checkpoint_opportunities
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn set_checkpoint_opportunities(&self, n: usize) {
        self.checkpoint_opportunities.store(n, Ordering::Relaxed);
    }

    pub(crate) fn record_partial_retry(&self) {
        self.partial_retry_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_partial_retry_fallback_full(&self) {
        self.partial_retry_fallback_full
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_wait(&self) {
        self.cost_chose_wait.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_optimistic_read(&self) {
        self.cost_chose_optimistic_read
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_ordered_admit(&self) {
        self.cost_chose_ordered_admit
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_soft_wait_arm(&self) {
        self.soft_wait_arms.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn set_soft_wait_arms(&self, n: usize) {
        self.soft_wait_arms.store(n, Ordering::Relaxed);
    }

    pub(crate) fn record_await_at_a_arm(&self) {
        self.await_at_a_arms.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_await_at_a_wake_ok(&self) {
        self.await_at_a_wake_ok.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_await_at_a_wake_reabort(&self) {
        self.await_at_a_wake_reabort.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_wait_program(&self) {
        self.cost_chose_wait_program.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_wait_handler(&self) {
        self.cost_chose_wait_handler.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_optimistic_read_program(&self) {
        self.cost_chose_optimistic_read_program
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cost_chose_optimistic_read_handler(&self) {
        self.cost_chose_optimistic_read_handler
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_early_abort(&self) {
        self.early_abort_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Fresh EVM session / interpreter start from tx head (plant v2 L1 denominator).
    pub(crate) fn record_evm_entry(&self) {
        self.evm_entries.fetch_add(1, Ordering::Relaxed);
    }

    /// M1: resume from checkpoint without counting as fresh tx-head entry.
    pub(crate) fn record_resume(&self) {
        self.resume_count.fetch_add(1, Ordering::Relaxed);
    }

    /// M1: rebind-only repair without rewind/restart.
    pub(crate) fn record_rebind_only(&self) {
        self.rebind_only.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_cold_optimistic_fast(&self) {
        self.cold_optimistic_fast.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_occ_fast_first(&self) {
        self.occ_fast_first.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn add_profile_handler_ns(&self, ns: u64) {
        if ns > 0 {
            self.profile_handler_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_profile_maybe_wait_ns(&self, ns: u64) {
        if ns > 0 {
            self.profile_maybe_wait_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_profile_validate_ns(&self, ns: u64) {
        if ns > 0 {
            self.profile_validate_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn add_profile_scheduler_ns(&self, ns: u64) {
        if ns > 0 {
            self.profile_scheduler_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    /// M1: rewind journal/PC to checkpoint then resume.
    pub(crate) fn record_rewind_to_cp(&self) {
        self.rewind_to_cp.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_full_abort_reexecute(&self) {
        self.full_abort_reexecute.fetch_add(1, Ordering::Relaxed);
    }

    /// Today's PartialRetry / EarlyVal still restarts interpreter from tx head.
    pub(crate) fn record_tx_head_reexec(&self) {
        self.tx_head_reexec.fetch_add(1, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub(crate) fn record_wait_park(&self) {
        self.wait_park_count.fetch_add(1, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub(crate) fn add_wait_park_ns(&self, ns: u64) {
        self.wait_park_ns.fetch_add(ns, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub(crate) fn record_ready_steal_on_wait(&self) {
        self.ready_steal_on_wait.fetch_add(1, Ordering::Relaxed);
    }

    /// Copy M2/P4 wave counters from the block's WaveParkTable at snapshot time.
    pub(crate) fn set_wave_metrics(
        &self,
        wait_park_count: usize,
        wait_park_ns: u64,
        ready_steal_on_wait: usize,
    ) {
        self.wait_park_count
            .store(wait_park_count, Ordering::Relaxed);
        self.wait_park_ns.store(wait_park_ns, Ordering::Relaxed);
        self.ready_steal_on_wait
            .store(ready_steal_on_wait, Ordering::Relaxed);
    }

    pub(crate) fn set_park_subtype_metrics(
        &self,
        park_count_softwait: usize,
        park_ns_softwait: u64,
        park_count_early_abort: usize,
        park_ns_early_abort: u64,
        park_count_blocking_other: usize,
        park_ns_blocking_other: u64,
    ) {
        self.park_count_softwait
            .store(park_count_softwait, Ordering::Relaxed);
        self.park_ns_softwait
            .store(park_ns_softwait, Ordering::Relaxed);
        self.park_count_early_abort
            .store(park_count_early_abort, Ordering::Relaxed);
        self.park_ns_early_abort
            .store(park_ns_early_abort, Ordering::Relaxed);
        self.park_count_blocking_other
            .store(park_count_blocking_other, Ordering::Relaxed);
        self.park_ns_blocking_other
            .store(park_ns_blocking_other, Ordering::Relaxed);
    }

    pub(crate) fn set_park_resume_metrics(
        &self,
        park_resume_at_k: usize,
        park_resume_full_abort_reexecute: usize,
    ) {
        self.park_resume_at_k
            .store(park_resume_at_k, Ordering::Relaxed);
        self.park_resume_full_abort_reexecute
            .store(park_resume_full_abort_reexecute, Ordering::Relaxed);
    }

    pub(crate) fn record_force_ordered_admit_reabort(&self) {
        self.force_ordered_admit_reabort
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_soft_wait_wake_ok(&self) {
        self.soft_wait_wake_ok.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_soft_wait_wake_reabort(&self) {
        self.soft_wait_wake_reabort.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_serial_barrier_resolve(&self) {
        self.serial_barrier_resolve.fetch_add(1, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub(crate) fn record_serial_barrier_defer(&self) {
        self.serial_barrier_defer.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_handler_sstore_capture(&self) {
        self.handler_sstore_capture.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_handler_ordered_admit_snap_capture(&self) {
        self.ordered_admit_snap_capture
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Iter20: note OrderedAdmit tip was present on resume but not PC-jumped (credit path).
    pub(crate) fn record_ordered_admit_snap_credit(&self, _steps: u64) {
        self.ordered_admit_snap_credit
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_serial_barrier_clique(&self) {
        self.serial_barrier_clique.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_second_repair_await(&self) {
        self.second_repair_await.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_first_repair_await(&self) {
        self.first_repair_await.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_fanout_fr_collapse(&self) {
        self.fanout_fr_collapse.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_fanout_validate_defer(&self) {
        self.fanout_validate_defer.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_fanout_absorb(&self) {
        self.fanout_absorb.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_edge_ordered_admit(&self) {
        self.edge_ordered_admit.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_edge_wait_for(&self) {
        self.edge_wait_for.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_wait_for_dependency(&self) {
        self.wait_for_dependency.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_wait_for_full_abort(&self) {
        self.wait_for_full_abort.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_refuse_admit(&self) {
        self.record_refuse_admit_n(1);
    }

    #[inline]
    pub(crate) fn record_refuse_admit_n(&self, n: usize) {
        if n > 0 {
            self.refuse_admit.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_ordered_admit_after_done(&self) {
        self.ordered_admit_after_done
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_partial_abort_win(&self) {
        self.partial_abort_win.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_partial_abort_attempt(&self) {
        self.partial_abort_attempt.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn set_pc_learn_metrics(
        &self,
        ready_width_mean: f64,
        idle_core_ns: u64,
        incarnation_gt0: usize,
        reexec_entries: usize,
        unfenced_reexec: usize,
        ordered_admit_cohorts: usize,
        optimistic_read_cohorts: usize,
    ) {
        self.ready_width_sum_bits
            .store(ready_width_mean.to_bits(), Ordering::Relaxed);
        self.idle_core_ns.store(idle_core_ns, Ordering::Relaxed);
        self.incarnation_gt0
            .store(incarnation_gt0, Ordering::Relaxed);
        self.reexec_entries.store(reexec_entries, Ordering::Relaxed);
        self.unfenced_reexec
            .store(unfenced_reexec, Ordering::Relaxed);
        self.ordered_admit_cohorts
            .store(ordered_admit_cohorts, Ordering::Relaxed);
        self.optimistic_read_cohorts
            .store(optimistic_read_cohorts, Ordering::Relaxed);
    }

    pub(crate) fn set_ns_learn_metrics(
        &self,
        refuse_ns: u64,
        reexec_ns: u64,
        optimistic_majority_block: bool,
        cost_ev_keep_ordered: usize,
        cost_ev_demote_optimistic: usize,
        k_cap_demote: usize,
        commute_skip: usize,
        batch_repair: usize,
        conflict_promote: usize,
        conflict_ignore: usize,
    ) {
        self.refuse_ns.store(refuse_ns, Ordering::Relaxed);
        self.reexec_ns.store(reexec_ns, Ordering::Relaxed);
        self.optimistic_majority_block
            .store(usize::from(optimistic_majority_block), Ordering::Relaxed);
        self.cost_ev_keep_ordered
            .store(cost_ev_keep_ordered, Ordering::Relaxed);
        self.cost_ev_demote_optimistic
            .store(cost_ev_demote_optimistic, Ordering::Relaxed);
        self.k_cap_demote.store(k_cap_demote, Ordering::Relaxed);
        self.commute_skip.store(commute_skip, Ordering::Relaxed);
        self.batch_repair.store(batch_repair, Ordering::Relaxed);
        self.conflict_promote
            .store(conflict_promote, Ordering::Relaxed);
        self.conflict_ignore
            .store(conflict_ignore, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn set_admit_seed_begin_ns(&self, ns: u64) {
        self.admit_seed_begin_ns.store(ns, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn admit_seed_begin_ns(&self) -> u64 {
        self.admit_seed_begin_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn add_worker_busy_ns(&self, ns: u64) {
        if ns > 0 {
            self.worker_busy_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn worker_busy_ns(&self) -> u64 {
        self.worker_busy_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn record_refuse_ns(&self, ns: u64) {
        if ns > 0 {
            self.refuse_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn record_reexec_ns(&self, ns: u64) {
        if ns > 0 {
            self.reexec_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn record_commute_skip(&self) {
        self.commute_skip.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn record_sf_schedule_pick(&self) {
        self.sf_schedule_picks.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn record_refuse_fill(&self, n: usize) {
        if n > 0 {
            self.refuse_fill_n.fetch_add(n, Ordering::Relaxed);
            self.refuse_admit.fetch_add(n, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn record_resolve_apply(&self) {
        self.resolve_apply_n.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn add_idle_core_ns(&self, ns: u64) {
        if ns > 0 {
            self.idle_core_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(crate) fn record_sf_mv_read(&self, vis: super::VisibilityPolicy) {
        match vis {
            super::VisibilityPolicy::WaitReleased => {
                self.sf_mv_wait_released_reads
                    .fetch_add(1, Ordering::Relaxed);
            }
            super::VisibilityPolicy::OrderedTip => {
                self.sf_mv_ordered_tip_reads.fetch_add(1, Ordering::Relaxed);
            }
            super::VisibilityPolicy::Opt => {}
        }
    }

    #[inline]
    pub(crate) fn set_true_spine_metrics(
        &self,
        steal_n: usize,
        refuse_fill_n: usize,
        mid_promote_n: usize,
        mid_promote_veto_n: usize,
        explore_n: usize,
        began_from_prior: bool,
        e1_n: usize,
        e2_n: usize,
        e3_n: usize,
        e4_n: usize,
        e5_n: usize,
        e6_n: usize,
        prior_plant_n: usize,
    ) {
        self.steal_n.store(steal_n, Ordering::Relaxed);
        self.refuse_fill_n
            .fetch_add(refuse_fill_n, Ordering::Relaxed);
        self.mid_promote_n.store(mid_promote_n, Ordering::Relaxed);
        self.mid_promote_veto_n
            .store(mid_promote_veto_n, Ordering::Relaxed);
        self.explore_n.store(explore_n, Ordering::Relaxed);
        self.began_from_prior
            .store(usize::from(began_from_prior), Ordering::Relaxed);
        self.learn_e1_n.store(e1_n, Ordering::Relaxed);
        self.learn_e2_n.store(e2_n, Ordering::Relaxed);
        self.learn_e3_n.store(e3_n, Ordering::Relaxed);
        self.learn_e4_n.store(e4_n, Ordering::Relaxed);
        self.learn_e5_n.store(e5_n, Ordering::Relaxed);
        self.learn_e6_n.store(e6_n, Ordering::Relaxed);
        self.prior_plant_n.store(prior_plant_n, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn sample_runnable_width(&self, width: usize) {
        self.runnable_width_sum
            .fetch_add(width as u64, Ordering::Relaxed);
        self.runnable_width_n.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn record_visibility(&self, vis: super::VisibilityPolicy) {
        match vis {
            super::VisibilityPolicy::Opt => {
                self.visibility_opt.fetch_add(1, Ordering::Relaxed);
            }
            super::VisibilityPolicy::WaitReleased => {
                self.visibility_wait_released
                    .fetch_add(1, Ordering::Relaxed);
            }
            super::VisibilityPolicy::OrderedTip => {
                self.visibility_ordered_tip.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    #[inline]
    pub(crate) fn record_resolve_plan(&self, plan: super::ResolvePlan) {
        match plan {
            super::ResolvePlan::Commit => {
                self.resolve_commit.fetch_add(1, Ordering::Relaxed);
            }
            super::ResolvePlan::PartialAbortRebind => {
                self.resolve_partial_rebind.fetch_add(1, Ordering::Relaxed);
            }
            super::ResolvePlan::PartialAbortRewind => {
                self.resolve_partial_rewind.fetch_add(1, Ordering::Relaxed);
            }
            super::ResolvePlan::OrderedReplay => {
                self.resolve_ordered_replay.fetch_add(1, Ordering::Relaxed);
            }
            super::ResolvePlan::FullReplay => {
                self.resolve_full_replay.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    #[inline]
    pub(crate) fn record_batch_repair(&self) {
        self.batch_repair.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_edge_optimistic_read(&self) {
        self.edge_optimistic_read.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_avoid_broadcast(&self) {
        self.avoid_broadcasts.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_canary_probe(&self) {
        self.canary_probes.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_independent_optimistic_read(&self) {
        self.independent_optimistic_read
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_spine_wait(&self) {
        self.spine_waits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn set_sketch_hot_size(&self, n: usize) {
        self.sketch_hot_size.store(n, Ordering::Relaxed);
    }

    pub(crate) fn record_data_publish_wake(&self) {
        self.data_publish_wakes.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_force_prefix_optimistic_read(&self) {
        self.force_prefix_optimistic_read
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_prefer_admit(&self) {
        self.prefer_admit.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_multi_spine_admit(&self) {
        self.multi_spine_admit.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_quiet_pessimistic_revoke(&self, n: usize) {
        if n > 0 {
            self.quiet_pessimistic_revoke
                .fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_writer_identity_preserved(&self) {
        self.writer_identity_preserved
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_ordered_admit_residual(&self) {
        self.ordered_admit_residual.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_canary_reopen(&self) {
        self.canary_reopen.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_writer_done_learned(&self) {
        self.writer_done_learned.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_detect_access(&self) {
        self.detect_accesses.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_predicted_essential(&self) {
        self.predicted_essential_hits
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_pcc_fire_at_a(&self) {
        self.pcc_fire_at_a.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_pcc_roi_skip(&self) {
        self.pcc_roi_skip.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_optimistic_read_occ_fast(&self) {
        self.optimistic_read_occ_fast
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_occ_kernel_exec(&self) {
        self.occ_kernel_execs.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_pcc_kernel_exec(&self) {
        self.pcc_kernel_execs.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_occ_kernel_validate(&self) {
        self.occ_kernel_validates.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_prefix_skip_roi_full_abort(&self) {
        self.prefix_skip_roi_full_abort
            .fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_jump_defer(&self) {
        self.jump_defer.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_journal_ff_entries(&self, n: usize) {
        if n > 0 {
            self.journal_ff_entries.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_journal_ff_hit(&self) {
        self.journal_ff_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Store the block's Avoid counters (not an increment — end-of-block census).
    pub(crate) fn record_access_avoid(
        &self,
        wait_once: usize,
        suppressed: usize,
        never_wait: usize,
        prefix_resume: usize,
    ) {
        self.access_wait_once.store(wait_once, Ordering::Relaxed);
        self.access_wait_suppressed
            .store(suppressed, Ordering::Relaxed);
        self.access_never_wait.store(never_wait, Ordering::Relaxed);
        self.prefix_resume_n.store(prefix_resume, Ordering::Relaxed);
    }

    /// SfMvMemory tip-plane + Detect|Avoid|Resolve path census (end-of-block).
    pub(crate) fn record_sf_mv_tips(
        &self,
        early_tip: usize,
        publish_wake: usize,
        wait_once_consume: usize,
        estimate_block_sf: usize,
        detect_before: usize,
        avoid_publish: usize,
        resolve_after_fail: usize,
    ) {
        self.sf_early_tip_n.store(early_tip, Ordering::Relaxed);
        self.sf_publish_wake_n
            .store(publish_wake, Ordering::Relaxed);
        self.sf_wait_once_consume_n
            .store(wait_once_consume, Ordering::Relaxed);
        self.estimate_block_sf
            .store(estimate_block_sf, Ordering::Relaxed);
        self.sf_detect_before_n
            .store(detect_before, Ordering::Relaxed);
        self.sf_avoid_publish_n
            .store(avoid_publish, Ordering::Relaxed);
        self.sf_resolve_after_fail_n
            .store(resolve_after_fail, Ordering::Relaxed);
    }

    pub(crate) fn record_prefix_resume(&self, _kept: usize) {
        self.prefix_resume_n.fetch_add(1, Ordering::Relaxed);
    }

    /// One prefix keep at a known `fail_k > 0`.
    pub(crate) fn record_fail_k(&self, k: u32) {
        let k = k as usize;
        if k == 0 {
            return;
        }
        self.fail_k_sum.fetch_add(k as u64, Ordering::Relaxed);
        self.fail_k_n.fetch_add(1, Ordering::Relaxed);
        self.fail_k_max.fetch_max(k, Ordering::Relaxed);
        self.fail_k_hist[k.min(31)].fetch_add(1, Ordering::Relaxed);
        let mut cur = self.fail_k_min.load(Ordering::Relaxed);
        loop {
            if cur != 0 && cur <= k {
                break;
            }
            let next = if cur == 0 { k } else { cur.min(k) };
            match self.fail_k_min.compare_exchange_weak(
                cur,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(seen) => cur = seen,
            }
        }
    }

    /// FullReplay that did not install prefix snaps. The next incarnation starts at k=0.
    pub(crate) fn record_full_from_zero(&self) {
        self.full_from_zero.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_value_stable_ff_hit(&self) {
        self.value_stable_ff_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_db_heavy_op(&self) {
        self.db_heavy_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// M1c/M1d: PC/boundary resume applied; `skipped` = prefix opcode steps not re-run.
    pub(crate) fn record_pc_resume(&self, skipped: u64) {
        self.pc_resume_count.fetch_add(1, Ordering::Relaxed);
        if skipped > 0 {
            self.prefix_opcodes_skipped
                .fetch_add(skipped as usize, Ordering::Relaxed);
        }
    }

    /// M1d: live `initialize_interp` applied a BoundarySnapshot (real PC jump).
    pub(crate) fn record_live_pc_resume(&self) {
        self.live_pc_resume_count.fetch_add(1, Ordering::Relaxed);
    }

    /// M1e: absolute PC jump applied on production RewindTo resume.
    pub(crate) fn record_absolute_jump_applied(&self) {
        self.absolute_jump_applied.fetch_add(1, Ordering::Relaxed);
    }

    /// M1e: safety gate deferred jump → credit-only / non-jump fallback.
    pub(crate) fn record_absolute_jump_fallback(&self) {
        self.absolute_jump_fallback.fetch_add(1, Ordering::Relaxed);
    }

    /// M1e: journal-blob accounts merged into revm state on jump resume.
    pub(crate) fn record_journal_blob_ff(&self, accounts: usize) {
        if accounts > 0 {
            self.journal_blob_ff_accounts
                .fetch_add(accounts, Ordering::Relaxed);
        }
    }

    /// M1g: nested CALL short-circuited from CallOutcome cache on resume.
    pub(crate) fn record_call_outcome_cache_hit(&self) {
        self.call_outcome_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// M1d: accumulate Inspector::step counts for this execute.
    pub(crate) fn record_inspector_steps(&self, steps: u64, is_resume: bool) {
        if steps == 0 {
            return;
        }
        self.inspector_steps
            .fetch_add(steps as usize, Ordering::Relaxed);
        if is_resume {
            self.inspector_steps_resume
                .fetch_add(steps as usize, Ordering::Relaxed);
        }
    }

    /// M3: OrderedAdmit from learned / residual WŜ prior.
    pub(crate) fn record_prior_ordered_admit_hit(&self) {
        self.prior_ordered_admit_hits
            .fetch_add(1, Ordering::Relaxed);
    }

    /// M3: prior WŜ did not prevent a bad OptimisticRead / failed OrderedAdmit.
    pub(crate) fn record_prior_ordered_admit_miss(&self) {
        self.prior_ordered_admit_miss
            .fetch_add(1, Ordering::Relaxed);
    }

    /// M3: first-incarnation validate fail overlapping prior-predicted locs.
    pub(crate) fn record_first_pass_validate_fail(&self, n: usize) {
        if n > 0 {
            self.first_pass_validate_fail
                .fetch_add(n, Ordering::Relaxed);
        }
    }

    /// M4/R1: copy engagement + HotSet counters at snapshot time.
    pub(crate) fn set_engagement_metrics(
        &self,
        lean_mode_txs: usize,
        full_mode_txs: usize,
        engagement_switches: usize,
        location_hot_resolves: usize,
        hotset_size: usize,
    ) {
        self.lean_mode_txs.store(lean_mode_txs, Ordering::Relaxed);
        self.full_mode_txs.store(full_mode_txs, Ordering::Relaxed);
        self.engagement_switches
            .store(engagement_switches, Ordering::Relaxed);
        self.location_hot_resolves
            .store(location_hot_resolves, Ordering::Relaxed);
        self.hotset_size.store(hotset_size, Ordering::Relaxed);
    }

    pub(crate) fn mark_hot(&self, address: Address) {
        self.hot_accounts.insert(address, ());
    }

    pub(crate) fn hot_accounts(&self) -> impl Iterator<Item = Address> + '_ {
        self.hot_accounts.iter().map(|entry| *entry.key())
    }

    pub(crate) fn snapshot(
        &self,
        wave_id: usize,
        mean_wait_posterior: f64,
        mean_p_at_wait: f64,
        mean_p_at_optimistic_read: f64,
        wave_width_mean: f64,
    ) -> SpecFenceMetrics {
        let mut wait_addresses: Vec<Address> =
            self.wait_addresses.iter().map(|e| *e.key()).collect();
        wait_addresses.sort_unstable();
        let mut speculate_addresses: Vec<Address> =
            self.speculate_addresses.iter().map(|e| *e.key()).collect();
        speculate_addresses.sort_unstable();
        SpecFenceMetrics {
            wait_admissions: self.wait_admissions.load(Ordering::Relaxed),
            speculate_executions: self.speculate_executions.load(Ordering::Relaxed),
            region_promotions: self.region_promotions.load(Ordering::Relaxed),
            occ_aborts: self.occ_aborts.load(Ordering::Relaxed),
            cascade_validations_scheduled: self
                .cascade_validations_scheduled
                .load(Ordering::Relaxed),
            independent_txs_skipped_by_fence: self
                .independent_txs_skipped_by_fence
                .load(Ordering::Relaxed),
            bayes_wait_decisions: self.bayes_wait_decisions.load(Ordering::Relaxed),
            bayes_speculate_decisions: self.bayes_speculate_decisions.load(Ordering::Relaxed),
            bayes_conflict_updates: self.bayes_conflict_updates.load(Ordering::Relaxed),
            bayes_success_updates: self.bayes_success_updates.load(Ordering::Relaxed),
            wave_promotions: self.wave_promotions.load(Ordering::Relaxed),
            wave_id,
            mean_wait_posterior,
            wait_addresses,
            speculate_addresses,
            region_validate_fail: self.region_validate_fail.load(Ordering::Relaxed),
            tx_full_abort_reexecute: self.tx_full_abort_reexecute.load(Ordering::Relaxed),
            ordered_admit_hits: self.ordered_admit_hits.load(Ordering::Relaxed),
            wait_hard_count: self.wait_hard_count.load(Ordering::Relaxed),
            optimistic_read_count: self.optimistic_read_count.load(Ordering::Relaxed),
            selective_invalidate_count: self.selective_invalidate_count.load(Ordering::Relaxed),
            cascade_revalidate_count: self.cascade_revalidate_count.load(Ordering::Relaxed),
            soft_edge_revokes: self.soft_edge_revokes.load(Ordering::Relaxed),
            selective_fallback_full: self.selective_fallback_full.load(Ordering::Relaxed),
            checkpoint_opportunities: self.checkpoint_opportunities.load(Ordering::Relaxed),
            partial_retry_count: self.partial_retry_count.load(Ordering::Relaxed),
            partial_retry_fallback_full: self.partial_retry_fallback_full.load(Ordering::Relaxed),
            cost_chose_wait: self.cost_chose_wait.load(Ordering::Relaxed),
            cost_chose_optimistic_read: self.cost_chose_optimistic_read.load(Ordering::Relaxed),
            cost_chose_ordered_admit: self.cost_chose_ordered_admit.load(Ordering::Relaxed),
            mean_p_at_wait,
            mean_p_at_optimistic_read,
            evm_entries: self.evm_entries.load(Ordering::Relaxed),
            resume_count: self.resume_count.load(Ordering::Relaxed),
            rebind_only: self.rebind_only.load(Ordering::Relaxed),
            cold_optimistic_fast: self.cold_optimistic_fast.load(Ordering::Relaxed),
            occ_fast_first: self.occ_fast_first.load(Ordering::Relaxed),
            profile_handler_ns: self.profile_handler_ns.load(Ordering::Relaxed),
            profile_maybe_wait_ns: self.profile_maybe_wait_ns.load(Ordering::Relaxed),
            profile_validate_ns: self.profile_validate_ns.load(Ordering::Relaxed),
            profile_scheduler_ns: self.profile_scheduler_ns.load(Ordering::Relaxed),
            rewind_to_cp: self.rewind_to_cp.load(Ordering::Relaxed),
            full_abort_reexecute: self.full_abort_reexecute.load(Ordering::Relaxed),
            tx_head_reexec: self.tx_head_reexec.load(Ordering::Relaxed),
            wait_park_count: self.wait_park_count.load(Ordering::Relaxed),
            wait_park_ns: self.wait_park_ns.load(Ordering::Relaxed),
            ready_steal_on_wait: self.ready_steal_on_wait.load(Ordering::Relaxed),
            park_count_softwait: self.park_count_softwait.load(Ordering::Relaxed),
            park_ns_softwait: self.park_ns_softwait.load(Ordering::Relaxed),
            park_count_early_abort: self.park_count_early_abort.load(Ordering::Relaxed),
            park_ns_early_abort: self.park_ns_early_abort.load(Ordering::Relaxed),
            park_count_blocking_other: self.park_count_blocking_other.load(Ordering::Relaxed),
            park_ns_blocking_other: self.park_ns_blocking_other.load(Ordering::Relaxed),
            wave_width_mean,
            journal_ff_entries: self.journal_ff_entries.load(Ordering::Relaxed),
            journal_ff_hits: self.journal_ff_hits.load(Ordering::Relaxed),
            value_stable_ff_hits: self.value_stable_ff_hits.load(Ordering::Relaxed),
            db_heavy_ops: self.db_heavy_ops.load(Ordering::Relaxed),
            pc_resume_count: self.pc_resume_count.load(Ordering::Relaxed),
            prefix_opcodes_skipped: self.prefix_opcodes_skipped.load(Ordering::Relaxed),
            live_pc_resume_count: self.live_pc_resume_count.load(Ordering::Relaxed),
            inspector_steps: self.inspector_steps.load(Ordering::Relaxed),
            inspector_steps_resume: self.inspector_steps_resume.load(Ordering::Relaxed),
            absolute_jump_applied: self.absolute_jump_applied.load(Ordering::Relaxed),
            absolute_jump_fallback: self.absolute_jump_fallback.load(Ordering::Relaxed),
            journal_blob_ff_accounts: self.journal_blob_ff_accounts.load(Ordering::Relaxed),
            call_outcome_cache_hits: self.call_outcome_cache_hits.load(Ordering::Relaxed),
            prior_ordered_admit_hits: self.prior_ordered_admit_hits.load(Ordering::Relaxed),
            prior_ordered_admit_miss: self.prior_ordered_admit_miss.load(Ordering::Relaxed),
            first_pass_validate_fail: self.first_pass_validate_fail.load(Ordering::Relaxed),
            lean_mode_txs: self.lean_mode_txs.load(Ordering::Relaxed),
            full_mode_txs: self.full_mode_txs.load(Ordering::Relaxed),
            engagement_switches: self.engagement_switches.load(Ordering::Relaxed),
            location_hot_resolves: self.location_hot_resolves.load(Ordering::Relaxed),
            hotset_size: self.hotset_size.load(Ordering::Relaxed),
            soft_wait_arms: self.soft_wait_arms.load(Ordering::Relaxed),
            await_at_a_arms: self.await_at_a_arms.load(Ordering::Relaxed),
            await_at_a_wake_ok: self.await_at_a_wake_ok.load(Ordering::Relaxed),
            await_at_a_wake_reabort: self.await_at_a_wake_reabort.load(Ordering::Relaxed),
            cost_chose_wait_program: self.cost_chose_wait_program.load(Ordering::Relaxed),
            cost_chose_wait_handler: self.cost_chose_wait_handler.load(Ordering::Relaxed),
            cost_chose_optimistic_read_program: self
                .cost_chose_optimistic_read_program
                .load(Ordering::Relaxed),
            cost_chose_optimistic_read_handler: self
                .cost_chose_optimistic_read_handler
                .load(Ordering::Relaxed),
            early_abort_count: self.early_abort_count.load(Ordering::Relaxed),
            park_resume_at_k: self.park_resume_at_k.load(Ordering::Relaxed),
            park_resume_full_abort_reexecute: self
                .park_resume_full_abort_reexecute
                .load(Ordering::Relaxed),
            force_ordered_admit_reabort: self.force_ordered_admit_reabort.load(Ordering::Relaxed),
            soft_wait_wake_ok: self.soft_wait_wake_ok.load(Ordering::Relaxed),
            soft_wait_wake_reabort: self.soft_wait_wake_reabort.load(Ordering::Relaxed),
            serial_barrier_resolve: self.serial_barrier_resolve.load(Ordering::Relaxed),
            serial_barrier_defer: self.serial_barrier_defer.load(Ordering::Relaxed),
            handler_sstore_capture: self.handler_sstore_capture.load(Ordering::Relaxed),
            ordered_admit_snap_capture: self.ordered_admit_snap_capture.load(Ordering::Relaxed),
            ordered_admit_snap_credit: self.ordered_admit_snap_credit.load(Ordering::Relaxed),
            serial_barrier_clique: self.serial_barrier_clique.load(Ordering::Relaxed),
            jump_defer: self.jump_defer.load(Ordering::Relaxed),
            second_repair_await: self.second_repair_await.load(Ordering::Relaxed),
            first_repair_await: self.first_repair_await.load(Ordering::Relaxed),
            fanout_fr_collapse: self.fanout_fr_collapse.load(Ordering::Relaxed),
            fanout_validate_defer: self.fanout_validate_defer.load(Ordering::Relaxed),
            fanout_absorb: self.fanout_absorb.load(Ordering::Relaxed),
            edge_ordered_admit: self.edge_ordered_admit.load(Ordering::Relaxed),
            edge_wait_for: self.edge_wait_for.load(Ordering::Relaxed),
            edge_optimistic_read: self.edge_optimistic_read.load(Ordering::Relaxed),
            avoid_broadcasts: self.avoid_broadcasts.load(Ordering::Relaxed),
            canary_probes: self.canary_probes.load(Ordering::Relaxed),
            independent_optimistic_read: self.independent_optimistic_read.load(Ordering::Relaxed),
            spine_waits: self.spine_waits.load(Ordering::Relaxed),
            sketch_hot_size: self.sketch_hot_size.load(Ordering::Relaxed),
            data_publish_wakes: self.data_publish_wakes.load(Ordering::Relaxed),
            force_prefix_optimistic_read: self.force_prefix_optimistic_read.load(Ordering::Relaxed),
            prefer_admit: self.prefer_admit.load(Ordering::Relaxed),
            multi_spine_admit: self.multi_spine_admit.load(Ordering::Relaxed),
            quiet_pessimistic_revoke: self.quiet_pessimistic_revoke.load(Ordering::Relaxed),
            writer_identity_preserved: self.writer_identity_preserved.load(Ordering::Relaxed),
            ordered_admit_residual: self.ordered_admit_residual.load(Ordering::Relaxed),
            canary_reopen: self.canary_reopen.load(Ordering::Relaxed),
            writer_done_learned: self.writer_done_learned.load(Ordering::Relaxed),
            detect_accesses: self.detect_accesses.load(Ordering::Relaxed),
            predicted_essential_hits: self.predicted_essential_hits.load(Ordering::Relaxed),
            pcc_fire_at_a: self.pcc_fire_at_a.load(Ordering::Relaxed),
            pcc_roi_skip: self.pcc_roi_skip.load(Ordering::Relaxed),
            optimistic_read_occ_fast: self.optimistic_read_occ_fast.load(Ordering::Relaxed),
            occ_kernel_execs: self.occ_kernel_execs.load(Ordering::Relaxed),
            pcc_kernel_execs: self.pcc_kernel_execs.load(Ordering::Relaxed),
            occ_kernel_validates: self.occ_kernel_validates.load(Ordering::Relaxed),
            prefix_skip_roi_full_abort: self.prefix_skip_roi_full_abort.load(Ordering::Relaxed),
            force_prefix_as_pi: self.force_prefix_as_pi.load(Ordering::Relaxed),
            canary_live_verb: self.canary_live_verb.load(Ordering::Relaxed),
            inc_avoid_hits: self.inc_avoid_hits.load(Ordering::Relaxed),
            h_or_wait_door: self.h_or_wait_door.load(Ordering::Relaxed),
            morph_pessimistic_actuator: self.morph_pessimistic_actuator.load(Ordering::Relaxed),
            writer_validated_ordered_admit_gate: self
                .writer_validated_ordered_admit_gate
                .load(Ordering::Relaxed),
            flat_edgekey_sot: self.flat_edgekey_sot.load(Ordering::Relaxed),
            wait_for_dependency: self.wait_for_dependency.load(Ordering::Relaxed),
            wait_for_full_abort: self.wait_for_full_abort.load(Ordering::Relaxed),
            refuse_admit: self.refuse_admit.load(Ordering::Relaxed),
            ordered_admit_after_done: self.ordered_admit_after_done.load(Ordering::Relaxed),
            partial_abort_win: self.partial_abort_win.load(Ordering::Relaxed),
            partial_abort_attempt: self.partial_abort_attempt.load(Ordering::Relaxed),
            ready_width_mean: f64::from_bits(self.ready_width_sum_bits.load(Ordering::Relaxed)),
            idle_core_ns: self.idle_core_ns.load(Ordering::Relaxed),
            incarnation_gt0: self.incarnation_gt0.load(Ordering::Relaxed),
            reexec_entries: self.reexec_entries.load(Ordering::Relaxed),
            unfenced_reexec: self.unfenced_reexec.load(Ordering::Relaxed),
            ordered_admit_cohorts: self.ordered_admit_cohorts.load(Ordering::Relaxed),
            optimistic_read_cohorts: self.optimistic_read_cohorts.load(Ordering::Relaxed),
            refuse_ns: self.refuse_ns.load(Ordering::Relaxed),
            reexec_ns: self.reexec_ns.load(Ordering::Relaxed),
            optimistic_majority_block: self.optimistic_majority_block.load(Ordering::Relaxed) != 0,
            cost_ev_keep_ordered: self.cost_ev_keep_ordered.load(Ordering::Relaxed),
            cost_ev_demote_optimistic: self.cost_ev_demote_optimistic.load(Ordering::Relaxed),
            k_cap_demote: self.k_cap_demote.load(Ordering::Relaxed),
            commute_skip: self.commute_skip.load(Ordering::Relaxed),
            batch_repair: self.batch_repair.load(Ordering::Relaxed),
            conflict_promote: self.conflict_promote.load(Ordering::Relaxed),
            conflict_ignore: self.conflict_ignore.load(Ordering::Relaxed),
            admit_seed_begin_ns: self.admit_seed_begin_ns.load(Ordering::Relaxed),
            runnable_set_width_mean: {
                let n = self.runnable_width_n.load(Ordering::Relaxed);
                if n == 0 {
                    0.0
                } else {
                    self.runnable_width_sum.load(Ordering::Relaxed) as f64 / n as f64
                }
            },
            visibility_opt: self.visibility_opt.load(Ordering::Relaxed),
            visibility_wait_released: self.visibility_wait_released.load(Ordering::Relaxed),
            visibility_ordered_tip: self.visibility_ordered_tip.load(Ordering::Relaxed),
            resolve_commit: self.resolve_commit.load(Ordering::Relaxed),
            resolve_partial_rebind: self.resolve_partial_rebind.load(Ordering::Relaxed),
            resolve_partial_rewind: self.resolve_partial_rewind.load(Ordering::Relaxed),
            resolve_ordered_replay: self.resolve_ordered_replay.load(Ordering::Relaxed),
            resolve_full_replay: self.resolve_full_replay.load(Ordering::Relaxed),
            sf_schedule_picks: self.sf_schedule_picks.load(Ordering::Relaxed),
            occ_schedule_picks: crate::specfence::executor::occ_pick_calls(),
            steal_n: self.steal_n.load(Ordering::Relaxed),
            refuse_fill_n: self.refuse_fill_n.load(Ordering::Relaxed),
            mid_promote_n: self.mid_promote_n.load(Ordering::Relaxed),
            mid_promote_veto_n: self.mid_promote_veto_n.load(Ordering::Relaxed),
            learn_e1_n: self.learn_e1_n.load(Ordering::Relaxed),
            learn_e2_n: self.learn_e2_n.load(Ordering::Relaxed),
            learn_e3_n: self.learn_e3_n.load(Ordering::Relaxed),
            learn_e4_n: self.learn_e4_n.load(Ordering::Relaxed),
            learn_e5_n: self.learn_e5_n.load(Ordering::Relaxed),
            learn_e6_n: self.learn_e6_n.load(Ordering::Relaxed),
            prior_plant_n: self.prior_plant_n.load(Ordering::Relaxed),
            idle_core_frac: {
                let idle = self.idle_core_ns.load(Ordering::Relaxed);
                let busy = self.worker_busy_ns.load(Ordering::Relaxed);
                let den = idle.saturating_add(busy);
                if den == 0 {
                    0.0
                } else {
                    idle as f64 / den as f64
                }
            },
            resolve_apply_n: self.resolve_apply_n.load(Ordering::Relaxed),
            sf_mv_wait_released_reads: self.sf_mv_wait_released_reads.load(Ordering::Relaxed),
            sf_mv_ordered_tip_reads: self.sf_mv_ordered_tip_reads.load(Ordering::Relaxed),
            explore_n: self.explore_n.load(Ordering::Relaxed),
            began_from_prior: self.began_from_prior.load(Ordering::Relaxed) != 0,
            access_wait_once: self.access_wait_once.load(Ordering::Relaxed),
            access_wait_suppressed: self.access_wait_suppressed.load(Ordering::Relaxed),
            access_never_wait: self.access_never_wait.load(Ordering::Relaxed),
            prefix_resume_n: self.prefix_resume_n.load(Ordering::Relaxed),
            full_from_zero: self.full_from_zero.load(Ordering::Relaxed),
            fail_k_n: self.fail_k_n.load(Ordering::Relaxed),
            fail_k_min: self.fail_k_min.load(Ordering::Relaxed),
            fail_k_max: self.fail_k_max.load(Ordering::Relaxed),
            fail_k_hist: std::array::from_fn(|i| self.fail_k_hist[i].load(Ordering::Relaxed)),
            sf_early_tip_n: self.sf_early_tip_n.load(Ordering::Relaxed),
            sf_publish_wake_n: self.sf_publish_wake_n.load(Ordering::Relaxed),
            sf_wait_once_consume_n: self.sf_wait_once_consume_n.load(Ordering::Relaxed),
            sf_detect_before_n: self.sf_detect_before_n.load(Ordering::Relaxed),
            sf_avoid_publish_n: self.sf_avoid_publish_n.load(Ordering::Relaxed),
            sf_resolve_after_fail_n: self.sf_resolve_after_fail_n.load(Ordering::Relaxed),
            estimate_block_sf: self.estimate_block_sf.load(Ordering::Relaxed),
        }
    }
}
