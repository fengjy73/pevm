# SpecFence — 19807137 ghost-Executing leftover_min land result

**Date:** 2026-09-21  
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`  
**Tip:** `7cb1bc8`  
**Baseline leftover:** `8a5c16f` width-1 `plant_global` Detect-star + abort done-stamp.  
**This land does not restore** `Scheduler::next_task*` / `next_task_with_wave_ready` / `validate_occ_stage`.  
**Width-8 leftover Detect is not re-landed.**  
**Earlier←later leftover_min recover is not re-landed.**

---

## Hypothesis and what landed

leftover_min walked 405→184→342→514 then stuck **ghost Executing**: nonce / WaitForDependency `Blocking(tx-1)` on a writer already done. `add_dependency` returned false, leftover_min stayed `Executing`, heal recovered, leftover_min re-parked (`n_unf≈198`).

Root of the unused WaitReleased: leftover_min is a **claim token**, typically ungated, so `VisibilityPolicy::for_ready` chose **Opt**. Worker `WaitReleased` was discarded in `VmDb::set_tx`. Opt raced Estimate then synthetic `Blocking(tx-1)`.

Also: `leftover_wake` on **every** producer-done `force_push`ed leftover_min while it was `ST_RUNNING` — strongest 6196166 first-SF heap hypothesis (`double free` / `munmap_chunk`).

Landed on this tip (kept claim-token + next-leftover-only Detect + bitset surplus refuse):

1. **`for_ready`: leftover_min → WaitReleased** even when ungated. `set_tx` now sees the same vis.
2. **leftover_min skip-park** when the blocker is later, already done/validated, or leftover surplus. `park_estimate_blocking` on those writers → `InconsistentRead` (Retry). leftover_min + done/surplus prefix skips nonce `Blocking`; LackOfFund/NonceTooHigh → Retry. Non-leftover keeps OCC `Blocking(tx-1)` (a `!pred_done` gate InvalidNonce-aborted 198 tx 120).
3. **`leftover_wake` only when leftover_min itself commits.** Non-min producer-done does not `force_push` leftover_min.
4. leftover_min Blocked on a later waiter: flush Detect + detach, **no recover**. leftover_min Blocked on an earlier writer: Detect-plant (skipping that plant hung 619 reuse).

---

## Discarded this cut

| Hypothesis | Result |
|------------|--------|
| leftover_min always `may_execute` while Detect-gated | **6196166 heap** `double free` |
| leftover_min recover + `force_push` after Blocked on done/surplus | **198 first-SF heap** `unaligned chunk` / unsorted list |
| leftover_min never Detect-plants on `w<tx` | **619 reuse hang** (leftover_min `ST_WAIT`, no pred to release) |
| width-8 leftover Detect | not re-landed (prior 14396881 / complete_arch heap) |
| earlier←later leftover_min recover | not re-landed (prior 619 reuse heap) |
| Paper OCC pick / `next_task*` | banned |

---

## Instant-off Soft=0 @8 (`specfence_3356896_compare`)

| Block | N | OCC med | SF cold | SF reuse | Soft | occ_picks | Status |
|-------|--:|--------:|--------:|---------:|-----:|----------:|--------|
| **3356896** | 3 | 1.418 ms | 2.727 | 1.605 | 0 | **0** | **green** Learn e1=215 e2=26 e6=1 explore=0 `began_from_prior` |
| **6196166** | 3 | 2.481 ms | 25.415 | 23.915 | 0 | **0** | **green** no heap (was SF[0] then heap @2bd7f25) |
| **19807137** | 1 | 2.332 s | heap | — | 0 | — | **open** leftover_min=205 Ready `may_execute=false` pred=204 `n_unf=527` then `malloc(): unaligned tcache chunk` |
| complete_arch Soft=0 | — | — | — | — | 0 | — | **heap** `double free or corruption (out)` |

198 leftover_min **progresses** (WaitReleased now actually applies; skip-park on done writers). It then Detect-gates on tx-1 leftover 204 and the refuse mill heap-aborts. Not papered with OCC pick.

619 first-SF + reuse stay off the heap when leftover_min still Detect-plants live earlier writers and leftover_wake is leftover_min-commit only. complete_arch is the same heap class as drop block-wide leftover Detect; leftover_wake-only was not enough.

Learn B1–B7 hold on 3356896. `occ_schedule_picks=0` on SF. `next_task*` still banned.

---

## Still open

leftover_min must **commit** when it Blocks on an already-done **or leftover-surplus** writer **without** Detect-gating itself off pick and **without** `force_push`/`recover` while `ST_RUNNING`. The 198 mill is leftover_min=205 gated on 204 (`n_unf=527`) — 204 is unfinished leftover, leftover_min cannot wait for it and cannot steal `may_execute` without re-landing the 619 heap.
