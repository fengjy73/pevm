# SpecFence sub-grain + learning fixes — impl map

**Date:** 2026-09-13  
**PR:** https://github.com/fengjy73/pevm/pull/3  
**Source contract:** `lab/notes/specfence-failing-tx-subgrain-and-learning-gaps.md`  
**Vocabulary:** Spec = Region; Fence = Bind / WaitFor / serial-lane + admit;
Unfenced = optimistic access (not Spec).

---

## File:fn

| Item | Contract | Symbol |
|------|----------|--------|
| **1. Done→Data residual Bind** | Avoid/Fence + writer Done∅Data → Bind residual (last Data or Storage). `UnfencedWriterDone` is not hang-freedom on must_wait. Visibility spin then residual SoT. | `HotSketch::{install_data_residual,install_done_residual,residual_bind}`; `Vm::fence_wait_for` (no Unfenced fallthrough on `must_wait`); `Vm::{bind_residual_data,bind_done_residual}` |
| **2. R1 when value-stable** | RebindOnly when snap/FF match or U4 identity + Estimate cleared + !true_suffix. R2/R4 only when identity lost. | `PartialRetryTable::{identity_held,identity_stable_match}`; `pevm.rs` `try_validate` (`value_stable` uses identity+FF) |
| **3. Learn writer_done / u_aa** | Trace verbs update H/Avoid priors **and are read** by Fence/admit. Long-tail ℓs enter `pack_top_locations`. | `LiveLearner::{note_writer_done,writer_done_hot,bind_cover}`; `pack_top_locations` ranks `writer_done`/`u_aa`; `Vm::maybe_wait_specfence` `note_hot` from those priors |
| **4. Multi-spine PreferAdmit law** | Ready-set ⊆ Region unfinished spine (secondary ℓs, not only star). | `HotSketch::ready_spine_writers`; `Vm::maybe_wait_specfence` / `fence_wait_for` admit Ready on all live spines; `Scheduler::admit_spine_writers` |
| **5. Canary reopen / early Fence** | Prod path calls `reopen_canary_if_probe_done` when probe Done ∧ !Avoid. After Avoid, residual Bind collapses late first_fence_seq. | `HotSketch::reopen_canary_if_probe_done` (called from `Vm::maybe_wait_specfence`); Avoid already `must_fence` in `choose_edge_action` |
| **6. AEC retire / structural reattach** | Live π = `choose_edge_action` only. `choose_resolve` / AdaptiveParams αβγδ / `meta_budget` / `d_wait` retired (not EV Await doors). Structural reads: `wait_depth_prior` → fan-out Fence pressure; `bind_success_total` → `bind_cover`; `ChainTemplate.confidence` → `template_live` serial_lane. | `SpecFenceCtx::choose_resolve` (`#[allow(dead_code)]` retired); `MorphWeights::wait_depth_prior` read in `maybe_wait_specfence`; `HotSketch::template_live` |

---

## Tests

| Test | File |
|------|------|
| `done_without_data_installs_storage_residual` | `sketch.rs` |
| `ready_spine_writers_covers_secondary_l` | `sketch.rs` |
| `reopen_canary_after_probe` | `sketch.rs` (existing + prod caller) |
| `r1_value_stable_when_identity_and_ff_match` | `rem.rs` |
| `writer_done_enters_hot_prior` | `learner.rs` |
| `g4_bind_and_wait_useful_and_publish_credit` | `learner.rs` (`bind_cover` live) |
| `subgrain_done_bind_r1_canary_prefer_admit` | `tests/specfence.rs` |

---

## Learning unused list (after this cut)

| State | Status |
|-------|--------|
| writer_done / u_aa | **wired** → H / `pack_top` / Fence |
| `bind_success_total` | **wired** → `bind_cover` → H |
| `prefer_admit` | **wired** as ready-set law |
| `wait_depth_prior` | **wired** as fan-out Fence actuator (not EV) |
| `ChainTemplate.confidence` | **wired** via `template_live` serial_lane |
| canary reopen | **wired** on prod path |
| `d_wait` / `cost_margin` | **retired** (lab JSON only) |
| `meta_budget` / SoftWait `meta_ops` | **retired** (Soft=0; not edge π) |
| AEC EV / AdaptiveParams αβγδ / `choose_resolve` | **retired** (not live π) |
| engagement Quiet/Storm edge verbs | **not** edge verbs; writer_done/H is the actuator |

---

## Hard bans (this cut)

| Ban | Status |
|-----|--------|
| SoftWait storms | Soft=0 (no new SoftWait arms) |
| EV Await / AdaptiveParams-as-θ | Await@a unchanged; `choose_resolve` not on access path |
| tip-identity Bind gate | Bind still on Data / residual |
| OCC-retry as π | no |
| Storm morph as π | fan-out prior is structural Fence pressure only |
| 597-only hardcodes | none |
