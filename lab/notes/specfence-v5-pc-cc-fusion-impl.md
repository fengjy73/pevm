# SpecFence v5 PC⊗CC fusion — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-v5-pc-cc-fusion-e28e` (PR #7 → `cursor/specfence-parallel-compute-68a3` / PR #6)  
**Plant SoT:** `lab/notes/specfence-complete-architecture-v5-pc-cc-fusion.md` (**authoritative** from `987b196`, not the recreation)  
**Catalog:** `lab/notes/specfence-pc-cc-fusion-per-block-catalog.json`  
**π SoT:** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md` (unchanged)  
**Honesty baseline:** parallel-compute nonempty N=1 median **SF/OCC = 0.744**; quiet **1.020**

**Verdict:** rebased onto the pushed SoT, then closed remaining plant gaps vs that document (AccessOrdinalLog module, deleted `quiet_fence_off`/`park_storm` decide gates, execute-first steal, Spec PE-publish wake). Incarnation Occ\|Pcc fork removed as SoT. Pre-rebase nonempty median **0.743** (flat vs 0.744). **Re-sweep after this cut** — numbers below are the pre-rebase honesty until the new all-blocks JSON lands. Soft=0. **Do not claim ≥0.7 as a finished product bar.**

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
  detect; k := note_access_k_only(ℓ)
  empty PE → Spec (OCC)
  ¬PE(ℓ,k) → Spec
  vis := e_vis + HotSet + WŜ
  decide → Spec | Bind | WaitFor | SerialLane
  first real Fence → certificate (rem / R1), not a tx kernel
  SerialLane: mark class + admit_spine; park only if lane head executing
SpecFence validate:
  ¬certificate → OCC bool + B0 + PE(true k)
  certificate → R1a / R1b / B0
schedule: next_task_with_wave only
  (do not steal on cumulative wait_park_count — that starved validate)
```

`decide` order (after Bind-theater fix): empty/¬PE → Spec; quiet/park_storm → Bind only if Data ∧ unfinished=0; unfinished>1 or ((lane∨HotSet) ∧ unfinished>0) → SerialLane; unfinished==1 ∧ executing → WaitFor; Data ∧ unfinished=0 → Bind; else Spec.

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
`lab/results/v5-fusion-all-blocks-sweep.json`,  
`lab/results/v5-fusion-focus-n3-sweep.json`.  
Committed digest: `lab/notes/v5-fusion-sweep-summary.json`.

### All-blocks N=1 @8 (98 nonempty / 99 loaded)

Empty snapshot **19910734** (`n_tx=0`) dropped — same rule as parallel-compute. Official `block_pairs.sf_occ`.

| | This cut | Parallel-compute to match |
|--|----------|---------------------------|
| median SF/OCC | **0.743** | **0.744** |
| p10 / min (nonempty) | 0.484 / **0.233** | 0.468 / **0.234** |
| mean | 1.75 | 1.74 |
| quiet heuristic (44 nonempty) median | **1.035** (25/44 ≥1) | 1.020 (24/46 ≥1) |
| quiet+ish (60) median | **0.932** | 0.922 (63) |
| fan_out heuristic (2) median | **0.274** | 0.468 (3) |
| ≥0.7 / ≥1.0 | **51 / 28** | 56 / 27 |

Soft=0, await=0, exclude-set=0 (`force_prefix_*` / canary / Soft = 0). Detect 272 615; `unfenced_occ_fast` 294 783; `pcc_fire_at_a` 3 931; `pcc_roi_skip` 1 085; `occ_kernel_execs` 52 228; `pcc_kernel_execs` 3 656; `occ_kernel_validates` 112 517; `prefer_admit` 14 693; `edge_wait_for` 71; `edge_bind` 3 860.

**OCC mode:** detect / pcc_fire / unfenced / occ_kernel_* = **0**.

`occ_kernel_*` / `pcc_kernel_*` metrics are **aliases**: spec-only incarnations vs fenced incarnations. They are not a live Occ\|Pcc SoT.

| Block | Role | SF/OCC (N=1) | parallel-compute N=1 |
|------:|------|-------------:|---------------------:|
| 14689597 | fan_out/spine | **0.365** | 0.535 |
| 2179522 | quiet | **1.567** | 0.234 |
| 19807137 | spine | 0.399 | 0.465 |
| 6196166 | fan_out | 0.315 | 0.597 |
| 6137495 | spine | 0.590 | — |
| 19606599 | spine | 0.579 | — |
| 19469097 | spine | 0.673 | — |
| 19606598 | quiet | 1.063 | — |

14689597 N=1: bind=111, wait=1, prefer_admit=1077, park_idle≈2.90. Still Bind/B0 vs OCC reincarnation.

2179522 N=1: bind=0, pcc_fire=0, SF 1.9 vs OCC 3.0 ms. **Do not advertise 1.567 as a quiet win** — OCC was slow this sample.

Mean inflated by 19434587 OCC N=1 pathology (sf_occ ≈90).

### Focus+worst+quiet N=3 (8-block set)

Printed median **0.523** / mean **0.509** / min **0.112**. Soft=0. Prior parallel-compute N=3 median **0.606** / min **0.268**.

| Block | Role | SF/OCC | parallel-compute N=3 |
|------:|------|-------:|---------------------:|
| 14689597 | focus | **0.523** | 0.268 |
| 19606599 | focus | 0.672 | 0.606 |
| 19469097 | focus | 0.740 | 0.678 |
| 19807137 | worst | 0.410 | 0.563 |
| 6196166 | park | 0.412 | 0.591 |
| 6137495 | worst-ish | 0.499 | 0.646 |
| 2179522 | quiet | **0.112** | **3.565** |
| 19606598 | quiet neighbor | 0.703 | 0.571 |

2179522 N=3 is the honest quiet tail: SF 16.5 vs OCC 1.8 ms, bind=0, unfenced=3346, dominant `meta/cold`. Parallel-compute N=3 **3.565** was OCC pathology (OCC 17.8 vs SF 5.0) — do not invert the story. Report **N=3 0.112** + all-blocks quiet **median 1.035**.

---

## Remaining (honest)

1. Quiet tail 2179522 N=3 **0.112** — still meta/schedule vs a fast OCC sample (`note_access_k_only` + detect on empty-PE path).  
2. Named 14689597 N=1 **0.365** (was 0.535) — serial-lane/Bind still loses to OCC reincarnation; N=3 recovered to 0.523.  
3. Fan_out cohort median **0.274** (2 blocks) vs PC 0.468 (3) — Bind theater is gone, but unfinished>1 now parks a lane.  
4. Per-worker steal deques not built; schedule is Block-STM indices + wave ready.  
5. Median is **flat** (0.744→0.743). Architecture SoT is the landing; wall is not a product win.
