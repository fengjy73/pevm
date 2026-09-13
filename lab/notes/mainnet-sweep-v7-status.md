# Mainnet sweep v7 (plant v2 M1l metrics export)

**Date:** 2026-09-06 (Asia/Shanghai)
**Base:** `64065bb` (plant v2 M1l) + exporter plant-v2 fields / per-block checkpoint / `--modes`
**Sweep:** attempted full 7 blocks × cores 1/2/4/8 × repeats 3; SpecFence+inspect nondeterministic livelock at width≥4/8 on some blocks.

```
cargo run -p pevm --release --config 'profile.release.lto=false' --example specfence_mainnet_sweep -- \
  --out lab/results/mainnet-sweep-v7.json
```

## Hang log (honest)

| Event | Detail |
|-------|--------|
| First full sweep | Hung SpecFence@8 on **14383540** ~16.7 min (futex/inspect) → killed |
| Resume safe6 | Hung SpecFence@8 on **14683600** ~16+ min → killed (same block completed SF@8 earlier — nondeterministic) |
| Per-block runner | **19434587** SpecFence@8 repeatedly livelocks (tried 120s/900s); **SF@8 never obtained** |
| Recovered | 14383540 & 14683600 SF@8 with repeats=1 under 120s timeout; c124 repeats=1 for plant metrics |
| Coverage | Full 3-repeat @8: 19807137, 13217637, 14029313, 15199017. Partial: 14683600, 14383540 (SF/OCC@8 n=1). **19434587 SF@8 missing** (SF@4 proxy) |

## SpecFence vs OCC @8 TPS (mean) vs v5

| block | SF@8 TPS | OCC@8 TPS | SF/OCC v7 | SF/OCC v5 | notes |
|-------|----------|-----------|-----------|-----------|-------|
| 19807137 | 5756 | 67931 | 0.085 | 0.118 |  |
| 14683600 | 9586 | 70865 | 0.135 | 0.477 | n=1 |
| 13217637 | 24473 | 243507 | 0.101 | 0.214 |  |
| 14383540 | 15104 | 122835 | 0.123 | 0.302 | n=1 |
| 15199017 | 14006 | 210561 | 0.067 | 0.309 |  |
| 14029313 | 11124 | 248525 | 0.045 | 0.296 |  |
| 19434587 | 1691* | 38090 | 0.044 | 0.366 | SF@8 HUNG (inspect livelock); SF@4 used as proxy |

- Arithmetic mean SF/OCC @8 (**excl. hung proxy**): **0.092** (geo **0.087**, n=6)
- Including 19434587 SF@4/OCC@8 proxy: **0.086**
- v5 mean SF/OCC @8: **0.297** (v6 was smoke-only, gate failed — not comparable full sweep)
- **\*** 19434587: SpecFence@8 hung every attempt; SF column is **SF@4** mean.
- No block has SpecFence ≥ OCC @8. Exit criterion **not met**.

## Plant v2 / P2 metrics table (SpecFence @8, or @4 if hung)

| block | SF@8 TPS | OCC@8 TPS | SF/OCC | evm_entries SF | evm_entries OCC | absolute_jump_applied | prior_bind_hits | lean_mode_txs | wait_hard | partial_retry |
|-------|----------|-----------|--------|----------------|-----------------|-----------------------|-----------------|---------------|-----------|---------------|
| 19807137 | 5756 | 67931 | 0.085 | 1592 | 3781 | 0 | 1584 | 0 | 7292 | 906 |
| 14683600 | 9586 | 70865 | 0.135 | 731 | 3181 | 0 | 648 | 0 | 1034 | 61 |
| 13217637 | 24473 | 243507 | 0.101 | 1117 | 1294 | 0 | 1048 | 0 | 113 | 9 |
| 14383540 | 15104 | 122835 | 0.123 | 738 | 5506 | 0 | 712 | 0 | 800 | 34 |
| 15199017 | 14006 | 210561 | 0.067 | 884 | 1252 | 0 | 769 | 0 | 270 | 12 |
| 14029313 | 11124 | 248525 | 0.045 | 753 | 1343 | 0 | 414 | 0 | 167 | 14 |
| 19434587 | 1691* | 38090 | 0.044 | 510 | 969 | 1 | 540 | 0 | 2185 | 96 |

