# SpecFence mixed-49 Soft=0 TPS — PR #45

**Date:** 2026-09-22
**Engine tip:** `06fd9d9` on [#45](https://github.com/fengjy73/pevm/pull/45) (`cursor/specfence-sf-ps-true-spine-d6e8`). No scheduler change for this pass.
**Corpus:** the 49 `mixed_RAW_WAW` blocks. Not the full 99. Fans `14689597`, `4864590`, `15537394` are excluded. Hang-correctness blocks outside the 49 (including `19807137`) were not run.
**Harness:** `specfence_all_blocks_sweep`, Soft=0, Instant-off (`SPECFENCE_ALL_REUSE=1`), N=3, 8 cores, `SPECFENCE_ALL_PROCESS_TOP=0`.
**Raw:** `lab/results/mixed-49-pr45-soft0-n3.json` (gitignored) and `lab/results/mixed-49-pr45-soft0-n3-summary.json`.

## Metric

Win = SpecFence **reuse** wall ≤ OCC **median** wall.

- SpecFence reuse median is `sorted[len/2]` of iterations 1 and 2 (the slower of the two reuse samples). Iteration 0 is cold. Same index rule as `specfence_3356896_compare`.
- OCC builds a new `Pevm` every iteration. OCC median is `sorted[len/2]` of all three OCC walls.
- Ratio = SF reuse median / OCC median. Median ratio below is the median of those 49 ratios.
- `occ_schedule_picks` is recorded on every SpecFence iteration (`occ_schedule_picks_iters`). It is `[0, 0, 0]` on all 49 blocks. `soft_wait_arms` is `[0, 0, 0]` on all 49. `next_task*` was not restored.

The sweep used to take cold and reuse after an in-place sort of all iterations. This run snapshots iteration 0 as cold and iterations 1.. as reuse before that sort.

## Result

**Win-rate: 3/49 (6%).** Median SF/OCC ratio: **2.15**. Mean ratio: 2.22.

Median SF reuse wall across blocks: 11.3 ms. Median OCC wall across blocks: 5.9 ms. Ratio of those two medians: 1.93.

| Type | n | Wins | Win-rate | Median SF/OCC |
|:----:|--:|-----:|-------------:|-------------:|
| A | 3 | 0/3 | 0% | 1.42 |
| B | 6 | 0/6 | 0% | 2.73 |
| C | 21 | 0/21 | 0% | 2.24 |
| D | 3 | 0/3 | 0% | 2.44 |
| E | 16 | 3/16 | 19% | 1.92 |

Wins, all type E: `19933122` (0.88), `19934116` (0.92), `19426587` (0.97).

Loaded 49/49. No skip, no hang, no abort, no allocator fault.

Next press targets **C and B losers**, not the full 99. A is the closest non-winning type (median 1.42, no wins). B is the worst type (median 2.73).

## Worst 10 losers

Reuse column is the slower of the two reuse walls. The pair of reuse samples is listed so a single slow sample is visible (`3356896` reuse was 3.14 ms and 7.97 ms).

| Block | Type | n | SF cold ms | SF reuse ms | SF reuse samples | OCC cold ms | OCC median ms | SF/OCC |
|------:|:----:|--:|-----------:|------------:|------------------:|------------:|--------------:|-------:|
| 3356896 | E | 176 | 2.83 | 7.97 | 3.14, 7.97 | 2.89 | 1.47 | 5.42 |
| 19638737 | E | 381 | 11.90 | 22.00 | 13.82, 22.00 | 6.35 | 5.38 | 4.09 |
| 8889776 | C | 330 | 14.87 | 12.88 | 11.79, 12.88 | 3.70 | 3.68 | 3.50 |
| 16146267 | E | 473 | 13.12 | 16.15 | 16.15, 12.67 | 4.93 | 4.93 | 3.27 |
| 15274915 | B | 1226 | 25.73 | 27.11 | 27.11, 24.68 | 8.40 | 8.40 | 3.23 |
| 19469097 | C | 336 | 31.01 | 27.57 | 27.57, 23.16 | 8.64 | 8.64 | 3.19 |
| 19860366 | C | 430 | 27.35 | 33.83 | 27.17, 33.83 | 13.58 | 10.67 | 3.17 |
| 19505152 | C | 417 | 19.71 | 25.90 | 25.90, 23.35 | 9.62 | 9.13 | 2.84 |
| 19469101 | C | 469 | 21.26 | 25.52 | 25.52, 25.06 | 8.37 | 9.18 | 2.78 |
| 14383540 | B | 722 | 15.54 | 20.31 | 14.22, 20.31 | 7.43 | 7.31 | 2.78 |

`19932703` (type C) had one OCC iteration at 2364 ms. Its OCC median is 7.64 ms, and the SF/OCC ratio on that median is 1.31. It is not in the worst 10.

## All 49

Walls are milliseconds. Blank win cell means SF reuse median was slower than OCC median.

| Block | Type | n | SF cold | SF reuse | OCC cold | OCC median | SF/OCC | Win |
|------:|:----:|--:|--------:|---------:|---------:|-----------:|-------:|:---:|
| 13217637 | A | 1100 | 10.30 | 12.64 | 7.17 | 6.55 | 1.93 | |
| 15199017 | A | 866 | 10.05 | 9.21 | 6.71 | 6.50 | 1.42 | |
| 8038679 | A | 237 | 2.54 | 2.61 | 1.88 | 1.88 | 1.39 | |
| 15274915 | B | 1226 | 25.73 | 27.11 | 8.40 | 8.40 | 3.23 | |
| 14383540 | B | 722 | 15.54 | 20.31 | 7.43 | 7.31 | 2.78 | |
| 14334629 | B | 819 | 17.37 | 21.79 | 10.20 | 8.00 | 2.73 | |
| 17666333 | B | 961 | 21.91 | 22.25 | 9.07 | 9.68 | 2.30 | |
| 15538827 | B | 823 | 17.22 | 17.67 | 7.64 | 8.66 | 2.04 | |
| 14029313 | B | 724 | 8.45 | 9.96 | 6.13 | 6.13 | 1.62 | |
| 8889776 | C | 330 | 14.87 | 12.88 | 3.70 | 3.68 | 3.50 | |
| 19469097 | C | 336 | 31.01 | 27.57 | 8.64 | 8.64 | 3.19 | |
| 19860366 | C | 430 | 27.35 | 33.83 | 13.58 | 10.67 | 3.17 | |
| 19505152 | C | 417 | 19.71 | 25.90 | 9.62 | 9.13 | 2.84 | |
| 19469101 | C | 469 | 21.26 | 25.52 | 8.37 | 9.18 | 2.78 | |
| 17034869 | C | 93 | 5.33 | 7.85 | 3.26 | 2.96 | 2.65 | |
| 18988207 | C | 186 | 13.14 | 12.65 | 5.27 | 4.94 | 2.56 | |
| 17034870 | C | 184 | 19.13 | 20.65 | 7.76 | 8.08 | 2.56 | |
| 19716145 | C | 341 | 27.48 | 28.73 | 13.16 | 12.00 | 2.39 | |
| 19606599 | C | 367 | 35.58 | 34.91 | 14.71 | 15.13 | 2.31 | |
| 19932148 | C | 227 | 10.79 | 14.37 | 6.34 | 6.41 | 2.24 | |
| 9069000 | C | 56 | 3.79 | 5.58 | 2.92 | 2.67 | 2.09 | |
| 14683600 | C | 660 | 22.18 | 20.62 | 9.77 | 9.88 | 2.09 | |
| 12459406 | C | 201 | 13.02 | 13.99 | 6.13 | 6.86 | 2.04 | |
| 19469099 | C | 257 | 14.92 | 16.20 | 9.26 | 8.00 | 2.02 | |
| 19737292 | C | 195 | 11.42 | 10.31 | 5.53 | 5.35 | 1.93 | |
| 19469098 | C | 268 | 8.87 | 8.77 | 5.26 | 5.25 | 1.67 | |
| 14689598 | C | 111 | 3.34 | 3.54 | 2.24 | 2.24 | 1.58 | |
| 18426253 | C | 147 | 7.75 | 8.72 | 5.96 | 5.86 | 1.49 | |
| 19932703 | C | 143 | 6.46 | 9.99 | 2364.15 | 7.64 | 1.31 | |
| 16257471 | C | 98 | 7.94 | 7.78 | 6.25 | 6.07 | 1.28 | |
| 19932810 | D | 270 | 15.51 | 17.34 | 5.83 | 6.88 | 2.52 | |
| 10760440 | D | 202 | 10.91 | 12.16 | 4.98 | 4.98 | 2.44 | |
| 5283152 | D | 150 | 3.32 | 4.04 | 1.99 | 1.87 | 2.15 | |
| 3356896 | E | 176 | 2.83 | 7.97 | 2.89 | 1.47 | 5.42 | |
| 19638737 | E | 381 | 11.90 | 22.00 | 6.35 | 5.38 | 4.09 | |
| 16146267 | E | 473 | 13.12 | 16.15 | 4.93 | 4.93 | 3.27 | |
| 19929064 | E | 103 | 4.45 | 7.88 | 3.02 | 3.03 | 2.60 | |
| 12243999 | E | 205 | 6.12 | 8.62 | 3.93 | 3.74 | 2.30 | |
| 11743952 | E | 206 | 12.72 | 20.50 | 9.30 | 8.94 | 2.29 | |
| 15752489 | E | 132 | 4.35 | 5.40 | 2.45 | 2.45 | 2.20 | |
| 12244000 | E | 133 | 8.32 | 11.31 | 5.55 | 5.88 | 1.92 | |
| 19917570 | E | 116 | 6.52 | 7.84 | 5.27 | 4.14 | 1.89 | |
| 12159808 | E | 180 | 8.96 | 8.63 | 6.60 | 5.34 | 1.62 | |
| 19933597 | E | 154 | 8.66 | 5.31 | 3.62 | 3.38 | 1.57 | |
| 19606598 | E | 91 | 2.80 | 2.71 | 1.92 | 1.91 | 1.41 | |
| 11114732 | E | 100 | 5.05 | 5.95 | 4.93 | 4.43 | 1.34 | |
| 19426587 | E | 37 | 2.08 | 2.01 | 2.38 | 2.07 | 0.97 | yes |
| 19934116 | E | 58 | 1.30 | 1.27 | 1.57 | 1.38 | 0.92 | yes |
| 19933122 | E | 45 | 0.50 | 0.47 | 0.66 | 0.53 | 0.88 | yes |
