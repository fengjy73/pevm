# SpecFence v9.4 Soft=0 honesty sweep

## This package (`c42f96a`, PR #11) — remaining PARTIAL close

**When:** 2026-09-14  
**Binary:** `specfence_all_blocks_sweep` release, LTO off, Soft=0 plant  
**Artifacts:** `lab/results/v9.4-sot-partial-all-blocks-n1-sweep.json`, `lab/results/v9.4-sot-partial-focus-n3-sweep.json`, `lab/notes/v9.4-sot-partial-sweep-summary.json`  
**Protocol:** same as below (fresh `Pevm` + `reset_heat` + `reset_inter_prior`; drop `n_tx==0`; SF vs OCC @8).

### Headline vs wall (`3687da6` Soft=0)

| Metric | Wall `3687da6` | This tip | Δ |
|--------|---------------:|---------:|--:|
| nonempty median SF/OCC | **0.6853** | **0.7033** | +0.0180 |
| 14689597 N=3 SF/OCC | **0.3482** | **0.4485** | +0.1003 |
| Soft | 0 | **0** | held |

**Wall beaten.** Product bars (median ≥0.95, fan ≥0.90, quiet p10 ≥0.90, R1 ≥50%) still **miss**. Do not claim crush of 0.95 / 0.90.

### All-blocks N=1 @8 (n=98 nonempty)

| | |
|--|--:|
| median / p10 / p90 / min / mean | 0.7033 / 0.4136 / 1.2143 / 0.2729 / 148.2002 |
| ≥0.7 / ≥0.95 / ≥1.0 | 50 / 29 / 23 |
| quiet n / median / p10 / ≥1 | 36 / 1.0732 / 0.4329 / 19/36 |
| morph | {'spine': 53, 'quiet': 36, 'quiet_ish': 9} |
| Soft / OrderedAdmit / WaitFor | **0** / **0** / 4875 |
| WaitFor wait_for_dependency / aborting | **5158** / **177** |
| OrderedAdmit-after-Done | 214 (OrderedAdmit verb = 0; cert-without-OrderedAdmit) |
| partial_abort win / attempt (rate) | 3 / 2439 (0.0012) |
| refuse_admit | **1540** |

OrderedAdmit rare landed in volume (2208 → 0). wait_for_dependency landed (`wait_for_full_abort` 3280 → 177). Schedule-first refuse fires (0 → 1540). R1 still token.

### Named Soft=0 N=3 @8

| bn | N=1 sf_occ | N=3 sf_occ | N=3 OrderedAdmit | N=3 Wait | N=3 wait_for_dependency | N=3 aborting | N=3 R1 w/a | N=3 refuse |
|---:|----------:|----------:|---------:|---------:|--------:|-------------:|-----------:|-----------:|
| 14689597 | 0.3981 | **0.4485** | 0 | 246 | 274 | 4 | 0/74 | 160 |
| 19807137 | 0.3088 | **0.2308** | 0 | 891 | 971 | 0 | 0/537 | 0 |
| 2179522 | (OCC-slow) | (OCC-slow) | 0 | 0 | 0 | 0 | 0/0 | 0 |
| 19606599 | 0.6116 | **0.8312** | 0 | 147 | 153 | 6 | 0/56 | 0 |
| 19469097 | 0.4506 | **0.5075** | 0 | 160 | 161 | 3 | 0/49 | 0 |

Notes:

- **2179522** N=1/N=3 ratios are OCC-slow noise (OCC wall hundreds–tens of thousands of ms). Do **not** advertise them.
- Fan **14689597** N=3 **0.4485** beats the 0.348 wall; OrderedAdmit=0; refuse=160; still misses ≥0.90.
- **19807137** remains worst (N=3 **0.2308**); WaitFor/wait_for_dependency dominate park idle; refuse=0 on that block.

### Product bars (this JSON)

| Bar | Result |
|-----|--------|
| Soft=0 | **held** |
| beat median 0.685 | **yes** (0.7033) |
| beat 14689597 0.348 @8 N≥3 | **yes** (0.4485) |
| nonempty median ≥0.95 | **miss** (0.7033) |
| 14689597 ≥0.90 @8 N≥3 | **miss** (0.4485) |
| quiet p10 ≥0.90 | **miss** (0.4329) |
| partial_abort win ≥50% attempts | **miss** (0.0012) |

---

## Pre-close wall (`3687da6`, PR #10)

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
| Soft / OrderedAdmit / WaitFor | 0 / 2208 / 2151 |
| WaitFor wait_for_dependency / aborting | 2061 / 3280 |
| OrderedAdmit-after-Done | 205 (share of OrderedAdmit 0.0928) |
| partial_abort win / attempt (rate) | 6 / 1636 (0.0037) |
| refuse_admit | 0 |

## Named Soft=0 N=3 @8

| bn | N=1 sf_occ | N=3 sf_occ | N=3 OrderedAdmit | N=3 Wait | N=3 wait_for_dependency | N=3 R1 w/a | N=3 ordered_admit_after_done |
|---:|----------:|----------:|---------:|---------:|--------:|-----------:|--------------------:|
| 14689597 | 0.3375 | **0.3482** | 472 | 47 | 45 | 0/42 | 1 |
| 19807137 | 0.1953 | **0.2265** | 202 | 442 | 420 | 4/491 | 90 |
| 2179522 | 1.6857 | **0.6382** | 0 | 0 | 0 | 0/0 | 0 |
| 19606599 | 0.6825 | **0.6922** | 55 | 46 | 44 | 0/41 | 1 |
| 19469097 | 0.6866 | **0.6247** | 65 | 72 | 70 | 1/54 | 4 |

Notes:

- **2179522** N=1 (1.6857) is OCC-slow noise; advertise **N=3 0.6382**.
- Fan **14689597** still OrderedAdmit-heavy; R1 almost never wins certs on that block.
- **19807137** remains worst (N=3 **0.2265**); WaitFor/wait_for_dependency/aborting dominate park idle.

## Product bars (this JSON)

| Bar | Result |
|-----|--------|
| Soft=0 | **held** |
| nonempty median ≥0.95 | **miss** (0.6853) |
| 14689597 ≥0.90 @8 N≥3 | **miss** (0.3482) |
| quiet p10 ≥0.90 | **miss** (0.5124) |
| partial_abort win ≥50% attempts | **miss** (0.0037) |
| OrderedAdmit-after-Done theater | path fixed; share still **0.0928** of OrderedAdmit |

Soft=0 held on all SF rows. Nonempty median 0.685 vs base 0.728 (delta -0.0427); 14689597 N=3 0.3482 vs base 0.362 (delta -0.0138). Quiet median ~1.06 but quiet p10 0.51 misses ≥0.90. partial_abort path live (attempt>0) but win rate near-zero. 2179522 N=1 can look OCC-slow; use N=3 (0.638). Do not claim product bars.
