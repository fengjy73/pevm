# SpecFence mixed-49 full package land

**Date:** 2026-09-22
**Branch:** `cursor/specfence-sf-ps-true-spine-d6e8` on [#45](https://github.com/fengjy73/pevm/pull/45).
**Soft=0.** `next_task*` / `validate_occ_stage` were not restored. Learn outputs stay G / ArmTable / release / explore. `force_push` is not on the product pick or requeue path (definition and unit tests only). `occ_schedule_picks` is 0 on every SpecFence iteration of the dig-10 and the 49.

Prior board (`06fd9d9`, note `lab/notes/specfence-mixed-49-pr45-soft0-tps-v1.md`): win-rate **3/49**, median SF/OCC **2.15**. Dig: `lab/notes/specfence-mixed-49-worst-deepdive-v1.md`.

The sweep prints `median_sf_occ` as **TPS** `sf_tps/occ_tps` (this run 0.533). The ratios below are wall ratios, same rule as the board: SF reuse median is `sorted[len/2]` of iterations 1 and 2; OCC median is `sorted[len/2]` of all three OCC walls.

Host requested 8 workers. Absolute milliseconds move between runs; compare ratios and counters.

## What landed

**P1.** `validate_to_plan` tries value-stable rebind before any FullReplay, including Opt. If that refuses, a non-lazy EffectiveWAW whose peer is executing or already executed becomes OrderedReplay. A live-only gate (`is_executing` and not `is_executed`) was measured and dropped: `19469097` went back to FullReplay ping-pong (picks/tx about 13, gate_stall about 38 ms).

**P2.** An unfenced FullReplay storm still queues one IntraPatch per location. It does not `mark_under_covered` or sticky-Opt. A Win that applies is sticky with `explore_budget=0`. `promote_short_edge` is not published when `n ≤ 176` (no OrderedAdmit prepaid on the thin shell). The PC width veto is unchanged. `chosen_strategy` is still the CostPolicy census, so a report of Opt does not mean the arm table skipped the patch.

**P3.** `chain_overflow` plants the first live hop and does not extend a live overflow tip. A later writer of that location stays ungated.

**P4.** A failed owner claim calls `release_owner` (CAS `ST_RUNNING` onto the queue) and leaves the 16-step pick loop. `requeue` does the same, and falls back to `wake_idle` when the CAS misses. No `force_push` of a live execute.

**P5.** On `n ≤ 176`, pick drops `flush_pending_idle_edges` and only takes the pending idle set. Skipping lazy higher-reader revalidate was measured and dropped: on `3356896` it committed a different account balance than sequential while receipts still matched (5/5 checks). Lazy writes still enqueue those readers.

## Correctness

Dig-10 Instant-off N=3 @ 8 with `SPECFENCE_COMPARE_CHECK=1`: all ten `seq=par`, exit 0, `occ_schedule_picks=0`, `soft_wait_arms=0`. No hang.

`cargo test -p pevm --lib --release -- --test-threads=1`: **413 passed**, 0 failed.

`complete_arch_edge_pi_seq_eq_par_softwait0`: ok.

49 sweep: loaded 49/49, skipped 0, no hang. Every SpecFence row has `occ_schedule_picks_iters` all 0 and `soft_wait_arms_iters` all 0.

## Mixed-49 wall

**Win-rate 3/49 (6%). Median SF/OCC 2.11** (board 2.15). Mean 2.28 (board 2.22). 23 blocks improved, 26 worsened. Median SF reuse wall 10.0 ms, median OCC wall 4.7 ms.

The three wins are the same gas `< 4_000_000` sequential fallbacks as the board. They are not spine wins.

| Type | n | Wins | Median SF/OCC | Board median |
|:----:|--:|-----:|-------------:|-------------:|
| A | 3 | 0/3 | 1.83 | 1.42 |
| B | 6 | 0/6 | 3.02 | 2.73 |
| C | 21 | 0/21 | 2.27 | 2.24 |
| D | 3 | 0/3 | 2.43 | 2.44 |
| E | 16 | 3/16 | 1.82 | 1.92 |

E's median moved down. A and B moved up. C is flat.

### Worst 10 (this sweep)

| Block | Type | n | SF reuse ms | reuse samples | OCC median ms | SF/OCC | Board |
|------:|:----:|--:|------------:|---------------|--------------:|-------:|------:|
| 17034869 | C | 93 | 13.52 | 13.52, 11.68 | 2.16 | 6.25 | 2.65 |
| 19860366 | C | 430 | 42.99 | 20.64, 42.99 | 8.90 | 4.83 | 3.17 |
| 8889776 | C | 330 | 10.48 | 10.48, 7.34 | 2.51 | 4.18 | 3.50 |
| 15538827 | B | 823 | 19.12 | 19.12, 11.14 | 5.11 | 3.74 | 2.04 |
| 15274915 | B | 1226 | 18.21 | 11.48, 18.21 | 5.04 | 3.61 | 3.23 |
| 19505152 | C | 417 | 26.03 | 26.03, 16.20 | 7.46 | 3.49 | 2.84 |
| 19606599 | C | 367 | 35.77 | 35.77, 26.59 | 11.04 | 3.24 | 2.31 |
| 17666333 | B | 961 | 23.92 | 23.92, 14.54 | 7.92 | 3.02 | 2.30 |
| 14334629 | B | 819 | 16.00 | 16.00, 12.46 | 5.45 | 2.93 | 2.73 |
| 19933597 | E | 154 | 8.98 | 8.98, 5.45 | 3.15 | 2.85 | 1.57 |

`3356896` on this sweep is **1.44** (board 5.42; that board figure was one 7.97 ms sample). `19932703` OCC cold is 2266 ms; its OCC median is 6.25 ms and the ratio is 1.22.

### All 49

| Block | Type | n | SF cold | SF reuse | OCC cold | OCC median | SF/OCC | Board | Win |
|------:|:----:|--:|--------:|---------:|---------:|-----------:|-------:|------:|:---:|
| 13217637 | A | 1100 | 13.94 | 10.05 | 4.96 | 4.67 | 2.15 | 1.93 | |
| 15199017 | A | 866 | 6.89 | 8.09 | 4.12 | 4.41 | 1.83 | 1.42 | |
| 8038679 | A | 237 | 1.64 | 1.70 | 1.65 | 1.26 | 1.35 | 1.39 | |
| 15538827 | B | 823 | 17.81 | 19.12 | 6.03 | 5.11 | 3.74 | 2.04 | |
| 15274915 | B | 1226 | 21.71 | 18.21 | 5.04 | 5.04 | 3.61 | 3.23 | |
| 17666333 | B | 961 | 13.84 | 23.92 | 7.92 | 7.92 | 3.02 | 2.30 | |
| 14334629 | B | 819 | 11.68 | 16.00 | 5.21 | 5.45 | 2.93 | 2.73 | |
| 14383540 | B | 722 | 10.51 | 13.08 | 6.06 | 5.57 | 2.35 | 2.78 | |
| 14029313 | B | 724 | 6.66 | 10.05 | 4.27 | 6.32 | 1.59 | 1.62 | |
| 17034869 | C | 93 | 5.56 | 13.52 | 2.34 | 2.16 | 6.25 | 2.65 | |
| 19860366 | C | 430 | 34.96 | 42.99 | 9.81 | 8.90 | 4.83 | 3.17 | |
| 8889776 | C | 330 | 9.18 | 10.48 | 2.91 | 2.51 | 4.18 | 3.50 | |
| 19505152 | C | 417 | 37.70 | 26.03 | 6.42 | 7.46 | 3.49 | 2.84 | |
| 19606599 | C | 367 | 26.97 | 35.77 | 12.68 | 11.04 | 3.24 | 2.31 | |
| 19469097 | C | 336 | 18.10 | 22.73 | 8.65 | 8.65 | 2.63 | 3.19 | |
| 19716145 | C | 341 | 17.67 | 21.70 | 9.04 | 9.04 | 2.40 | 2.39 | |
| 17034870 | C | 184 | 17.53 | 15.28 | 6.64 | 6.64 | 2.30 | 2.56 | |
| 19469099 | C | 257 | 15.05 | 12.62 | 5.51 | 5.51 | 2.29 | 2.02 | |
| 19469101 | C | 469 | 17.18 | 17.55 | 7.66 | 7.66 | 2.29 | 2.78 | |
| 19737292 | C | 195 | 9.50 | 10.51 | 4.63 | 4.63 | 2.27 | 1.93 | |
| 19932148 | C | 227 | 10.70 | 11.48 | 5.07 | 5.07 | 2.26 | 2.24 | |
| 18988207 | C | 186 | 8.60 | 10.35 | 4.55 | 4.66 | 2.22 | 2.56 | |
| 18426253 | C | 147 | 8.07 | 11.67 | 6.34 | 5.52 | 2.11 | 1.49 | |
| 12459406 | C | 201 | 14.48 | 10.68 | 6.41 | 5.09 | 2.10 | 2.04 | |
| 19469098 | C | 268 | 7.20 | 7.95 | 4.84 | 3.95 | 2.01 | 1.67 | |
| 9069000 | C | 56 | 2.58 | 4.12 | 2.33 | 2.33 | 1.77 | 2.09 | |
| 14683600 | C | 660 | 18.40 | 15.19 | 7.78 | 8.90 | 1.71 | 2.09 | |
| 14689598 | C | 111 | 2.83 | 2.72 | 1.89 | 1.89 | 1.44 | 1.58 | |
| 16257471 | C | 98 | 6.19 | 6.89 | 5.25 | 5.25 | 1.31 | 1.28 | |
| 19932703 | C | 143 | 8.65 | 7.65 | 2265.98 | 6.25 | 1.22 | 1.31 | |
| 10760440 | D | 202 | 17.67 | 9.89 | 3.81 | 3.73 | 2.65 | 2.44 | |
| 19932810 | D | 270 | 8.37 | 14.47 | 5.09 | 5.96 | 2.43 | 2.52 | |
| 5283152 | D | 150 | 2.43 | 3.16 | 1.55 | 1.50 | 2.11 | 2.15 | |
| 19933597 | E | 154 | 5.91 | 8.98 | 3.15 | 3.15 | 2.85 | 1.57 | |
| 19917570 | E | 116 | 14.98 | 8.29 | 3.47 | 3.41 | 2.43 | 1.89 | |
| 19929064 | E | 103 | 3.95 | 6.55 | 2.75 | 2.75 | 2.38 | 2.60 | |
| 15752489 | E | 132 | 2.85 | 3.67 | 2.46 | 1.76 | 2.08 | 2.20 | |
| 12243999 | E | 205 | 4.72 | 5.78 | 3.25 | 2.87 | 2.02 | 2.30 | |
| 12159808 | E | 180 | 6.10 | 8.17 | 4.39 | 4.39 | 1.86 | 1.62 | |
| 16146267 | E | 473 | 6.80 | 7.39 | 4.08 | 3.98 | 1.86 | 3.27 | |
| 12244000 | E | 133 | 12.70 | 9.25 | 5.37 | 5.09 | 1.82 | 1.92 | |
| 19638737 | E | 381 | 10.57 | 8.03 | 4.44 | 4.44 | 1.81 | 4.09 | |
| 11743952 | E | 206 | 17.49 | 13.57 | 9.05 | 9.03 | 1.50 | 2.29 | |
| 3356896 | E | 176 | 2.65 | 2.00 | 1.39 | 1.39 | 1.44 | 5.42 | |
| 11114732 | E | 100 | 5.14 | 5.26 | 3.93 | 3.93 | 1.34 | 1.34 | |
| 19606598 | E | 91 | 2.04 | 1.87 | 1.46 | 1.46 | 1.28 | 1.41 | |
| 19934116 | E | 58 | 1.30 | 1.27 | 1.45 | 1.28 | 0.99 | 0.92 | seq |
| 19426587 | E | 37 | 2.03 | 2.02 | 2.30 | 2.04 | 0.99 | 0.97 | seq |
| 19933122 | E | 45 | 0.47 | 0.46 | 0.58 | 0.50 | 0.91 | 0.88 | seq |

## Dig-10 counters (slow reuse vs the dig)

Same ten blocks, Instant-off N=3, this engine. `dig` columns are the pre-package deep dive. Partial is rebind+rewind. `ppt` is picks per tx.

| Block | Ratio | Dig | Full | Dig Full | Ord | Part | E5 | mid | Hole ms | Dig hole | ppt | Dig ppt |
|------:|------:|----:|-----:|---------:|----:|-----:|---:|----:|--------:|---------:|----:|--------:|
| 3356896 | 2.68 | 2.20 | 34 | 30 | 17 | 0 | 33 | 0 | 1.09 | 1.31 | 2.14 | 1.96 |
| 19638737 | 2.35 | 2.05 | 26 | 27 | 16 | 0 | 26 | 2 | 4.97 | 8.06 | 2.86 | 2.40 |
| 8889776 | 3.03 | 4.38 | 216 | 218 | 95 | 0 | 215 | 12 | 5.28 | 11.27 | 3.44 | 3.73 |
| 16146267 | 2.46 | 2.75 | 101 | 116 | 56 | 0 | 101 | 11 | 6.28 | 6.31 | 2.00 | 2.33 |
| 15274915 | 3.07 | 3.95 | 124 | 160 | 115 | 0 | 120 | 5 | 13.06 | 4.67 | 1.63 | 1.87 |
| 19469097 | 2.38 | 3.13 | 156 | 178 | 100 | 0 | 156 | 6 | 17.04 | 21.64 | 4.62 | 3.93 |
| 19860366 | 8.78 | 4.42 | 155 | 159 | 436 | 0 | 151 | 23 | 33.60 | 12.11 | 14.18 | 6.00 |
| 19505152 | 4.38 | 4.07 | 141 | 141 | 42 | 0 | 140 | 41 | 20.95 | 14.18 | 5.83 | 6.39 |
| 19469101 | 2.21 | 2.65 | 159 | 205 | 84 | 1 | 158 | 28 | 10.60 | 12.60 | 2.51 | 2.67 |
| 14383540 | 2.35 | 2.23 | 65 | 65 | 22 | 0 | 63 | 11 | 4.62 | 11.64 | 1.81 | 2.12 |

`occ_schedule_picks=0` and `soft_wait_arms=0` on every row. `explore_n=0` on the slow reuse. `chosen_strategy` is Opt; `selected_arms` still contains Win entries where `mid_promote_n > 0`.

## What the counters do and do not show

FullReplay fell on the C blocks that were FullReplay-heavy in the dig (`19469101` 205→159, `19469097` 178→156, `8889776` 218→216, `19860366` 159→155). Value-stable rebind still almost never fires (partial is 0 or 1). OrderedReplay is the salvage that actually runs. E5 still tracks FullReplay: the storm is smaller, not gone.

`mid_promote_n` is non-zero on 9 of the 10 slow-reuse rows. `3356896` stays 0: the thin shell records the Win in the arm table only when a patch applies, and this slow reuse did not apply one. `promote_short_edge` is off at `n ≤ 176` on purpose.

Hole blocks `8889776`, `19469097`, `19638737`, and `14383540` have a shorter `gate_stall` than the dig. `15274915` and `19860366` do not. `19860366`'s slow reuse planted 436 OrderedReplays, hole 33.6 ms, 14 picks/tx. The other reuse sample on that run was 46.6 ms. The 49 sweep's two reuse samples were 20.6 ms and 43.0 ms (ratio 4.83 vs board 3.17). Narrow-tail `19505152` picks/tx moved 6.39→5.83 on this dig sample, not to ~1.

`3356896` SF/OCC is 1.44 on the 49 sweep and 2.68 on this dig-10 sample (dig was 2.20, board 5.42). Closer to 1 on the sweep that matches the board's method. Not a stable 1.0, and the block is still above OCC.

`specfence-lab` was not updated. This credential gets "repository not found" for that remote.
