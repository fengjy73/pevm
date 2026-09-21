# SpecFence true spine — remaining hang land result

**Date:** 2026-09-21  
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`  
**SoT:** `specfence-sf-ps-remaining-hang-land-v1.md`  
**This land does not restore** `Scheduler::next_task*` / `next_task_with_wave_ready` / `validate_occ_stage` on SpecFence.

Raw Instant-off (gitignored): `lab/results/sf-ps-remain-*.json`.

---

## 1. Verified root causes (not papered with OCC pick)

### 1a. 6196166 heap-abort after `plant_observed_waw`

DashMap is not reentrant. `note_producer_done` holding `consumers.get` across `may_execute` (`consumers.get` again) corrupted the heap (`free(): invalid pointer` / `corrupted size vs. prev_size` / `double free`).

**Fix (kept):** drop the `consumers` shard before `may_execute`. Never walk `waiters`/`consumers` from `note_abort_reincarnate`. Never `ungate` + `recover_executing` on the Blocked path (that double-freed again).

`schedule::pick` must `mark_wait` after a missed `try_execute` — leaving `ST_RUNNING` blocked `force_idle_recover` (N=3 `gated=false n_unf=27`).

Reuse Detect leftover vs `add_dependency` cycle: later leftover Detect-gated on us, we parked Aborting on them. Flush the leftover pred only.

### 1b. 19469101 120s timeout

Thin FullReplay Opt ping-pong (`hops=0` skipped plant). `plant_observed_window` width-1 + `break_replay_mill` + `plant_invalid_locs` on every non-lazy invalid ℓ. Never `Q_ordered` from FullReplay.

**Status:** Instant-off Soft=0 N=3 green at `cc762ed` / `8a5c16f` (~19–30 ms, `occ_picks=0`). Re-confirm after leftover-width.

### 1c. 19807137 first-SF abort / hang

Several distinct bugs, all verified on hang-trace:

| Symptom | Cause |
|---------|--------|
| `glob_min` sticky-done, ~24-head mill | Commit then FullReplay left `done_bits` set; leftover_min plants no-op'd. **Fix:** `abort_and_estimate` → `note_abort_reincarnate` bits-only. |
| `leftover_min=405 n_run=1 n_unf=307` | Width-1 `plant_global` starred every leftover on leftover_min. 405 `may_execute` but `add_dependency`-parked on a later waiter that Detect-waited on 405. |
| `leftover_w=8 n_unf=190` (width-8) | Surplus Detect-chained on leftover_min/chain. leftover_n stayed 8 because FullReplay never freed slots. **Fix:** `clear_leftover_slot` on abort; surplus wait on a live tip `< consumer`, not leftover_min. |
| Recover both sides of earlier←later | incarnation++ mill, `n_unf=437`. **Reverted.** Flush Detect + `detach_dependent` only. |
| Drop product `plant_global` | 19807137 still hung (`leftover_w=0 n_unf=505` per-ℓ leftover). **6196166 SF[0]/[1] then heap-abort 134.** Restored `plant_global`. |

One width-8 run printed `specfence[0] ok wall_ms=61` then SEGV (139) during teardown — heap still dirty on that schedule.

**Still open:** Instant-off Soft=0 N≥3 for 19807137. Do not paper with OCC pick.

### 1d. `complete_arch_edge_pi_seq_eq_par_softwait0`

Green at `cc762ed` (0.02s Soft=0 seq≡par). Re-confirm after leftover-width. Block-wide leftover drain-rebind DashMap-rebound first-wave and heap-aborted this test; drain rebind stays off.

---

## 2. Learn B1–B7 (must not regress)

Wires unchanged this land: Resolve→observe, IntraPatch this-ℓ only + PC veto, Prior before `admit_seed`, reuse `explore=0`, E6 lazy-only demote.

---

## 3. Instant-off Soft=0 @8 (this land)

| Block | N | OCC med | SF cold | SF reuse | Soft | occ_picks (SF) | Status |
|-------|--:|--------:|--------:|---------:|-----:|----------------|--------|
| **6196166** | 3 | 1.892 ms | 8.133 | 10.066 | 0 | **0** | green at `8a5c16f`; heap-abort if `plant_global` dropped |
| **19469101** | 3 | ~10 ms | 27.4 | 19.3 | 0 | **0** | green at `cc762ed` — re-confirm |
| **19807137** | 1 | 2.28 s | — | — | 0 | — | **still hanging** (see 1c) |
| 3356896 / 14396881 / 14689597 | | | | | 0 | 0 | last land green — re-confirm |

---

## 4. Source asserts

Still ban `next_task_with_wave_ready(`, `.next_task(`, `validate_occ_stage(` on the SF path. `apply` keeps `plant_observed_window` + `break_replay_mill`.
