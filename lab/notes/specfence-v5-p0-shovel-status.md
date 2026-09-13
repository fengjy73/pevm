# SpecFence V5-P0 shovel — status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch tip (pre-commit):** on `specfence` after shovel  
**Authority:** `lab/notes/specfence-v5-first-principles-clean-slate.md`

## Goal

Behavioral shovel of sedimentary SpecFence *control* so one algorithm remains on the hot path:

**Block-STM + FenceGraph SoftWait + AEC `choose_action` + continuous θ learner + Lean force-bind/selective repair.**

Do **not** restore fanout→WaitHard ladders. Do **not** enable inspect/jump by default.

## What was deleted / no-op'd (SpecFence path)

| Item | Action |
|------|--------|
| `seed_wait_regions` SoftWait/account Wait arming | **No-op** unless `ConcurrencyMode::Pcc`. SpecFence returns immediately. Caller in `pevm.rs` already PCC-gated. |
| `SpecFenceCtx::should_wait_account` | Already always `false` for SpecFence (diagnostic stub). Comment updated V5-P0. |
| `regions.promote_account` on SpecFence | Already early-return gated in `vm.rs` (`promote_on_conflict` / `promote_region` / `promote_if_multi_writer`). Left PCC calls intact. |
| `should_wait_location` Bayes bool OR | **Shoveled:** SpecFence always returns `false`. PCC still sticky-true. SpecFence Wait only via `choose_action` → FenceGraph SoftWait in `Vm::maybe_wait`. |
| `BayesMap::should_wait_hard` as SpecFence π | **Removed from SpecFence control.** Kept as legacy helper + unit tests; Beta posteriors remain EV features (`posterior_conflict` / bind success). |
| `RegionTable::should_wait` authority | Documented as mirror/PCC probe only. SoftWait SoT = FenceGraph. Mirrors via `promote_from_bayes` / `promote_location` retained temporarily. |
| AdaptiveEngagement abort_rate → HotSet escalate | **`note_abort` always returns `false`** (metrics-only). Removed whole-write-set `hotset.insert` storm on escalate in `pevm.rs`. Execute stays Lean unless `SPECFENCE_ENABLE_INSPECT`. |
| HotSet as Wait gate | Unchanged structurally; π already uses membership only as `fanout_hint` feature. H_w/H_a densification on conflict locs kept as features. Storm escalate deleted. |
| mod.rs crate docs | **Rewritten to SpecFence v5**; M1a–M1l = research-only behind flag; cite clean-slate note. |

## What was kept (purified)

- FenceGraph SoftWait + WavePark (M2/P4)
- `choose_action` AEC argmin EV (sole π choke)
- `LiveLearner` / `InterBlockPrior` as θ features
- PartialRetry force-bind + selective invalidate (abort cheapening)
- `RwPriorMap` Bind features
- Block-STM validate / ESTIMATE fence
- finegrain collectors (lab opt-in)
- `boundary.rs` inspect plant (behind `SPECFENCE_ENABLE_INSPECT`)

## Key behavioral diffs

1. **One π:** SpecFence `maybe_wait` → `choose_resolve` / `choose_action` only. No Bayes-bool second policy via `should_wait_location`.
2. **One Wait authority:** SoftWait arms only on `ResolveAction::WaitHard` → `dag.arm_soft`. RegionTable Wait bits = mirrors.
3. **No engagement ladder:** abort storms no longer force-insert write-sets into HotSet; engagement does not flip execute mode.
4. **Priors ≠ SoftWait:** inter-block top-ℓ still warm-starts HotSet/Bayes tracking; never arms SoftWait at block start.

## Tests

```text
cargo test -p pevm --lib
  → 76 passed, 0 failed

cargo test -p pevm --test specfence
  → 23 passed, 0 failed, 13 ignored (M1* research-only)
```

## Smoke (G7 harness @8)

```text
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_g7_smoke
```

| Block | SoftWait | WaitHard | SF/OCC | vs prior tip |
|-------|----------|----------|--------|--------------|
| **14689597** | **23** | 23 | **0.151** | prior SoftWait~20 / SF/OCC~0.161 |
| 19606599 | 12 | 12 | 0.314 | |
| 19469097 | 33 | 33 | 0.322 | |
| 19606598 | 2 | 2 | 0.357 | |
| **mean** | | | **0.286** | |

**SoftWait stays scarce** (≪ G7 428). SF/OCC on 597 slightly lower than 0.161 — expected noise / shovel; **do not chase SoftWait up**.

Artifacts: `lab/results/g7-sf-occ-smoke.json`, `lab/results/g7-flip-smoke.json`.

## Remaining debt (V5-P1 / P2)

### V5-P1 — Single repair story
- Lean abort always force-bind+selective when prefix exists; one path.
- Delete dead FullRestart branches where safe.
- Optional: lite EffectBoundary / mid-tx RewindTo+FF without inspect tax (hang-free only).

### V5-P2 — θ quality
- Wire wake latency / reexec into EV more tightly.
- Meta budget as measured tax → SpecRead.
- Improve Bind quality so 597 SF/OCC recovers without Wait storms.

### Later cleanup
- Delete unused Bayes bool helpers / `record_bayes_wait` / `HotSet::insert` / account Wait maps once PCC isolation is confirmed.
- Rename modules per clean-slate target (`fence.rs` / `policy.rs` / …) — behavioral shovel first (done).
- HeatMap end-of-block `update_heat` still runs under SpecFence but does not arm SoftWait — demote/remove later.
- Trim metrics eras / duplicate counters.

## Forbidden

Restoring `fanout_hint → WaitHard`, `D_WAIT` Boolean ladders, Heat/account sticky Wait, or default-on inspect to chase SF/OCC.
