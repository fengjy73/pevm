# SpecFence true spine — hang fix + Learn wiring land result

**Date:** 2026-09-21  
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`  
**SoT:** `specfence-sf-ps-hang-learn-wiring-land-v1.md` + `specfence-sf-ps-true-spine-learn-pc-detailed-v1.md`  
**This land does not restore** `Scheduler::next_task*` / `next_task_with_wave_ready` / `validate_occ_stage` on SpecFence.

Raw Instant-off (gitignored): `lab/results/sf-ps-hang-learn-*.json`.

---

## 1. Hang root cause (verified, not the brief’s only hypothesis)

The brief’s `|Runnable|=0` leftover-gate story is **partly true** and was fixed. The hang-class first-SF that sat at **~387% CPU** was a **different** bug.

### 1a. Leftover Detect gates (PC-5)

`is_gated` with no consumer map (or a dead producer) made `may_execute=false` forever. IntraPatch used **all** `blocked_consumers()`, so a promote of ℓ moved strangers off `Q_indep` with no resume.

**Fix:** `may_execute` leftover `None => true`; idle `collapse_false_gates`; IntraPatch = `consumers_queued_on(ℓ)` only; PC veto if width would collapse below cores.

### 1b. Thin FullReplay Opt ping-pong (the 387% livelock)

6196166 first SF after OCC[0]: workers busy, not idle. OCC finishes in ~2.5 ms with ~90 aborts. SpecFence `FullReplay` on EffectiveWAW only planted a Detect edge when `hops_to_admit>0`. Thin majority → hops=0 → two Opt writers rewrite the same ℓ forever (Commit of i invalidates j, j FullReplay invalidates i).

**Fix:** `plant_observed_waw` — on non-lazy EffectiveWAW FullReplay/OrderedReplay, always `note_consumer_on(succ, pred, ℓ)` and `mark_wait` if the producer is still live. Waiter set is deduped.

### 1c. Reuse Prior over-planting

`install_prior_into_policy` must seed CostPolicy **before** `admit_seed` (B4). Remembering Win/Full on a 50-writer thin loc at begin recreated a wait-set that hung or corrupted the heap (`free(): invalid pointer` / `corrupted size vs. prev_size`).

**Fix:** remember only; never `promote_short_edge` from Prior; if `n_pairs > w_max` force Opt at begin (mid-block `plant_observed_waw` still raises real edges).

### Still failing (honest)

| Block | Status after this land |
|-------|------------------------|
| **14689597** | **cleared** Instant-off Soft=0 N=3 |
| **3356896 N=5** | **cleared** (previously stuck after good iters) |
| **14396881** | **cleared** N=3 |
| 6196166 | first SF still **heap-abort** (`free(): invalid pointer`) on some runs; other runs finished SF[0]/SF[1] then crashed/hung |
| 19807137 | first SF **abort** after OCC ~2.4 s |
| 19469101 | first SF **timeout 120 s** after OCC |
| `complete_arch_edge_pi_seq_eq_par_softwait0` | still **>180 s** (not green; Soft=0 seq≡par not re-proven on this cluster) |

Do **not** paper these with OCC pick.

---

## 2. Learn wiring call-graph (before → after)

Learn’s only legal outputs remain **G / ArmTable / release / explore quota**.

| Wire | Before (PR45 land) | After |
|------|--------------------|-------|
| **B1** | `apply` called `observe`, but Commit with empty invalid skipped E1; E4 missing; counters not exported | `observe` on every `apply`; Commit without loc still E1; FullReplay E2; Partial E3; `note_e4` on refuse_fill; E5 under-cover; E6 lazy. Exported on Instant-off rows |
| **B2** | IntraPatch at pick; moved **all** blocked consumers | IntraPatch at **pick and `release_successors`**; `consumers_queued_on(ℓ)` only; ≤1 / ℓ / block; PC veto counted |
| **B3** | Commit `force_push` Q_* | same + IntraPatch + E1 |
| **B4** | `begin_from_prior` filled ArmTable only; `admit_seed` read CostPolicy | `install_prior_into_policy` **before** `admit_seed`; long thin spines stay Opt |
| **B5** | `end_pack` thin / under-covered Opt | unchanged + tests |
| **B6** | sticky `explore_budget=0` | reuse Instant-off `explore_n=0` (14689597 cold explore=2 → reuse 0) |
| **B7** | `force_opt` on lazy, no G change | `demote_lazy_graph` ungate → `Q_indep`; 3356896 `learn_e6_n=1` |

Grep proof: `specfence_learn_wiring_is_first_class` + unit tests (`install_prior_seeds_policy_block_arm`, `intra_patch_moves_only_this_location`, `pc_veto_when_width_would_collapse`).

Source asserts still ban `next_task_with_wave_ready(`, `.next_task(`, `validate_occ_stage(` on the SF path.

---

## 3. Instant-off Soft=0 @8

Host OCC is hotter than the PR43/44 Instant box. Compare ratios and counters, not absolute ms.

| Block | N | OCC med | SF cold | SF reuse | reuse/OCC | Soft | occ_picks (SF) |
|-------|--:|--------:|--------:|---------:|----------:|-----:|----------------|
| **3356896** | 5 | 0.931 ms | 2.092 | **1.760** | **1.89×** | 0 | **0** |
| **14396881** | 3 | 4.417 ms | 6.347 | **5.871** | **1.33×** | 0 | **0** |
| **14689597** (hang class) | 3 | 6.240 ms | 32.732 | **41.500** | **6.65×** | 0 | **0** |
| 6196166 | 3 | — | — | — | — | — | first SF abort |
| 19807137 | 3 | OCC[0] 2.37 s | — | — | — | — | first SF abort |
| 19469101 | 3 | OCC[0] 10.4 ms | — | — | — | — | first SF timeout |

### Learn / PC counters (last SF row unless noted)

| Block | e1 | e2 | e4 | e5 | e6 | mid / veto | prior_plant | began_from_prior | steal | refuse_fill | resolve_apply | explore |
|-------|---:|---:|---:|---:|---:|-----------:|------------:|:-----------------|------:|------------:|--------------:|--------:|
| 3356896 N=5 last | 0* | 45 | 0 | 45 | **1** | 0 / 0 | 0 | **true** | 0 | 0 | 258 | **0** |
| 3356896 reuse i=1 | 0* | 46 | 0 | 46 | 1 | 0 / 0 | 0 | **true** | 0 | 0 | 270 | 0 |
| 14396881 last | 0* | 8 | 0 | 8 | 0 | 0 / 0 | 0 | **true** | **1** | 0 | 1365 | **0** |
| 14689597 cold | 0* | 488 | 22 | 481 | 0 | 0 / 0 | 0 | false | 0 | 22 | 1518 | 2 |
| 14689597 reuse i=1 | 0* | 492 | 10 | 488 | 0 | 0 / 0 | **1** | **true** | 2 | 10 | 1539 | **0** |
| 14689597 reuse i=2 | 0* | 974 | 0 | 973 | 0 | 0 / 0 | **2** | **true** | 0 | 0 | 2486 | **0** |

\* E1 was 0 because Commit’s invalid set is empty so `observe` had no ℓ. Fixed this revision: Commit without loc still increments E1 (not yet re-measured).

B4 reuse evidence: 14689597 `prior_plant` 0 → 1 → 2 and `began_from_prior=true` on wave-1 of reuse (not end-block-only). 3356896 / 14396881 reuse `began_from_prior=true` with `explore_n=0`.

No SoftWait on any measured SF row.

---

## 4. cargo test

| Suite | Result |
|-------|--------|
| `cargo test -p pevm --lib --release -- --test-threads=1` | **380 passed** (was 374) |
| `specfence_learn_wiring_is_first_class` | added (integration; compile-gated) |
| `complete_arch_edge_pi_seq_eq_par_softwait0` | **still timeout >180 s** |

---

## 5. idle / steal / refuse — honest

`idle_core_frac` stays 0 on these short Instant-off rows (`idle_ns=0`, `busy_ns` attributed in `run_sf_block`). That is “no idle sample”, not proof of full-core occupancy on 6196166-class.

Steal is `pop_front` on the shared deques. 14396881 last steal=1; 14689597 reuse steal=2. Independent antichains often empty via local `pop_back` (steal=0).

---

## 6. Remaining

- Heap corruption / first-SF abort on 6196166 and 19807137 — diagnose on the SF ring (DashMap / waiter Vec), **not** `next_task*`.
- 19469101 first-SF timeout.
- complete_arch erc20 cluster still too slow.
- Re-measure Instant-off after the Commit-E1 counter fix.
- Full 99-block N=3 not run.

TPS was **not** recovered by restoring Block-STM as the SpecFence pick root.
