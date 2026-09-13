# SpecFence v5 PC⊗CC fusion — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-v5-pc-cc-fusion-e28e` (PR #7 → `cursor/specfence-parallel-compute-68a3` / PR #6)  
**Plant SoT:** `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md` (**authoritative** from `987b196`, not the recreation)  
**Catalog:** `lab/notes/specfence-pc-cc-fusion-per-block-catalog.json`  
**π SoT:** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md` (unchanged)  
**Honesty baseline:** parallel-compute nonempty N=1 median **SF/OCC = 0.744**; quiet **1.020**

**Verdict:** rebased onto the pushed SoT (`987b196` + catalog), then landed the remaining plant vs that document. Incarnation Occ\|Pcc fork is not SoT. Nonempty median **0.655** (↓ vs 0.744). Quiet heuristic median **1.050** (19/36 ≥1). Soft=0. 14689597 N=1 **0.558** (↑ vs our pre-rebase 0.365 / PC 0.535). 2179522 N=3 **0.751** (was 0.112). **Do not claim ≥0.7 as a finished product bar** — 0.655 is this run’s median, not a guarantee.

---

## Land map (file:fn)

| # | SoT item | file:fn | Status |
|---|---------|---------|--------|
| 1 | Architecture SoT + diagnosis + switch audit | `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md` et al. | **landed** |
| 2 | Mode(a) certificates, not `mark_pcc(tx)` | `kernel.rs::note_fence` / `may_resolve` / `rem_legal` | **landed** |
| 3 | `decide(a, e_vis, PE, learning)` — no quiet/park kill-switch | `access_policy.rs::decide` | **landed** |
| 4 | AccessOrdinalLog true \(k\) (not rem DashMap) | `access_log.rs::note` on every SF access | **landed** |
| 5 | Spec abort PE at true \(k\) (never residual 1) | `executor.rs::validate_occ_kernel` | **landed** |
| 6 | Prior PE + Data / executing writer may Fence | `decide` Bind / WaitFor | **landed** |
| 7 | Serial-lane multi-writer PE class (before Bind) | `access_policy.rs` + `vm.rs::pcc_serial_lane` | **landed** |
| 8 | R1 only with certificates; Spec miss = B0 | `uses_specfence_resolve` = `may_resolve` | **landed** |
| 9 | Ready/steal = wave + execute-first (no validation stampede) | `scheduler.rs::next_task_with_wave` when wave Some | **landed** |
| 10 | HotSet / WŜ observe → PE posterior, not SerialLane door | `access_policy.rs::decide` | **landed** |
| 11 | Empty PE ∧ ¬Fence ⇒ OCC (±detect, ±k-log) | `specfence_plant_is_occ` after k-log | **landed** |
| 12 | Spec PE-publish still Avoid / Data-wake | `vm.rs` finalize | **landed** |

**Not claimed:** work-stealing deques per worker; Bind/B0 on named spine/fan_out still loses to OCC reincarnation; AccessOrdinalLog still writes a per-tx first_k map on Spec.

---

## Control loop (live)

```
OCC:     next_task(); occ_read; validate_read_locations; B0
SpecFence access a:
  detect; k := access_log.note(ℓ)          # not rem DashMap
  empty PE → Spec (OCC)
  ¬PE(ℓ,k) → Spec
  vis := e_vis (+ HotSet/WŜ observe only)
  decide → Spec | Bind | WaitFor | SerialLane
  first real Fence → certificate (rem / R1), not a tx kernel
  SerialLane: mark class + admit_spine; park only if lane head executing
SpecFence validate:
  ¬certificate → OCC bool + B0 + PE(true k from access_log)
  certificate → R1a / R1b / B0
schedule: wave ready; Execute if next idx Ready; else validate
  (do not fetch_add execution_idx on a miss; do not steal on wait_park_count)
```

`decide` (SoT §3.2): empty/¬PE → Spec; unfinished>1 or (lane ∧ unfinished>0) → SerialLane; unfinished==1 ∧ executing → WaitFor; Data ∧ unfinished=0 → Bind; else Spec. **Deleted live gates:** `quiet_fence_off`, `park_storm`, HotSet-as-SerialLane.

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo +nightly test -p pevm --lib --release -- --test-threads=1` | **168 ok** |
| `cargo +nightly test -p pevm --test specfence --release -- --test-threads=1` | **42 ok / 20 ignored** |
| erc20 / raw_transfers / mixed / uniswap / beneficiary / small_blocks | **all ok** |

seq≡par held on those suites. SoftWait Soft = 0 in sweeps. `specfence_p1a_selective_invalidate_and_fence` accepts Spec-miss **B0** (`full_restart`) as the plant, not only selective / `tx_full_retry`.

