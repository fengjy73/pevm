# Mainnet multi-core sweep (real numbers)

Sweep: `cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- --out lab/results/mainnet-sweep.json`
Started 2026-09-03 22:00:06 CST, finished 22:00:13 CST (≈7.6 s wall). 273 rows, **0 `ok=false`**.
All 7 configured blocks had snapshots (5 already on disk; 14683600 and 19434587 fetched via BlastAPI). Sequential @ 1 worker; occ/pcc/specfence @ 1,2,4,8 workers; 3 repeats; `reset_heat` each repeat.

## Per-block size

| block    | n_tx | gas_used |
|----------|------|----------|
| 19807137 | 712  | 29,981,386 |
| 14683600 | 660  | 30,021,734 |
| 13217637 | 1100 | 29,985,362 |
| 14383540 | 722  | 30,059,751 |
| 15199017 | 866  | 30,028,395 |
| 14029313 | 724  | 30,074,554 |
| 19434587 | 390  | 28,904,241 |

First sequential repeat of 19807137 is a process warmup (3102 ms / 229 TPS vs ~22 ms / ~32k TPS on r1–r2). Later sequential means use the warm numbers. Sequential mean for 19807137 is dragged to 21,409 TPS by that cold point.

## TPS vs cores (mean of 3 repeats)

OCC **rises with cores on every block**, with diminishing returns at 8: e.g. 15199017 81k→129k→194k→253k; 13217637 78k→132k→202k→231k; 19434587 16k→27k→38k→42k (levels).

PCC is mixed. It **rises** on 14029313 (64k→99k→155k→186k), 15199017 (74k→123k→137k→150k), 14683600 (31k→50k→85k→89k), 14383540 (50k→75k→78k→90k), 19807137 (30k→46k→49k→61k). It **drops then recovers / levels** on 13217637 (94k @1 → 74k @2 → 102k @4 → 95k @8). On 19434587 it rises then **levels/drops** (17k→27k→33k→33k).

SpecFence **rises** on 14029313 (75k→117k→168k→196k), 14383540 (44k→70k→89k→123k), 15199017 (81k→124k→145k→154k), 19807137 (31k→50k→69k→72k). It **rises then levels** on 14683600 (31k→56k→78k→80k) and 19434587 (16k→28k→34k→34k). It **rises then drops** on 13217637 (73k→108k→120k→87k at 8 cores).

At 8 cores OCC is the throughput leader on every block. SpecFence sits between OCC and PCC except on 14029313 (SpecFence 196k ≈ OCC 191k > PCC 186k) and 13217637 (OCC 231k >> SpecFence 87k ≈ PCC 95k).

## Abort-rate trend

OCC `occ_aborts` is **0 on every row** (abort_rate 0.000). Either these in-memory blocks really abort-free under Block-STM lazy beneficiary, or Occ mode does not populate `last_specfence_metrics().occ_aborts`. Do not treat OCC abort=0 as a proven conflict-free claim.

PCC and SpecFence abort_rate **increase with cores** on all blocks. Highest on 19434587 (PCC 0.00→0.05→0.20→0.25; SpecFence 0.00→0.05→0.19→0.22). Next is 14383540 (SpecFence 0.00→0.008→0.021→0.049) and 14683600 (0.00→0.010→0.025→0.038). Lowest on 13217637 and 14029313 (≤0.005 at 8 cores).

## SpecFence Wait vs Speculate

SpecFence is **mixed on every block**: `speculate_executions` is non-zero on all 84 SpecFence rows (means 88–322; even at 1 core, 86–313). `wait_admissions` is 0 at 1 core (single worker never waits) and non-zero at 2/4/8 cores on every block (2-core means 97–875; 8-core means 269–964). `region_promotions` also non-zero (44–138 mean). So SpecFence does **not** collapse to OCC (Wait would stay 0) or PCC (Speculate would stay 0). Wait counts are in the same ballpark as PCC at the same core count; SpecFence additionally records 1.5–2× more promotions than PCC.

CSV sibling: `lab/results/mainnet-sweep.csv` (273 rows). Figures: `lab/figures/block_<id>_tps_abort.png`, `overview_tps_abort.png`, `specfence_wait_vs_speculate.png`.
