# PR #40 TPS losers — optimal vs overhead (index)

**Baseline:** PR #40 `cursor/specfence-midband-spine-tps-041c` @ `a7562676301eb173ef7a3692f89cd3b3e2cdbb75`  
**This branch:** `cursor/specfence-tps-losers-optimal-overhead-ff75`  
**Nature:** analysis only. No SpecFence CC / policy / learn change.  
**Soft=0 · Instant-off · professional terms**

## Read order

1. **[MAIN](specfence-pr40-tps-losers-optimal-vs-overhead.md)** — NEAR/FAR vs theoretical optimal arrangement; why FAR or why residual overhead if NEAR; PC / CC / learn; corpus 23/98.
2. **[Summary JSON](specfence-pr40-tps-losers-optimal-vs-overhead-summary.json)** — K-block Instant-off + DAG bounds.
3. Per-block Instant-off JSON under `lab/results/pr40-k11-optimal-overhead/`.
4. DAG / serial hat: `lab/results/pr40-k11-optimal-overhead/dag-upper-bound.json`.

## Context

- Task SoT: user question after PR #40 mid-band land (`23/98` SF TPS≥OCC).
- Corpus: [`specfence-tps-losers-midband-spine-summary.json`](specfence-tps-losers-midband-spine-summary.json) · land [`specfence-tps-losers-midband-spine-land.md`](specfence-tps-losers-midband-spine-land.md).
- Style: [`specfence-3356896-optimal-vs-overhead-pr19`](https://github.com/fengjy73/pevm) (NEAR/FAR vs equal-weight list-schedule; Instant-tax not added into wall).

## K=11 (this analysis)

| # | Block | Why selected |
|--:|------:|--------------|
| 1 | 14396881 | Worst SF/OCC TPS (0.311); large near-independent contrast |
| 2 | 15274915 | 2nd-worst TPS (0.325); large n=1226 |
| 3 | 13217637 | Worst-tier (0.360); n=1100, wait-set 8, Opt |
| 4 | 16146267 | Mid-band real spine; below PR39 |
| 5 | 19807137 | Under-covered conflict spine |
| 6 | 8889776 | Mid-band real-spine representative |
| 7 | 19638737 | Mid-band real-spine representative |
| 8 | 19716145 | Mid-band; largest named Δ vs PR39 |
| 9 | 19860366 | Mid-band; named Δ vs PR39 |
| 10 | 19469101 | Mid-large Opt leftover (key_blocks) |
| 11 | 3356896 | Thin light-cover; prior NEAR style reference |

Sweep rows for these blocks are Soft=0 in the PR #40 99-block JSON.
