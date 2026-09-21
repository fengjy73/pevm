# SpecFence Parallel Spine (SF-PS) — land result

**Date:** 2026-09-21  
**PR baseline:** `cursor/specfence-shell-cut-redig-6a8f` (PR #43)  
**Design SoT:**
- [`specfence-first-class-architecture-redesign-v1.md`](specfence-first-class-architecture-redesign-v1.md)
- [`specfence-sf-ps-full-land-v1.md`](specfence-sf-ps-full-land-v1.md)

## What landed

SpecFence mode no longer treats Block-STM/OCC as the protocol root.

| Face | Land |
|------|------|
| **A. Schedule** | `schedule::pick(RunnableSet)` is the SpecFence main pick. `next_occ_task` is OCC/PCC only. Empty wait-set = Avoid=noop antichain, still on SF-PS. |
| **B. Visibility** | `VisibilityPolicy::{Opt, WaitReleased, OrderedTip}`. Opt = DAG independent set (Avoid=noop), **not** `ConcurrencyMode::Occ`. |
| **C. Resolve** | Validate emits `ResolvePlan::{Commit, PartialAbortRebind, PartialAbortRewind, OrderedReplay, FullReplay}`. Edged path prefers Resolve over bool→incarnation++. |
| **D. Learn→G** | Thin n≤176 `train_hat` stays light (no Win_8 mill). Under-covered spines drop Full/Seg as success. `skip_ungated_*` is compat Avoid=noop, not a Learn target. |
| **E. Docs/metrics** | `mod.rs` / glossary / SPECFENCE.md are SF-PS. Metrics: RunnableSet width, visibility counts, ResolvePlan histogram, `sf_schedule_picks` / `occ_schedule_picks`. |

## Call-graph evidence

SpecFence worker:

```
next_sf_task → schedule::pick(RunnableSet)
  → ProducerStage (conflict subgraph)
  → scheduler.next_task_with_wave_ready(wave, ready)
Execute(VisibilityPolicy) → validate_specfence → ResolvePlan.apply
```

`next_occ_task` is incremented only on `ConcurrencyMode::Occ` / `Pcc`.  
Unit + fixture test: `specfence_sf_ps_pick_never_calls_next_occ_task`.

## Hard constraints

- Soft=0 (`soft_wait_arms` stays 0)
- seq≡par (existing specfence / erc20 / iter11 fixtures)
- lazy-update never OrderedAdmit
- OCC contrast engine kept

## Numbers

Filled after Instant-off / N=3 reuse sweep on this PR. Short-term TPS jitter vs PR43 is reported honestly; we did **not** retreat to the OCC main loop to chase a ratio.
