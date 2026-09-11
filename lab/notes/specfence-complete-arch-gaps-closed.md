# SpecFence complete-arch gaps closed

**Date:** 2026-09-10  
**PR:** https://github.com/fengjy73/pevm/pull/3  
**SoT:** `lab/notes/specfence-complete-cc-architecture.md`  
**Audit file** `specfence-complete-arch-impl-gap-audit.md` @ `94b2d55` was **not on this checkout**; gaps below are the user must-close list + `specfence-complete-arch-impl.md` PARTIAL/MISSING.

**JSON:** `lab/results/gaps-closed-xblock-{sf-occ,flip,xblock}.json`

---

## Verdict

**All eight contracts are LANDED in code.** SoftWait Soft = 0. Hang-free after dropping the Ready→Spec leak.

**Makespan did not move toward OCC.** 597 SF wall median **17.7 ms** vs OCC **6.2 ms** (SF/OCC TPS **0.338**). Mean SF/OCC **0.426**. WaitFor on 597 warm is **21** vs Spec **4117** — Bind (841) takes published Data; remaining Specs are first-wave / independence, not the old Ready leak. Clique Spec count did **not** collapse. Warm 597 **42.6 ms** worse than cold **18.7**. Do not celebrate abort (SF 50 vs OCC 56).

Recommend: **human confirm**. Fidelity is closed; wall is not.

---

## Before → after (every PARTIAL/MISSING)

| Item | Before (complete-arch `b7c7353`) | After | Class |
|------|----------------------------------|-------|-------|
| **1. WaitFor leak (D6+A1)** | `choose_edge_action` / `maybe_wait` Spec'd when `!is_executing` (`1283b1c`) | WaitFor if essential ∧ `w < reader`. Hang-freedom = `Scheduler::admit_spine` + steal, not Spec | **LANDED** |
| **2. Ordered chain templates (A1)** | `ChainTemplate` fanout/conf only | `push_spine` / `next_writer_before` sorted writer indices; `predicted_chain_len` on inter prior | **LANDED** |
| **3. Progressive wake on Data (A2)** | finish_execution dependents only; SoftWait wake unused | `wake_on_data_publish` → `wave.wake_location` + `FenceGraph::wake_on_data` (hard waits, Soft=0). Ready stays in finish (597 double-free if early Ready) | **LANDED** |
| **4. Immediate Avoid first-wave** | Avoid at Data finish only | Also on ESTIMATE (`convert_writes_to_estimates`); in-batch flag for later `maybe_wait` | **LANDED** |
| **5. Early-visible + piece abort (A3)** | Bind-on-Data; tip abort still suffix/FR | Bind kept; `EdgeTable::min_k_of_invalid` seeds R2 `k_fail`; unvalidated tip → R1/R2 not a Bind door | **LANDED** |
| **6. EdgeKey Detect→Resolve (A5)** | Table recorded; validate ignored `k` | `min_k_of_invalid` / `later_touches`; R2 suffix from EdgeKey `k`; R4 still failure (not removed) | **LANDED** |
| **7. Contention-split (A4)** | Independence = !H && !Avoid && !template key | `in_serial_lane` = H ∪ Avoid ∪ `template_live`; only that set is serialized; indep Specs | **LANDED** |
| **8. Inter warm + live decay (A6)** | Seed H; decay multiplied conf but left H | `decay_warm_failures` drops H+template when `conf < 0.20`; `template_live` gates Wait/H | **LANDED** |

---

## File:fn (new / changed)

| Contract | Symbol |
|----------|--------|
| D6 WaitFor | `choose_edge_action` — no `writer_admitted` gate |
| D6 admit | `Scheduler::admit_spine` |
| A1 spine | `HotSketch::{push_spine,next_writer_before}` |
| A2 Avoid | `pevm.rs` ESTIMATE loop; `vm.rs` finish `broadcast_avoid` |
| A2 wake | `Vm::wake_on_data_publish`, `FenceGraph::arm_hard_wait` / `wake_on_data` |
| A4 lane | `HotSketch::in_serial_lane` |
| A5 k | `EdgeTable::min_k_of_invalid` → `pevm.rs` validate `k_fail` |
| A6 | `HotSketch::decay_warm_failures` + `TopLocPrior.chain_len_ema` |

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo test -p pevm --lib --release` | green (incl. `wait_for_ready_known_essential`, `ordered_spine_next_writer`, `hard_wait_wakes_on_data_not_soft`, `decay_drops_live_template`) |
| `cargo test -p pevm --test specfence --release -- --test-threads=1` | **39 passed**, 20 ignored |
| `gaps_closed_waitfor_avoid_publish_wake` | seq≡par, Soft=0, Avoid/Bind, warm WaitFor∨Bind |

---

## Metrics vs complete-arch / OCC (N=3 @8)

| Block | SF wall med | OCC wall med | SF/OCC TPS | SF abort med | OCC abort med | Soft |
|------:|------------:|-------------:|-----------:|-------------:|--------------:|-----:|
| **14689597** | **17.7** | **6.2** | **0.338** | **50** | **56** | 0 |
| **19606599** | **29.2** | **13.0** | **0.446** | **126** | **99** | 0 |
| **19469097** | **18.0** | **8.7** | **0.484** | **148** | **125** | 0 |
| **19606598** | **3.3** | **1.3** | **0.436** | **9** | **4** | 0 |

Mean SF/OCC = **0.426** (complete-arch was 0.375 on a faster OCC host). 597 wall **worse** than complete-arch 14.5 (more WaitFor/admit). Soft=0. Await@a=0.

### 597 π (xblock warm vs complete-arch warm)

| | complete-arch | gaps-closed |
|--|-------------:|------------:|
| edge_bind | 824 | **841** |
| edge_wait_for | 11 | **21** |
| edge_spec | 4016 | **4117** |
| avoid | 917 | **916** |
| SF-warm wall | 16.9 | **42.6** |
| SF-cold wall | 15.1 | **18.7** |
| OCC wall | 6.1 | **5.0** |

**Contract still failing in *metrics* (not in π):** clique Spec count. After Avoid, readers **Bind** published Data; Specs are pre-writer / independence. WaitFor cannot eat 4k Specs without inventing unpublished writers. Wall toward OCC: **fails** (597 ~2.9×).

Warm ≥ cold: **still fails** on storm cores (A6 decay is live; not a makespan win).

---

## Residual (not a missing verb)

1. **R2 abs JUMP under concurrency** — still production-OFF (Iter21+ hang). ResumePath `aj` remains. Not a new door.
2. **R4 FullRestart** still occurs (failure mode, not π).
3. **AEC/morph** still compiled, unused as wait π.
4. Progressive Ready-before-`is_done` was **tried and ripped out** (597 `double free`). Wake is notification; dependents drain at finish.

---

## Hard bans

SoftWait Soft=0. No EV Await door. No tip-identity Bind gate. No OCC-retry as π. No Storm morph as π. No new θ.
