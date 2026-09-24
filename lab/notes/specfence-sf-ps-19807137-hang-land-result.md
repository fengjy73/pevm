# SpecFence — 19807137 leftover hang land result

**Date:** 2026-09-21  
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`  
**Tip:** `3c742ef` (next-leftover Detect + claim bitset + Validated-only leftover_min advance)  
**Baseline leftover:** `8a5c16f` width-1 `plant_global` Detect-star + abort done-stamp.  
**This land does not restore** `Scheduler::next_task*` / `next_task_with_wave_ready` / `validate_occ_stage`.  
**Width-8 leftover Detect is not re-landed.**

---

## Hypothesis and what landed

Width-1 `plant_global` Detect-starred every surplus leftover onto `leftover_min=405`. 405 `may_execute` then `add_dependency`-parked on a later Detect waiter (`n_unf=307`, `n_run=1`).

Middle ground on this tip:

1. **Claim token** — leftover_min is elected; surplus is pick-refused via a **bitset** (DashSet on `may_execute` heap-aborted 6196166).
2. **Detect only the next leftover** — not the block-wide wait-set. Dropping Detect entirely heap-aborted 6196166 (same class as drop `plant_global`).
3. **Idle advance leftover_min only after Validated** — advancing on Executed let the next leftover run while leftover_min was still validating.
4. leftover_min Blocked on a later waiter: flush Detect + detach, **no recover** (recover leftover_min heap-aborted 6196166 reuse).

Not re-landed: width-8 leftover Detect, both-sides recover, overflow claim-only, product-path drop of `plant_global`.

---

## Instant-off Soft=0 @8

| Block | N | OCC med | SF cold | SF reuse | Soft | occ_picks | Status |
|-------|--:|--------:|--------:|---------:|-----:|----------:|--------|
| **3356896** | 3 | ~1.5 ms | 4.265 | 6.580–11.789 | 0 | **0** | **green** Learn e1=233 e2=29 e6=1 explore=0 `began_from_prior` |
| **19807137** | 1 | 2.373 s | hang | — | 0 | — | **open** leftover_min walks (405→514) then ghost-Executing mill `min_st=exec n_run=0 n_unf=198` |
| **6196166** | 3 | ~2.4 ms | 5.6–19.4 first SF ok | heap | 0 | **0** on SF[0] | **open** first-SF / reuse `double free` / `munmap_chunk` / unsorted list |
| **19469101** | ≥3 | — | — | — | 0 | — | pending (not re-run this tip) |
| **14396881** | ≥3 | — | — | — | 0 | — | pending |
| **14689597** | ≥3 | — | — | — | 0 | — | pending |
| complete_arch Soft=0 | — | — | — | — | 0 | — | **heap** `munmap_chunk` on this tip |

198 leftover_min **progresses** without starring 307 Detect waiters. It then sticks as a WaitForDependency/nonce ghost (`Executing`, worker gone). Heal recovers; leftover_min re-parks. Not papered with OCC pick.

619/complete_arch heap is the known “drop block-wide leftover Detect” class. Next-leftover Detect is not enough to keep those heaps off.

Learn B1–B7 hold on 3356896. `occ_schedule_picks=0` on SF. `next_task*` still banned.
