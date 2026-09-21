# SpecFence true spine — remaining hang land result

**Date:** 2026-09-21  
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`  
**SoT:** `specfence-sf-ps-remaining-hang-land-v1.md`  
**This land does not restore** `Scheduler::next_task*` / `next_task_with_wave_ready` / `validate_occ_stage` on SpecFence.

Product leftover/worker/resolve after this note: **`8a5c16f` baseline** (width-1 `plant_global`, abort done-stamp clear). Width-8 leftover Detect experiments are **not landed** — they hung 19807137 differently and heap-aborted 14396881 / complete_arch.

Raw Instant-off (gitignored): `lab/results/sf-ps-remain-*.json`.

---

## 1. Verified root causes (not papered with OCC pick)

### 1a. 6196166 heap-abort after `plant_observed_waw`

DashMap is not reentrant. `note_producer_done` holding `consumers.get` across `may_execute` corrupted the heap (`free(): invalid pointer` / `double free`).

**Fix (kept on `8a5c16f`):** drop the `consumers` shard before `may_execute`. Never walk `waiters`/`consumers` from `note_abort_reincarnate`. Never `ungate` + `recover_executing` on the Blocked path.

`schedule::pick` must `mark_wait` after a missed `try_execute` (N=3 `ST_RUNNING` leak, `n_unf=27`).

Reuse Detect leftover vs `add_dependency` cycle: flush the leftover pred only.

**Instant-off Soft=0 N=3:** green at `8a5c16f` (8.1 / 8.6 / 10.1 ms, `occ_picks=0`, `began_from_prior`). Re-confirmed at `3d88e00` with width-8 still in tree (9.1 / 10.2 / 5.2 ms). Dropping product `plant_global` heap-aborted this block after SF[0]/[1].

### 1b. 19469101 120s timeout

Thin FullReplay Opt ping-pong. `plant_observed_window` width-1 + `break_replay_mill` + `plant_invalid_locs`. Never `Q_ordered` from FullReplay.

**Instant-off Soft=0 N=3:** green this land at `3d88e00` — OCC med 8.649 ms, SF cold 13.789 / reuse 16.501, `occ_picks=0`, explore=0, `began_from_prior`.

### 1c. 19807137 first-SF abort / hang — still open

Hang-trace verified:

| Symptom | Cause | Experiment |
|---------|--------|------------|
| `glob_min` sticky-done, ~24-head mill | Commit then FullReplay left `done_bits` | **Kept:** `abort_and_estimate` → `note_abort_reincarnate` bits-only |
| `leftover_min=405 n_run=1 n_unf=307` | Width-1 `plant_global` stars leftovers on leftover_min; 405 `add_dependency`-parks on a later waiter | Width-8 leftover tips: surplus Detect-chain hung `leftover_w=8 n_unf=190/544`. One run printed SF ok 61 ms then SEGV 139 |
| Recover both sides of earlier←later | incarnation++ mill `n_unf=437` | **Not landed** |
| Drop product `plant_global` | still hung `n_unf=505` per-ℓ leftover; **6196166 heap-abort** | Restored `plant_global` |
| Overflow no Detect (claim-only) | hung `leftover_w=8 n_unf=441`; **14396881 SF[2] `free(): invalid size`**; 14689597 first-SF timeout | **Reverted to `8a5c16f` leftover** |

Do not paper with OCC pick. Next: leftover_min must commit without starring surplus (same-ℓ `plant_observed_window` only is not enough; block-wide Detect is too much).

### 1d. `complete_arch_edge_pi_seq_eq_par_softwait0`

Green at `cc762ed` (0.02s Soft=0 seq≡par). Width-8 leftover heap-aborted (`munmap_chunk` / `free(): invalid pointer`). Also aborted when leftover/worker/resolve were restored to `cc762ed` on this tip — not leftover-width-8 alone. Re-confirm on `8a5c16f` leftover restore.

Drain-rebind of first-wave onto leftover_min stays **off**.

---

## 2. Learn B1–B7 (must not regress)

Wires unchanged: Resolve→observe, IntraPatch this-ℓ only + PC veto, Prior before `admit_seed`, reuse `explore=0`, E6 lazy-only demote.

3356896 N=3 this land (width-8 still in tree): `occ_picks=0`, explore=0, e6=1, `began_from_prior`.

---

## 3. Instant-off Soft=0 @8

| Block | N | OCC med | SF cold | SF reuse | Soft | occ_picks | Status |
|-------|--:|--------:|--------:|---------:|-----:|----------:|--------|
| **6196166** | 3 | 1.772–1.892 | 8.1–9.1 | 10.1 | 0 | **0** | green |
| **19469101** | 3 | 8.649 | 13.789 | 16.501 | 0 | **0** | green |
| **3356896** | 3 | 1.505 | 2.484 | 3.355 | 0 | **0** | green (width-8 tree) |
| **19807137** | ≥3 | ~2.3 s | — | — | 0 | — | **open** |
| **14396881** | 3 | — | — | — | 0 | — | SF[0]/[1] ok then heap on width-8; re-confirm on `8a5c16f` leftover |
| **14689597** | 3 | — | — | — | 0 | — | first-SF timeout on width-8; re-confirm on `8a5c16f` leftover |
| complete_arch | Soft=0 | — | — | — | 0 | — | heap on width-8; re-confirm |

---

## 4. Source asserts

Still ban `next_task_with_wave_ready(`, `.next_task(`, `validate_occ_stage(` on the SF path. `apply` keeps `plant_observed_window` + `break_replay_mill`.
