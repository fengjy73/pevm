# SpecFence parallel-compute — implementation map

**Date:** 2026-09-13  
**Branch:** `cursor/specfence-parallel-compute-68a3` (PR #6 → `cursor/specfence-clean-slate-8598` / PR #5)  
**Plant SoT:** `lab/notes/specfence-parallel-compute-architecture.md`  
**π SoT:** `lab/notes/specfence-complete-architecture-v4-frozen-grain.md` (unchanged)  
**Honesty baseline:** clean-slate nonempty N=1 median **SF/OCC = 0.464**; quiet **0.716**

**Verdict:** one-iteration parallel-computer split landed. Nonempty median **0.744** (↑ vs 0.464). Quiet heuristic median **1.020** (24/46 ≥1). Soft=0. **Do not claim ≥0.7 as a finished product bar** — 0.744 is this run’s median, not a guarantee; fan_out named blocks still 0.27–0.53.

---

## Land map (file:fn)

| # | SoT item | file:fn | Status |
|---|---------|---------|--------|
| 1 | Architecture SoT first | `lab/notes/specfence-parallel-compute-architecture.md` | **landed** |
| 2 | Incarnation kernel Occ/Pcc | `kernel.rs::KernelTable` | **landed** |
| 3 | OCC schedule zero wave/fence | `pevm.rs` worker + `next_occ_task` | **landed** |
| 4 | OCC validate = bool walk + B0 | `executor.rs::validate_occ_stage` + `mv_memory::validate_read_locations` | **landed** |
| 5 | SpecFence OccKernel validate | `executor.rs::validate_occ_kernel` (no RebindThis) | **landed** |
| 6 | rem / CallEntry / wake only PccKernel | `vm.rs::execute` finalize | **landed** |
| 7 | PCC Fire upgrades kernel | `vm.rs::pcc_overlay` `mark_pcc` | **landed** |
| 8 | Abort trains PE with residual k=1 | `validate_occ_kernel` | **landed** |
| 9 | HotSet / WŜ observe off rem | OccKernel finalize observe-only | **landed** |
| 10 | SF schedule ready+steal | `next_sf_task` / wave | **landed** |

**Not claimed:** work-stealing deques per worker (Block-STM indices + wave ready remain); PrefixSkip still PccKernel-only museum.

---

## Control loop (live)

```
OCC:     next_task(); occ_read; validate_read_locations; B0
SpecFence execute:
  kernel := Occ unless Repair armed
  access: detect; Unfenced → occ_read; PE∩ROI → pcc (mark Pcc)
  rem/CallEntry/wake  iff PccKernel
  HotSet observe on both kernels (not rem)
SpecFence validate:
  OccKernel → OCC bool + B0 (+ note_abort k=1)
  PccKernel → R1a / R1b / B0
```

---

## Tests

| Suite | Result |
|-------|--------|
| `cargo +nightly test -p pevm --lib --release -- --test-threads=1` | **163 ok** |
| `cargo +nightly test -p pevm --test specfence --release -- --test-threads=1` | **42 ok / 20 ignored** |
| erc20 / raw_transfers / mixed / uniswap / beneficiary / small_blocks | **all ok** |

seq≡par held on those suites. SoftWait Soft = 0 in sweeps.

---

## Honesty vs 0.464

Sweeps (gitignored JSON):  
`lab/results/parallel-compute-all-blocks-sweep.json`,  
`lab/results/parallel-compute-focus-sweep.json`.  
Committed digest: `lab/notes/parallel-compute-sweep-summary.json`.

### All-blocks N=1 @8 (98 nonempty / 99 loaded)

Empty snapshot **19910734** (`n_tx=0`) dropped — same rule as clean-slate.

| | This cut | Clean-slate to beat |
|--|----------|---------------------|
| median SF/OCC | **0.744** | **0.464** |
| p10 / min (nonempty) | 0.468 / **0.234** | 0.288 / **0.206** |
| mean | 1.74 | 1.36 |
| quiet heuristic (46 nonempty) median | **1.020** (24/46 ≥1) | 0.716 (20/48 ≥1) |
| quiet+ish (63) median | **0.922** | — |
| fan_out heuristic (3) median | **0.468** | 0.399 (18 — cohort cut differs) |
| ≥0.7 / ≥1.0 | **56 / 27** | 29 / 21 |

Soft=0, await=0, exclude-set=0 on all SF rows. Detect 282 061; `unfenced_occ_fast` 304 527; `pcc_fire_at_a` 3 636; `occ_kernel_execs` 63 597; `pcc_kernel_execs` 3 444; `occ_kernel_validates` 116 828.

**OCC mode:** detect / pcc_fire / unfenced / occ_kernel_execs = **0**.

| Block | Role | SF/OCC (N=1) | clean-slate N=1 |
|------:|------|-------------:|----------------:|
| 19807137 | worst/spine | **0.465** | 0.323 |
| 6196166 | park | **0.597** | 0.317 |
| 14689597 | focus (spine here) | **0.535** | 0.229 |
| 2179522 | quiet | **0.234** | 0.405 |

2179522 N=1 still Unfenced≢OCC wall (SF 5.9 vs OCC 1.4 ms, bind=0). Quiet **median** is ≈1.0; this block is the quiet tail.

### Focus+worst+quiet N=3 (8-block set)

Printed median **0.606** / mean **0.936** / min **0.268**. Soft=0. Prior clean-slate N=3 median **0.426** / min **0.204**.

| Block | Role | SF/OCC | clean-slate N=3 |
|------:|------|-------:|----------------:|
| 14689597 | focus | 0.268 | 0.233 |
| 19606599 | focus | 0.606 | 0.533 |
| 19469097 | focus | 0.678 | 0.431 |
| 19807137 | worst | 0.563 | 0.204 |
| 6196166 | park | 0.591 | 0.426 |
| 6137495 | worst-ish | 0.646 | 0.330 |
| 2179522 | quiet | **3.565** | **0.385** |
| 19606598 | quiet neighbor | 0.571 | 0.460 |

2179522 N=3 is OCC N=1-style pathology (OCC wall 17.8 ms vs SF 5.0). Do **not** advertise 3.56 as a quiet win; use the all-blocks quiet median.

Mean inflated by 19434587 OCC N=1 pathology (sf_occ ≈89).

---

## Remaining (honest)

1. Quiet tail 2179522 N=1 **0.234** — still meta/schedule vs a fast OCC sample.  
2. Named fan_out 14689597 still 0.27–0.53 — PCC Bind/B0 vs OCC reincarnation.  
3. Per-worker steal deques not built; schedule is Block-STM + wave.  
4. PccKernel Resolve museum remains for PrefixSkip.

Plant invariants held: OCC zero SF counters; OccKernel first incarnation is OCC ESTIMATE→Blocking; PCC only on PE ∩ ROI; exclude-set 0; Soft=0; journal-less RebindThis = 0.
