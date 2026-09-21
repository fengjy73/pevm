# SpecFence true spine — land result

**Date:** 2026-09-21  
**PR:** [#45](https://github.com/fengjy73/pevm/pull/45) `cursor/specfence-sf-ps-true-spine-d6e8`  
**Base:** PR #44 `cursor/specfence-sf-ps-full-land-09b0` (user-judged shallow)  
**PR #43 baseline (TPS only):** `cursor/specfence-shell-cut-redig-6a8f`  
**SoT:**
- `specfence-sf-ps-true-spine-learn-pc-detailed-v1.md` (main)
- `specfence-sf-ps-pr44-shallow-gap-and-true-spine-v1.md`
- `specfence-sf-ps-true-spine-learn-pc-land-v1.md`
- `specfence-first-class-architecture-redesign-v1.md`

Raw JSON (gitignored): `lab/results/sf-ps-true-spine-instant-*.json`, `lab/results/sf-ps-true-spine-focus-n3.json`.

This land **does not** restore `Scheduler::next_task*` on the SpecFence path to chase TPS.

---

## Call graph (evidence, not a rename)

SpecFence worker (`pevm.rs` `ConcurrencyMode::SpecFence`):

```
run_sf_block
  → schedule::pick
       RunnableSet.pick(Q_indep | Q_released | Q_ordered | Q_revalidate)
       + steal (opposite-end) + refuse_fill independent
       IntraPatch at pick boundary (≤1 promote / ℓ / block; PC veto)
  → Execute(VisibilityPolicy::{Opt, WaitReleased, OrderedTip})
       edged Estimate tip ≡ SfMvMemory.read(WaitReleased|OrderedTip)
       (skip Estimate; no nested DashMap get — that deadlocks)
  → validate_to_plan → ResolvePlan
  → resolve_plan::apply
       finish_validation_sf + release_successors + enqueue_higher_revalidate
       (edged path does not call validate_occ_kernel)
```

OCC contrast only:

```
next_occ_task → Scheduler::next_task
validate_occ_stage
```

Scheduler is an incarnation/status ledger (`try_execute_producer`, `finish_execution`, `finish_validation_sf`, `prepare_revalidate`). It is **not** the pick host.

Proof:

| Check | Result |
|-------|--------|
| `specfence_true_spine_sources_never_call_next_task` | pass (schedule / runnable_set / worker / resolve_plan / sf_mv / arm_table / pevm.rs) |
| `specfence_sf_ps_pick_never_calls_next_occ_task` | pass |
| unit source asserts (`next_task_with_wave_ready(`, `.next_task(`, `validate_occ_stage(`) | pass |
| Instant-off SF rows `occ_schedule_picks` | **0** (3356896 N=3, 14396881 N=3) |
| Instant-off OCC rows `occ_schedule_picks` | >0 (contrast live) |
| `resolve_apply_n` last SF 3356896 | 265–340 (apply mutates certs/queues, not a counter-only plan) |
| `sf_schedule_picks` last SF 3356896 | 340–451 |

PR #44 anti-pattern (`schedule::pick` → `next_task_with_wave_ready`) is gone.

---

## What is first-class (vs PR #44)

| Face | PR #44 (shallow) | This land |
|------|------------------|-----------|
| Pick | wave-ready host walk | Real `Q_indep` / `Q_released` / `Q_ordered` + `Q_revalidate`; steal; refuse = fill independent |
| Memory | `VisibilityPolicy` enum, OCC default read | Edged vis skips Estimate tip (`WaitReleased` / `OrderedTip`). `SfMvMemory::read` unit-tested; product walk must not re-enter DashMap |
| Resolve | `validate_specfence` often `validate_occ_kernel` | `validate_to_plan` → `apply` (`Commit` / Rebind / Rewind / OrderedReplay / FullReplay) |
| Worker | Block-STM loop with SF entry | `run_sf_block` RunnableSet ring |
| Learn | mostly metrics | Intra E1–E6 + IntraPatch (thin `w_max`: n≤176 ⇒ w≤2). Inter `begin_from_prior` / `end_pack`; reuse `explore_n=0` |

---

## Hard constraints

| Constraint | Result |
|------------|--------|
| Soft=0 | Instant-off 3356896 / 14396881 every SF row `soft_wait_arms=0` |
| seq≡par | `cargo test -p pevm --lib --release`: **374 passed**. iter11 / independent / same_sender / source asserts: pass |
| lazy never OrderedAdmit | Detect same-to / long same-to stay lazy; commute refuses shared absolute Basic |
| OCC contrast kept | `ConcurrencyMode::Occ` still uses `next_occ_task` |
| no SoftWait | Soft arm count 0 |
| thin w_max | 3356896 `w_cap=2`; reuse climbed Win_1→Win_2, never Win_8 |
| under-covered ≠ Full-as-success | `ArmTable::end_pack` forces Opt when `under_covered && Full` |
| hot sticky reuse explore_n=0 | Instant-off reuse rows `explore_n=0` |

---

## Regressions

| Suite | Result |
|-------|--------|
| `cargo test -p pevm --lib --release -- --test-threads=1` | **374 passed** |
| `specfence_independent_raw_transfers` | pass |
| `specfence_iter11_handler_multi_sstore_jump_seq_eq_par` | pass (isolated 0.04s) |
| `specfence_same_sender` | pass |
| `specfence_true_spine_sources_never_call_next_task` | pass |
| `specfence_sf_ps_pick_never_calls_next_occ_task` | pass |
| `complete_arch_edge_pi_seq_eq_par_softwait0` | **timeout 90s** (erc20 cluster; not treated as green) |

---

## Instant-off Soft=0 @8 (compare harness)

Host is faster/noisier than the PR #43 Instant-off box. Compare **ratios, arms, and call-graph counters**, not absolute ms. OCC on this host is ~1 ms on 3356896 (PR #44 Instant OCC was ~7 ms).

### 3356896 (n=176) — N=3 complete

| | OCC med | SF cold | SF reuse | reuse/OCC |
|--|--------:|--------:|---------:|----------:|
| this land | 0.996 ms | 2.827 ms | **2.435 ms** | **2.45×** (SF slower) |
| PR #44 Instant N=5 | 7.141 ms | 8.254 ms | 4.004 ms | 0.56× |
| PR #43 Instant | — | — | — | 1.13×, Win_8 |

Last reuse row: Soft=0, `occ_schedule_picks=0`, `sf_picks=340`, `resolve_apply_n=265`, `vis_opt/wait/tip=302/0/3`, Resolve Commit=220 FullReplay=45, `w_cap=2`, `explore_n=0`, `steal_n` on earlier iters **1 / 2**, `refuse_fill_n` **6 / 2**.

N=5 hung on `specfence[3]` after three good SF iters — reuse iter-3 flake / leftover; N=3 is the honest complete measurement.

### 14396881 (n=1346) — N=3 complete

| | OCC med | SF cold | SF reuse | reuse/OCC |
|--|--------:|--------:|---------:|----------:|
| this land | 4.483 ms | 6.327 ms | **5.750 ms** | **1.28×** |
| PR #44 Instant | 15.714 ms | 18.408 ms | 16.493 ms | 1.05× |

Soft=0, `occ_picks=0`, `resolve_apply_n≈1366`, Opt-only vis, Avoid=noop independents, `explore_n=0`. `steal_n=0` (shared-deque `pop_back` never needed `pop_front` on this antichain).

### 6196166 / 19807137 — first SF did not finish

OCC[0] printed (6196166 ~2.4 ms; 19807137 ~2.3 s). First SpecFence iter exceeded 90 s. **Not scored.** Not “fixed” by putting SF back on Block-STM pick.

---

## idle / steal / refuse_fill

Shared `RunnableSet` deques: local pop is `pop_back`, steal is `pop_front`.

| block | steal_n (SF iters) | refuse_fill_n | idle_core_frac | note |
|-------|-------------------|---------------|----------------|------|
| 3356896 N=3 | 1, 2, 0 | 6, 2, 0 | 0 | `idle_ns=0`, `busy_ns` 5–13 ms — no idle |
| 14396881 N=3 | 0, 0, … | 0 | 0 | width ~630; local pops emptied the antichain |

`idle_core_frac = idle_ns / (idle_ns + busy_ns)`. Zero idle on these short blocks is expected, not a missing counter (`worker_busy_ns` is attributed in `run_sf_block`).

---

## Focus N=3 reuse @8

Requested six: 3356896, 19807137, 14396881, 6196166, 19469101, 14689597.

Sweep (`specfence_all_blocks_sweep`, reuse=1, @8) completed **2/4** started blocks then hung on 19469101 SF (300 s). 14689597 Instant-off first SF also exceeded 90 s. 6196166 / 19807137 were not started in this sweep.

JSON: `lab/results/sf-ps-true-spine-focus-n3.json` (status=running, 4 rows).

| block | n | OCC reuse med ms | SF reuse med ms | SF/OCC | Soft | occ_picks | note |
|------:|--:|-----------------:|----------------:|-------:|-----:|----------:|------|
| **3356896** | 176 | 2.309 | **2.246** | **0.97** | 0 | 0 | Win_1→Opt; OA=11 |
| **14396881** | 1346 | 4.669 | **5.507** | **1.18** | 0 | 0 | Full→Full report is in-block FullReplay, not Full-as-success pack |
| 19469101 | 469 | 8.5 (OCC) | — | — | — | — | SF hung after OCC |
| 14689597 | 564 | — | — | — | — | — | Instant-off SF[0] >90 s |
| 6196166 | 108 | — | — | — | — | — | Instant-off SF[0] >90 s |
| 19807137 | 712 | — | — | — | — | — | Instant-off SF[0] >90 s |

Focus-2 median SF/OCC **1.08**. Same two on PR #44 Instant: 0.56 and 1.05. Same two on PR #43 N=3: 0.691 and 0.893.

Honest vs PR #43 / #44: this host’s OCC Instant is much hotter (~1 ms on 3356896 vs PR #44’s 7 ms). **Sweep reuse 0.97 on 3356896 is the fair same-harness number.** Instant-off 2.45× is OCC-hot, not a Block-STM retreat (`occ_picks=0`, Resolve apply, Win_1/2).

---

## Remaining

- **6196166 / 19807137** first SpecFence iter hang (not OCC). Diagnose on the RunnableSet ring; do not call `next_task*`.
- **3356896 N=5** hung once on SF iter 3 after three good iters.
- **complete_arch / erc20 cluster** 90 s timeout.
- Full 99-block N=3 reuse not run.
- `SfMvMemory::read` is the unit-tested API; the VM walk is the reentrancy-safe equivalent (DashMap `get` is not nested).
- PartialAbort Rebind/Rewind histogram is still thin; edged path is Commit / OrderedReplay / FullReplay first.

TPS was **not** recovered by restoring Block-STM as the SpecFence pick root.
