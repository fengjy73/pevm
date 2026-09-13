# SpecFence V5-P1 — single Lean repair story — status

**Date:** 2026-09-07 (Asia/Shanghai)  
**Branch:** `specfence`  
**Parent tip:** `0c25731` (V5-P0 shovel)  
**Authority:** `lab/notes/specfence-v5-first-principles-clean-slate.md` §7 V5-P1

## Goal

One hang-free Lean abort repair path:

> when certified prefix exists → force-bind certified + selective invalidate;  
> otherwise FullRestart from head.

No inspect / PC jump on default Lean. Do **not** restore Wait ladders / fanout→WaitHard.

## What landed

### Single helper — `PartialRetryTable::apply_lean_abort_repair`

```text
if plan_partial_retry → certified non-empty:
  set_force_bind(certified)
  clear_repair          # drop stale RewindTo / FF (Lean never arms inspect)
  → LeanAbortRepair::ForceBind { reexec_cost: 1.2 }
else:
  clear_force_bind; clear_repair
  → LeanAbortRepair::FullRestart { reexec_cost: 2.2 }
```

Caller (`try_validate` lean arm) always selective-invalidates + `finish_validation_fenced`
(FullRestart-from-head reexec with force_prefix Bind-when-Data / SpecRead-else).

### Path collapse in `pevm.rs`

| Before | After |
|--------|-------|
| ~864 lean: inline `plan_partial_retry` + `set_force_bind` / clear + dead empty-certified arm | **one** `apply_lean_abort_repair` call |
| ~984 research: RewindTo + duplicated FullRestart×2 | RewindTo kept; FullRestart arms → `research_full_restart_invalidate` |
| RebindOnly pre-abort (~841) | unchanged (still before `try_validation_abort`) |

Research-inspect (`SPECFENCE_ENABLE_INSPECT=1` → `!lean_tx`) still prefers
`plan_repair` → RewindTo+FF; Lean default never calls it.

### Hang-free invariants kept

- `force_prefix`: Bind when Data ready, else SpecRead — **never WaitHard without Data**
  (SoftWait livelock class from abort-cheapening note).
- EarlyAbort / SoftWait wake still set force-bind; lean execute ignores RewindTo PC jump
  (`rewind_resume = !lean && is_rewind_resume`).
- AEC `choose_action` unchanged.

## Tests

```text
cargo test -p pevm --lib
  → 78 passed (incl. apply_lean_abort_repair_force_bind_no_rewind,
                 apply_lean_abort_repair_full_restart_without_prefix)

cargo test -p pevm --test specfence
  → 23 passed, 0 failed, 13 ignored (M1* research-only)
```

## Smoke (G7 harness @8)

```text
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_g7_smoke
```

| Block | SoftWait | WaitHard | SF/OCC | vs P0 |
|-------|----------|----------|--------|-------|
| **14689597** | **27** | 27 | **0.154** | P0 SoftWait 23 / SF/OCC 0.151 |
| 19606599 | 17 | 17 | 0.336 | P0 SoftWait 12 / 0.314 |
| 19469097 | 66 | 66 | 0.368 | P0 SoftWait 33 / 0.322 |
| 19606598 | 2 | 2 | 0.296 | P0 SoftWait 2 / 0.357 |
| **mean** | | | **0.288** | P0 mean 0.286 |

**SoftWait on 597 stays scarce** (27 ≪ G7 428; in ~20–40 band). SF/OCC ≈ P0
(noise). Prefer not chase SoftWait up.

Artifacts: `lab/results/g7-sf-occ-smoke.json`, `lab/results/g7-flip-smoke.json`,
`lab/results/g7-v5-p1-smoke.run.log`.

## Files

- `crates/pevm/src/specfence/rem.rs` — `LeanAbortRepair` + `apply_lean_abort_repair` + unit tests; V5-P1 docs
- `crates/pevm/src/specfence/mod.rs` — export + RepairPlant comment
- `crates/pevm/src/pevm.rs` — lean abort → helper; research FullRestart dedupe
- `lab/notes/specfence-v5-p1-repair-status.md` — this note

## Remaining (V5-P2+)

- θ quality: wake latency / reexec into EV; meta budget → SpecRead
- Bind quality so 597 SF/OCC recovers without Wait storms
- Optional lite EffectBoundary / mid-tx RewindTo+FF **without** inspect tax (hang-free only)
- Later: delete unused Bayes bool helpers / HeatMap demote

## Forbidden

Restoring `fanout_hint → WaitHard`, Boolean Wait ladders, Heat/account sticky Wait,
or default-on inspect to chase SF/OCC.
