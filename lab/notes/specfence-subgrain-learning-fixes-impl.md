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
| **1. Done→Data residual Bind** | Avoid/Fence + writer Done∅Data → Bind residual (last Data or Storage). `UnfencedWriterDone` is not hang-freedom on must_wait. No hope-spin: park Executing only, then residual SoT. | `HotSketch::{install_data_residual,install_done_residual,residual_bind}`; `Vm::fence_wait_for` (no Unfenced fallthrough on `must_wait`); `Vm::{bind_residual_data,bind_done_residual}` |
| **2. R1 when value-stable** | RebindOnly when snap/FF match or U4 identity + Estimate cleared + !true_suffix. R2/R4 only when identity lost. | `PartialRetryTable::{identity_held,identity_stable_match}`; `pevm.rs` `try_validate` (`value_stable` uses identity+FF) |
| **3. Learn writer_done / u_aa** | Trace verbs update H/Avoid priors **and are read** by Fence/admit. Long-tail ℓs enter `pack_top_locations`. | `LiveLearner::{note_writer_done,writer_done_hot,bind_cover}`; `pack_top_locations` ranks `writer_done`/`u_aa`; `Vm::maybe_wait_specfence` `note_hot` from those priors |
| **4. Multi-spine PreferAdmit law** | Ready-set ⊆ Avoid unfinished spines (cap 8). Live on Unfenced path; this-ℓ unfinished always admitted. | `HotSketch::ready_spine_writers`; `Vm::maybe_wait_specfence` (Unfenced); `fence_wait_for` this-ℓ only; `Scheduler::admit_spine_writers` |
| **5. Canary reopen / early Fence** | Prod path calls `reopen_canary_if_probe_done` when probe Done ∧ !Avoid. After Avoid, residual Bind collapses late first_fence_seq. | `HotSketch::reopen_canary_if_probe_done` (called from `Vm::maybe_wait_specfence`); Avoid already `must_fence` in `choose_edge_action` |
| **6. AEC retire / structural reattach** | Live π = `choose_edge_action` only. `choose_resolve` / AdaptiveParams αβγδ / `meta_budget` / `d_wait` retired (not EV Await doors). Structural reads: `wait_depth_prior` → H `note_hot` (not OR'd into essential); `bind_success_total` → `bind_cover`; `ChainTemplate.confidence` → `template_live` serial_lane. | `SpecFenceCtx::choose_resolve` (`#[allow(dead_code)]` retired); `MorphWeights::wait_depth_prior` read in `maybe_wait_specfence`; `HotSketch::template_live` |

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
| `wait_depth_prior` | **wired** as H `note_hot` (not OR'd into essential) |
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
| Storm morph as π | fan-out prior is structural H only |
| 597-only hardcodes | none |

---

## Process (N=3 @8, last SF iter)

`unfenced_writer_done` = **0** on 597 / 599 / 097 (post-U1: 589 / 3237 / 1595).
`unfenced_after_avoid` reason = 0. SoftWait Soft = 0. Await@a = 0.

| Block | writer_done | bind | wait | indep | prefer_admit | R1 | rewind | bind_residual | canary_reopen |
|------:|------------:|-----:|-----:|------:|-------------:|---:|-------:|--------------:|--------------:|
| **14689597** | **0** | 1576 | 80 | 605 | 4 | 0 | 111 | 781 | 22 |
| **19606599** | **0** | 5150 | 47 | 1094 | 14 | 10 | 211 | 4371 | 288 |
| **19469097** | **0** | 2439 | 51 | 687 | 4 | 7 | 212 | 1920 | 84 |

Reason histograms: Bind-after-Avoid covers the star; residual Bind absorbed Done∅Data. Independents stay Unfenced.

JSON: `lab/results/exec-process-{14689597,19606599,19469097}-subgrain-fixes.json`,
`lab/results/subgrain-fixes-xblock-{sf-occ,flip}.json`, `lab/results/subgrain-fixes-xblock.json`

---

## Wall / TPS honesty vs OCC (N=3 @8)

Compare **ratios** to post-U1 (mean SF/OCC 0.376; 597 ~3.1×). This cut mean SF/OCC = **0.260**.

| Block | SF wall med | OCC wall med | SF/OCC wall | SF/OCC TPS | Soft |
|------:|------------:|-------------:|------------:|-----------:|-----:|
| **14689597** | **23.3** | **6.3** | **3.7×** | **0.202** | 0 |
| **19606599** | **62.1** | **10.7** | **5.8×** | **0.241** | 0 |
| **19469097** | **30.0** | **9.1** | **3.3×** | **0.235** | 0 |
| **19606598** | **4.1** | **1.5** | **2.8×** | **0.362** | 0 |

597/097 sit in the post-U1 3–4.5× band. 599 is **5.8×** — residual Bind + repair still inflate wall; not a makespan win. 597 p90 (106) shows one slow iter; median is the honest figure.

xblock 598 sf-cold did not hang. Soft=0 throughout.

Wall-recovery after the first cut (Ready-park / full-spine walk / canary mill / `wait_depth`∨essential): park Executing only; PreferAdmit Avoid-only cap 8 on Unfenced; one reopen per ℓ; `wait_depth_prior` is H only; no Done hope-spin. Numbers above are the recovered N=3 @8 (pre hope-spin removal). Fresh xblock on this tip is recorded in the same JSON names.