Additional plant counters (same cores as above):

| block | resume_count | journal_ff_hits | ready_steal_on_wait | inspector_steps | full_mode_txs |
|-------|--------------|-----------------|---------------------|-----------------|---------------|
| 19807137 | 1699 | 5321 | 630 | 1467978 | 3291 |
| 14683600 | 84 | 629 | 52 | 1270650 | 815 |
| 13217637 | 9 | 32 | 9 | 405348 | 1126 |
| 14383540 | 55 | 298 | 19 | 814046 | 793 |
| 15199017 | 16 | 147 | 15 | 506622 | 900 |
| 14029313 | 16 | 115 | 18 | 443585 | 769 |
| 19434587 | 156 | 1047 | 53 | 1689283 | 666 |

## Honest verdict — did plant v2 move SF/OCC?

**No — it moved the wrong way.** Mean SF/OCC @8 fell from **~0.30 (v5)** to **~0.09 (v7)** on the six blocks with real SF@8 samples. Absolute OCC TPS also differs vs v5 (machine/load noise); the within-sweep ratio is the honest comparison and is unambiguously worse.

Attribution from exported plant v2 counters:

1. **`absolute_jump_applied ≈ 0` on mainnet** (except sparse hits on 19434587 @4). M1e–M1l jump path is not amortizing mainnet prefixes; microbench wins did not transfer.
2. **`lean_mode_txs = 0` everywhere** — M4 engagement stays on `full_mode_txs` for these blocks, so the inspect/plant tax is always on.
3. **`inspector_steps` is huge** (0.4M–1.7M per block @8). That is the dominant new cost vs v5 PartialRetry-only path.
4. **Resume/prior-bind are live** (`resume_count`, `prior_bind_hits`, `journal_ff_hits`, `ready_steal_on_wait` > 0) but do not offset inspect overhead or WaitHard on hot blocks.
5. **Hang risk is real:** SpecFence+inspect livelocks nondeterministically at width 8 (and occasionally @4) — matches M1l “not yet 7-block ready” note. 19434587 SF@8 never completed.

### Recommend next

**Pause further M1m jump-width engineering aimed at mainnet TPS.** Plant v2 has not closed the SF/OCC gap; it opened an inspect tax and hang surface.

Preferred next forks (pick one):

1. **Pause / triage:** freeze jump work; profile `inspector_steps` vs OCC baseline; decide whether REM/inspect belongs on the default mainnet path at all.
2. **More engineering (hang-first):** make multi-SSTORE+LOG + mainnet SF@8 hang-free *before* any TPS claim; gate on hang rate → 0 at nproc, not on jump counts.
3. **M1m only if scoped to hang-free + lean engagement:** e.g. keep jumps off the cold lean path; force lean when `absolute_jump` cannot fire; do **not** expect SF/OCC ≫ v5 from another jump tip alone.

**Not recommended:** another mainnet-facing jump tip (M1m-as-TPS) without first removing inspect livelock and measuring lean engagement > 0 on cold blocks.

## Exporter changes

- Plant v2 fields in JSON/CSV: `absolute_jump_*`, `prior_bind_*`, `journal_ff_*`, `prefix_opcodes_skipped`, `ready_steal_on_wait`, `lean_mode_txs` / `full_mode_txs` / `engagement_switches`, `inspector_steps` / `inspector_steps_resume` (plus existing resume/rewind/rebind/full_restart/tx_head_reexec).
- OCC/PCC rows keep zeros for SpecFence-only fields.
- Per-block checkpoint write; optional `--modes` for hang recovery.

## Artifacts

- JSON: `lab/results/mainnet-sweep-v7.json` (198 rows merged)
- CSV: `lab/results/mainnet-sweep-v7.csv`
- Partials: `lab/results/mainnet-sweep-v7-b*.json`
- Log: `lab/notes/mainnet-sweep-v7.log`
- Figures: skipped (`matplotlib` not installed; tables sufficient)