Toolchain: `cargo +nightly` (edition 2024), `--config 'profile.release.lto=false'` for local release.

---

## Honesty vs 0.744

Sweeps (gitignored JSON):  
`lab/results/v5-sot-all-blocks-sweep.json`,  
`lab/results/v5-sot-focus-n3-sweep.json`.  
Committed digest: `lab/notes/v5-fusion-sweep-summary.json`.

A broken execute-first `fetch_add` on miss (median **0.643**, 14689597 **0.159**) was discarded; numbers below are the **fixed** steal.

### All-blocks N=1 @8 (98 nonempty / 99 loaded)

Empty snapshot **19910734** (`n_tx=0`) dropped — same rule as parallel-compute. Official `block_pairs.sf_occ`.

| | This cut | Parallel-compute to match |
|--|----------|---------------------------|
| median SF/OCC | **0.655** | **0.744** |
| p10 / min (nonempty) | 0.428 / **0.292** | 0.468 / **0.234** |
| mean | 1.75 | 1.74 |
| quiet heuristic (36 nonempty) median | **1.050** (19/36 ≥1) | 1.020 (24/46 ≥1) |
| quiet p10 | **0.559** | — |
| ≥0.7 / ≥1.0 | **44 / 22** | 56 / 27 |

Soft=0, await=0, exclude-set=0. Detect 305 281; `unfenced_occ_fast` 326 876; `pcc_fire_at_a` 4 571; `pcc_roi_skip` 479; `occ_kernel_execs` 61 871; `pcc_kernel_execs` 4 136; `occ_kernel_validates` 96 978; `prefer_admit` 16 407; `edge_wait_for` 132; `edge_bind` 4 439.

**OCC mode:** detect / pcc_fire / unfenced / occ_kernel_* = **0**.

`occ_kernel_*` / `pcc_kernel_*` metrics are **aliases**: spec-only incarnations vs fenced incarnations. They are not a live Occ\|Pcc SoT.

| Block | Role | SF/OCC (N=1) | parallel-compute N=1 |
|------:|------|-------------:|---------------------:|
| 14689597 | fan_out/spine | **0.558** | 0.535 |
| 2179522 | quiet | **1.526** | 0.234 |
| 19807137 | fan_out | 0.383 | 0.465 |
| 6196166 | fan_out | 0.460 | 0.597 |
| 6137495 | spine | 0.452 | — |
| 19606599 | spine | 0.625 | — |
| 19469097 | spine | 0.580 | — |
| 19606598 | quiet | 0.677 | — |

14689597 N=1 **0.558** (bind=87, wait=1, aborts=177). Better than this branch’s pre-SoT-gap 0.365; ≈ PC 0.535. Still Bind/B0 vs OCC reincarnation.

2179522 N=1: bind=0, SF 1.9 vs OCC 2.8 ms. **Do not advertise 1.526 as a quiet win** — OCC was slow this sample.

Mean inflated by 19434587 OCC N=1 pathology.

### Focus+worst+quiet N=3 (8-block set)

Printed median **0.541** / mean **0.532** / min **0.368**. Soft=0. Prior parallel-compute N=3 median **0.606** / min **0.268**.

| Block | Role | SF/OCC | parallel-compute N=3 |
|------:|------|-------:|---------------------:|
| 14689597 | focus | **0.454** | 0.268 |
| 19606599 | focus | 0.541 | 0.606 |
| 19469097 | focus | 0.557 | 0.678 |
| 19807137 | worst | 0.428 | 0.563 |
| 6196166 | park | 0.368 | 0.591 |
| 6137495 | worst-ish | 0.483 | 0.646 |
| 2179522 | quiet | **0.751** | **3.565** |
| 19606598 | quiet neighbor | 0.677 | 0.571 |

2179522 N=3 is the honest quiet tail: SF 2.2 vs OCC 1.6 ms, bind=0, ratio **0.751** (was 0.112 on the recreation plant). Parallel-compute N=3 **3.565** was OCC pathology — do not invert the story. Report **N=3 0.751** + all-blocks quiet **median 1.050**.

---

## Remaining (honest)

1. Nonempty median **0.655** vs 0.744 — architecture SoT landed; wall is a **regression**, not a product win. More Bind/WaitFor (4.4k / 132) and execute-first changed the mix.  
2. 14689597 N=1 **0.558** / N=3 **0.454** — better than the recreation’s 0.365, still ≪ SoT bar 0.85.  
3. Quiet p10 **0.559** ≪ SoT 0.85. 2179522 N=3 **0.751** is the named quiet tail (do not advertise N=1 1.526).  
4. Per-worker steal deques not built; schedule is Block-STM indices + wave ready + execute-if-Ready.  
5. AccessOrdinalLog still writes a per-tx first_k map on Spec (±k-log vs OCC).
