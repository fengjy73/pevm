# SpecFence — 19807137 leftover hang land result

**Date:** 2026-09-21  
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`  
**Baseline leftover:** `8a5c16f` width-1 `plant_global` + abort done-stamp.  
**This land does not restore** `Scheduler::next_task*` / `next_task_with_wave_ready` / `validate_occ_stage`.  
**Width-8 leftover Detect is not re-landed.**

---

## Hypothesis

Width-1 `plant_global` Detect-starred every surplus leftover onto `leftover_min=405`. 405 `may_execute` then `add_dependency`-parked on a later Detect waiter. Flush-only left 405 Aborting and the wait-set parked on 405 (`n_unf=307`, `n_run=1`).

Same-ℓ `plant_observed_window` alone is not enough (drop `plant_global` hung `n_unf=505` + 6196166 heap). Block-wide Detect is too much. Width-8 leftover Detect hung differently and heap-aborted 14396881 / complete_arch.

**Middle ground:** leftover_min is a **claim token**, not a Detect wait-set.

- `plant_global_leftover` records `leftover_claimed` and elects `leftover_min`.
- Surplus is pick-refused (`leftover_surplus`) with **no** `note_consumer_on`.
- When leftover_min commits, elect the next claimed leftover and wake only that head.
- If leftover_min still `add_dependency`-parks on a later waiter: flush Detect, detach the inverted scheduler edge, recover leftover_min only. Do not recover later (both-sides recover milled `n_unf=437`).

---

## Instant-off Soft=0 @8 (fill after runs)

| Block | N | OCC med | SF cold | SF reuse | Soft | occ_picks | Status |
|-------|--:|--------:|--------:|---------:|-----:|----------:|--------|
| **19807137** | ≥3 | ~2.37 s | — | — | 0 | — | pending |
| **6196166** | ≥3 | — | — | — | 0 | — | pending |
| **19469101** | ≥3 | — | — | — | 0 | — | pending |
| **3356896** | ≥3 | — | — | — | 0 | — | pending |
| **14396881** | ≥3 | — | — | — | 0 | — | pending |
| **14689597** | ≥3 | — | — | — | 0 | — | pending |
| complete_arch Soft=0 | — | — | — | — | 0 | — | pending |

Learn B1–B7 must hold. `occ_schedule_picks=0` on SF.
