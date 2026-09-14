# SpecFence v9.4 full-land Soft=0 honesty sweep

**When:** 2026-09-14 15:36 CST  
**Tip:** `3687da6` (PR #10)  
**Binary:** `specfence_all_blocks_sweep` release, LTO off, Soft=0 plant  
**Artifacts:** `lab/results/v9.4-full-land-all-blocks-n1-sweep.json`, `lab/results/v9.4-full-land-focus-n3-sweep.json`, `lab/notes/v9.4-full-land-sweep-summary.json`

## Protocol

- Fresh `Pevm` + `reset_heat` + `reset_inter_prior` per iter (sweep harness).
- SF vs OCC @**8** cores; all ethereum snapshots; drop `n_tx==0` (**19910734**).
- Soft must be 0 — **held** (`soft_wait_arms=0` on every SF row / aggregate soft=0).
- N=1 coverage on **98** nonempty; N=3 focus on named set.

## Headline vs base (`bb67ff7` Soft=0)

| Metric | Base | This tip | Δ |
|--------|-----:|---------:|--:|
| nonempty median SF/OCC | **0.728** | **0.6853** | -0.0427 |
| 14689597 N=3 SF/OCC | **0.362** | **0.3482** | -0.0138 |
| quiet p10 | (base n/a in brief) | **0.5124** | — |
| Soft | 0 | **0** | held |

**No celebration.** Median and fan both miss product bars (median≥0.95, fan≥0.90, quiet p10≥0.90).

## All-blocks N=1 @8 (n=98)

| | |
|--|--:|
| median / p10 / p90 / min / mean | 0.6853 / 0.4641 / 1.1778 / 0.1953 / 2.5308 |
| ≥0.7 / ≥0.95 / ≥1.0 | 46 / 25 / 22 |
| quiet n / median / p10 / ≥1 | 33 / 1.0634 / 0.5124 / 21/33 |
| morph | {'spine': 53, 'quiet': 33, 'quiet_ish': 12} |
| Soft / Bind / WaitFor | 0 / 2208 / 2151 |
| WaitFor pin / aborting | 2061 / 3280 |
| Bind-after-Done | 205 (share of Bind 0.0928) |
| R1 win / attempt (rate) | 6 / 1636 (0.0037) |
| schedule_refuse | 0 |

## Named Soft=0 N=3 @8

| bn | N=1 sf_occ | N=3 sf_occ | N=3 Bind | N=3 Wait | N=3 pin | N=3 R1 w/a | N=3 bind_after_done |
|---:|----------:|----------:|---------:|---------:|--------:|-----------:|--------------------:|
| 14689597 | 0.3375 | **0.3482** | 472 | 47 | 45 | 0/42 | 1 |
| 19807137 | 0.1953 | **0.2265** | 202 | 442 | 420 | 4/491 | 90 |
| 2179522 | 1.6857 | **0.6382** | 0 | 0 | 0 | 0/0 | 0 |
| 19606599 | 0.6825 | **0.6922** | 55 | 46 | 44 | 0/41 | 1 |
| 19469097 | 0.6866 | **0.6247** | 65 | 72 | 70 | 1/54 | 4 |

Notes:

- **2179522** N=1 (1.6857) is OCC-slow noise; advertise **N=3 0.6382**.
- Fan **14689597** still Bind-heavy; R1 almost never wins certs on that block.
- **19807137** remains worst (N=3 **0.2265**); WaitFor/pin/aborting dominate park idle.

## Product bars (this JSON)

| Bar | Result |
|-----|--------|
| Soft=0 | **held** |
| nonempty median ≥0.95 | **miss** (0.6853) |
| 14689597 ≥0.90 @8 N≥3 | **miss** (0.3482) |
| quiet p10 ≥0.90 | **miss** (0.5124) |
| R1 win ≥50% attempts | **miss** (0.0037) |
| Bind-after-Done theater | path fixed; share still **0.0928** of Bind |

Soft=0 held on all SF rows. Nonempty median 0.685 vs base 0.728 (delta -0.0427); 14689597 N=3 0.3482 vs base 0.362 (delta -0.0138). Quiet median ~1.06 but quiet p10 0.51 misses ≥0.90. R1 path live (attempt>0) but win rate near-zero. 2179522 N=1 can look OCC-slow; use N=3 (0.638). Do not claim product bars.
